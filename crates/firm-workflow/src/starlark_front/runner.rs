use super::*;

pub(super) fn json_to_value<'v>(heap: Heap<'v>, value: &serde_json::Value) -> Value<'v> {
    use serde_json::Value as J;
    match value {
        J::Null => Value::new_none(),
        J::Bool(b) => Value::new_bool(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                heap.alloc(i)
            } else {
                heap.alloc(n.as_f64().unwrap_or(0.0))
            }
        }
        J::String(s) => heap.alloc(s.as_str()),
        J::Array(items) => {
            let values: Vec<Value<'v>> = items.iter().map(|v| json_to_value(heap, v)).collect();
            heap.alloc(values)
        }
        J::Object(map) => {
            use starlark::values::dict::AllocDict;
            let entries: Vec<(Value<'v>, Value<'v>)> = map
                .iter()
                .map(|(k, v)| (heap.alloc(k.as_str()), json_to_value(heap, v)))
                .collect();
            heap.alloc(AllocDict(entries))
        }
    }
}

/// The mirror of [`json_to_value`]: recursively read a Starlark value into a
/// [`serde_json::Value`] so a script-supplied `schema` dict can be carried on the
/// plain [`AgentStepSpec`] across the barrier's thread boundary. Dicts become
/// objects, lists become arrays, strings/bools/ints/floats map directly, and
/// Starlark `None` becomes JSON null. Any value that is none of these (a
/// function, say) is dropped to JSON null — a schema only carries plain data.
pub(super) fn value_to_json(value: Value<'_>) -> serde_json::Value {
    use serde_json::Value as J;
    if value.is_none() {
        return J::Null;
    }
    if let Some(b) = value.unpack_bool() {
        return J::Bool(b);
    }
    if let Some(i) = value.unpack_i32() {
        return J::Number(i.into());
    }
    if let Some(s) = value.unpack_str() {
        return J::String(s.to_string());
    }
    if let Some(dict) = DictRef::from_value(value) {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key = k
                .unpack_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| k.to_str());
            map.insert(key, value_to_json(v));
        }
        return J::Object(map);
    }
    if let Some(list) = ListRef::from_value(value) {
        return J::Array(list.iter().map(value_to_json).collect());
    }
    // Floats (and any other numeric form not covered above) are not directly
    // unpackable; fall back to Starlark's own JSON conversion, else JSON null.
    value.to_json_value().unwrap_or(J::Null)
}

/// Evaluate a Starlark workflow program, driving every `agent()` call through
/// `driver`, and build a [`WorkflowOutcome`] from the steps it produced.
///
/// `name` is the workflow name (becomes the run's name and the default phase).
/// `args` is injected as the `args` global. The interpreter is hermetic: the script
/// has no access to the clock, randomness, or IO, so the orchestration is
/// deterministic — only the journaled `agent()` leaves are nondeterministic.
///
/// The program MUST call `workflow(name, design_intent)` exactly once: the run is
/// rejected with [`StarlarkRunError::MissingDesignIntent`] if it does not, or if
/// the declared `design_intent` is blank / under [`MIN_DESIGN_INTENT_LEN`]
/// characters. On success the returned [`StarlarkRun`] carries the captured meta
/// (name / design_intent / source) alongside the outcome.
pub fn run_starlark(
    script: &str,
    name: &str,
    args: Option<&serde_json::Value>,
    driver: &AgentStepFn<'_>,
) -> Result<StarlarkRun, StarlarkRunError> {
    run_starlark_with_budget(script, name, args, driver, None, None)
}

/// Like [`run_starlark`] but with an optional per-run spend ceiling in USD (the
/// CLI `--max-budget-usd`, also lowerable by a `workflow(budget_usd=…)` header)
/// and an optional `replay` cache for `--resume`.
///
/// Once cumulative step cost reaches the budget, further `agent()` / `parallel()`
/// calls are short-circuited into failed `budget` steps instead of dispatching
/// workers. When `replay` is `Some`, each leaf's deterministic ordinal is looked
/// up in the map: a hit reuses the prior run's succeeded [`StepResult`] WITHOUT
/// dispatching the worker and WITHOUT tallying budget (the no-re-spend goal); a
/// miss dispatches for real. `pipeline()` leaves are excluded from replay in v1.
pub fn run_starlark_with_budget(
    script: &str,
    name: &str,
    args: Option<&serde_json::Value>,
    driver: &AgentStepFn<'_>,
    budget_usd: Option<f64>,
    replay: Option<HashMap<u64, StepResult>>,
) -> Result<StarlarkRun, StarlarkRunError> {
    // Snapshot the scheduler's lifetime spawn counter so the delta attributes this
    // run's agents.
    let spawned_before = scheduler_agents_spawned();

    let ctx = StarlarkCtx {
        driver,
        default_phase: name.to_string(),
        current_phase: RefCell::new(None),
        steps: RefCell::new(Vec::new()),
        logs: RefCell::new(Vec::new()),
        patch_actions: RefCell::new(Vec::new()),
        artifact_manifest_requests: RefCell::new(Vec::new()),
        meta: RefCell::new(None),
        budget_usd: Cell::new(budget_usd),
        spent_usd: Cell::new(0.0),
        verdict: RefCell::new(None),
        success_criterion: RefCell::new(None),
        output: RefCell::new(None),
        ordinal_next: Cell::new(0),
        replay: replay.unwrap_or_default(),
    };

    // `Extended` enables top-level statements (so an agent can write top-level
    // `for`/`if`), def, and lambdas — the expressive program shape we want.
    let ast = AstModule::parse("workflow.star", script.to_owned(), &Dialect::Extended)
        .map_err(|error| StarlarkRunError::Parse(error.to_string()))?;
    // `Json` adds `json.encode`/`json.decode` so a program can serialize a prior
    // `agent()`'s structured dict and inject it verbatim into the next prompt —
    // the forward-injection mechanism the orchestration patterns rely on.
    let globals = GlobalsBuilder::extended_by(&[LibraryExtension::Json])
        .with(workflow_globals)
        .build();

    // Evaluate inside a scoped temp heap. The `ctx` lives outside the closure so
    // its accumulated steps survive the heap teardown; only the Starlark values
    // (and `args`) are heap-bound.
    Module::with_temp_heap(|module| {
        if let Some(args) = args {
            let value = json_to_value(module.heap(), args);
            module.set("args", value);
        }
        let mut eval = Evaluator::new(&module);
        eval.extra = Some(&ctx);
        eval.eval_module(ast, &globals)
            .map(|_| ())
            .map_err(|error| StarlarkRunError::Eval(error.to_string()))
    })?;

    // ENFORCE the mandatory meta header: `workflow(name, design_intent)` must
    // have run, and the design_intent must be a real (>= MIN_DESIGN_INTENT_LEN
    // chars) justification — every workflow must explain WHY it is shaped so.
    let (meta_name, design_intent) = ctx.meta.into_inner().ok_or_else(|| {
        StarlarkRunError::MissingDesignIntent(
            "every workflow must declare a design_intent explaining WHY it is structured \
             this way: call workflow(name, design_intent) at the top of the program"
                .to_string(),
        )
    })?;
    let trimmed = design_intent.trim();
    if trimmed.chars().count() < MIN_DESIGN_INTENT_LEN {
        return Err(StarlarkRunError::MissingDesignIntent(format!(
            "every workflow must declare a design_intent explaining WHY it is structured \
             this way: design_intent must be at least {MIN_DESIGN_INTENT_LEN} characters \
             (got {})",
            trimmed.chars().count()
        )));
    }

    let steps = ctx.steps.into_inner();
    let logs = ctx.logs.into_inner();
    let patch_actions = ctx.patch_actions.into_inner();
    let artifact_manifest_requests = ctx.artifact_manifest_requests.into_inner();
    let verdict = ctx.verdict.into_inner();
    let success_criterion = ctx.success_criterion.into_inner();
    let output = ctx.output.into_inner();
    let mut outcome = outcome_from_steps(name, steps, spawned_before);
    // A declared verdict makes the run status INTENT-RELATIVE: mechanical
    // step-success becomes necessary-but-not-sufficient, so a run whose workers all
    // ran but whose self-check failed reports Failed (not a misleading Completed).
    if let Some((ok, reason)) = &verdict {
        outcome.status = if *ok {
            WorkflowRunStatus::Completed
        } else {
            WorkflowRunStatus::Failed
        };
        let crit = success_criterion
            .as_deref()
            .map(|c| format!(" [criterion: {c}]"))
            .unwrap_or_default();
        let why = if reason.trim().is_empty() {
            String::new()
        } else {
            format!(" — {reason}")
        };
        outcome.summary = format!(
            "{name} verdict: intent {}{crit}{why}",
            if *ok { "met" } else { "NOT met" }
        );
    }
    // Persist the run's NARRATION + GRADING metadata into final_output so it
    // survives the run instead of being dropped: the declared `output()` RESULT, the
    // `log()` lines, the typed `verdict`, and the declared `success_criterion`. The
    // per-step array moves under `steps`. The calling agent reads `result` as the
    // run's one unambiguous answer; everything else is auditable post-hoc.
    let steps_output = outcome.final_output.take();
    outcome.final_output = Some(serde_json::json!({
        "result": output,
        "steps": steps_output,
        "logs": logs,
        "patch_actions": patch_actions,
        "artifact_manifests": artifact_manifest_requests,
        "verdict": verdict
            .as_ref()
            .map(|(ok, reason)| serde_json::json!({ "ok": ok, "reason": reason })),
        "success_criterion": success_criterion,
    }));
    Ok(StarlarkRun {
        outcome,
        meta: WorkflowMeta {
            name: meta_name,
            design_intent: trimmed.to_string(),
            success_criterion,
            source: script.to_string(),
        },
    })
}
