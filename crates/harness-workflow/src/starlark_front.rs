//! Starlark workflow front-end.
//!
//! The SOLE dynamic authoring front-end (alongside the Rust built-ins like
//! [`super::investigate`]) that lets an agent author a real *program*
//! at runtime — loops, conditionals, data-driven fan-out — and have the harness
//! *evaluate* it. The program's `agent()` / `parallel()` host functions drive the SAME
//! ephemeral-worker backend through the injected [`AgentStepFn`] seam, and the run is
//! journaled identically (via [`super::outcome_from_steps`]).
//!
//! The interpreter is [Starlark](https://github.com/facebook/starlark-rust): hermetic by
//! design (no clock / random / IO exposed to the script), so the control plane stays
//! deterministic exactly like Claude Code's internal Workflow tool — all nondeterminism
//! lives in the journaled `agent()` leaves.
//!
//! ## Host API (the globals a script may call)
//! * `workflow(name, design_intent)` — REQUIRED meta header. Declares the run's
//!   name and the `design_intent`: a free-text explanation of WHY the workflow is
//!   structured the way it is. The run is REJECTED if `workflow(...)` is never
//!   called or `design_intent` is blank / shorter than [`MIN_DESIGN_INTENT_LEN`]
//!   characters — every workflow must justify its shape.
//! * `agent(prompt, provider="codex", label=None, phase=None, model=None, isolation=None, schema=None)`
//!   — run ONE ephemeral worker synchronously. In text mode (no `schema`) it
//!   returns the worker's output text (so the script can chain, e.g.
//!   `scan = agent(...)` then `scan.splitlines()`). In STRUCTURED mode
//!   (`schema={...}`) it forces the worker to reply with a single JSON object
//!   carrying the schema's top-level keys, then returns the parsed dict
//!   (`res["ok"]`), or `None` if the worker produced no valid JSON.
//! * `parallel(specs)` — a barrier fan-out: run every spec concurrently and block until
//!   ALL finish, returning a list in input order where each element is the parsed
//!   structured dict (if that spec had a `schema` and parsed) else its
//!   output-summary string. `specs` is a list of dicts, each with a required
//!   `prompt` and optional `provider` (default "codex"), `label`, `phase`,
//!   `model`, `isolation`, and `schema` — e.g.
//!   `parallel([{"prompt": "fix " + x} for x in args["items"]])`.
//! * `phase(name)` — set the default phase for subsequent steps.
//! * `log(message)` — emit a progress line.
//! * `args` — the run's JSON parameterization, injected as a module global value.

use std::cell::RefCell;

use starlark::any::ProvidesStaticType;
use starlark::environment::{GlobalsBuilder, LibraryExtension, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};

use crate::{outcome_from_steps, run_agent_step, AgentStepFn, AgentStepSpec, StepResult};
use crate::{scheduler_agents_spawned, WorkflowOutcome};

/// Minimum length (in characters) a `design_intent` must reach to be accepted.
/// Shorter (or blank) intents do not explain WHY the workflow is shaped as it is,
/// so the run is rejected fail-fast.
pub const MIN_DESIGN_INTENT_LEN: usize = 20;

/// An error from authoring or evaluating a Starlark workflow program. Carries the
/// human-facing message Starlark produced (parse diagnostics or a runtime error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarlarkRunError {
    /// The script failed to parse (syntax error).
    Parse(String),
    /// The script raised an error during evaluation.
    Eval(String),
    /// The mandatory `workflow(name, design_intent)` meta header was missing or
    /// its `design_intent` was blank / too short. Carries the human-facing reason.
    MissingDesignIntent(String),
}

impl std::fmt::Display for StarlarkRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StarlarkRunError::Parse(msg) => write!(f, "workflow script parse error: {msg}"),
            StarlarkRunError::Eval(msg) => write!(f, "workflow script evaluation error: {msg}"),
            StarlarkRunError::MissingDesignIntent(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for StarlarkRunError {}

/// The mandatory meta a Starlark workflow program declares via its
/// `workflow(name, design_intent)` header, returned to the caller alongside the
/// [`WorkflowOutcome`] so the CLI can journal the run's name + design_intent and
/// snapshot the authored `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMeta {
    /// The workflow name declared in the header.
    pub name: String,
    /// The free-text justification for the workflow's shape (validated non-blank
    /// and at least [`MIN_DESIGN_INTENT_LEN`] characters).
    pub design_intent: String,
    /// The raw Starlark program text that was evaluated.
    pub source: String,
}

/// The result of evaluating a Starlark workflow program: the run [`WorkflowOutcome`]
/// plus the captured [`WorkflowMeta`] (name / design_intent / source).
#[derive(Debug, Clone)]
pub struct StarlarkRun {
    pub outcome: WorkflowOutcome,
    pub meta: WorkflowMeta,
}

/// The shared evaluation context handed to every host function via `eval.extra`.
/// Holds the injected delivery driver plus the run's accumulating state. Interior
/// mutability (`RefCell`) lets the `&self`-borrowed host functions append steps and
/// move the current phase. Single-threaded for serial calls; Stage 2's `parallel()`
/// extracts plain specs before fanning out so no Starlark value crosses a thread.
#[derive(ProvidesStaticType)]
struct StarlarkCtx<'a> {
    /// The injected agent-step driver (real provider delivery, or a test mock).
    driver: &'a AgentStepFn<'a>,
    /// The default phase when a step does not name one (the workflow name).
    default_phase: String,
    /// The phase set by the most recent `phase()` call, if any.
    current_phase: RefCell<Option<String>>,
    /// Steps accumulated in call order — the run's ordered `Vec<StepResult>`.
    steps: RefCell<Vec<StepResult>>,
    /// Progress lines emitted via `log()`.
    logs: RefCell<Vec<String>>,
    /// The `(name, design_intent)` captured by the mandatory `workflow()` header,
    /// or `None` until it is called. `run_starlark` enforces that it is set.
    meta: RefCell<Option<(String, String)>>,
}

impl StarlarkCtx<'_> {
    /// The phase a new step lands in: the last `phase()` call, else the default.
    fn phase_for(&self, explicit: Option<String>) -> String {
        explicit
            .or_else(|| self.current_phase.borrow().clone())
            .unwrap_or_else(|| self.default_phase.clone())
    }

    /// Run one agent step through the driver, record it, and return its result.
    #[allow(clippy::too_many_arguments)]
    fn run_one(
        &self,
        prompt: String,
        provider: String,
        label: Option<String>,
        phase: Option<String>,
        model: Option<String>,
        isolation: Option<String>,
        schema: Option<serde_json::Value>,
    ) -> StepResult {
        let spec = AgentStepSpec {
            phase: self.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
            schema,
        };
        let result = run_agent_step(self.driver, &spec);
        self.steps.borrow_mut().push(result.clone());
        result
    }

    /// Run a barrier fan-out over already-extracted plain specs. Drives the
    /// EXISTING crate-level [`crate::parallel`] (scheduler-backed), then records
    /// every [`StepResult`] in input order and returns the results so the caller
    /// can build the script-visible return list (the structured dict when a spec
    /// had a schema and parsed, else its summary string). No Starlark value
    /// crosses a thread boundary — the specs were read off the heap before this.
    fn run_parallel(&self, specs: Vec<AgentStepSpec>) -> Vec<StepResult> {
        let results = crate::parallel(self.driver, &specs);
        self.steps.borrow_mut().extend(results.clone());
        results
    }
}

/// Downcast the evaluator's `extra` slot back to the [`StarlarkCtx`]. The slot is
/// always set by [`run_starlark`] before evaluation, so this never fails in practice.
fn ctx_of<'a, 'v>(eval: &'a Evaluator<'v, '_, '_>) -> &'a StarlarkCtx<'a> {
    eval.extra
        .expect("workflow eval.extra is always set by run_starlark")
        .downcast_ref::<StarlarkCtx>()
        .expect("workflow eval.extra is always a StarlarkCtx")
}

/// Read a single string field off a spec dict. Returns `None` when the key is
/// absent or its value is Starlark `None`; errors when present-but-not-a-string.
fn dict_str(dict: &DictRef<'_>, key: &str) -> anyhow::Result<Option<String>> {
    match dict.get_str(key) {
        None => Ok(None),
        Some(value) if value.is_none() => Ok(None),
        Some(value) => value
            .unpack_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| anyhow::anyhow!("parallel() spec field `{key}` must be a string")),
    }
}

/// Read an optional schema dict off a spec dict (the per-spec structured-output
/// schema). Returns `None` when the key is absent or Starlark `None`; errors when
/// present-but-not-a-dict. The dict is converted to a `serde_json` object via
/// [`value_to_json`] so it can ride on the plain [`AgentStepSpec`] across the
/// barrier's thread boundary.
fn dict_schema(dict: &DictRef<'_>, key: &str) -> anyhow::Result<Option<serde_json::Value>> {
    match dict.get_str(key) {
        None => Ok(None),
        Some(value) if value.is_none() => Ok(None),
        Some(value) => {
            if DictRef::from_value(value).is_none() {
                return Err(anyhow::anyhow!(
                    "parallel() spec field `{key}` must be a dict"
                ));
            }
            Ok(Some(value_to_json(value)))
        }
    }
}

/// Read a `parallel()` `specs` list (a Starlark list of dicts) into PLAIN Rust
/// [`AgentStepSpec`]s, resolving phase/label defaults via `ctx`. This happens on
/// the eval thread BEFORE any fan-out, so no Starlark value crosses a thread.
fn read_parallel_specs(
    ctx: &StarlarkCtx<'_>,
    specs: Value<'_>,
) -> anyhow::Result<Vec<AgentStepSpec>> {
    let list = ListRef::from_value(specs)
        .ok_or_else(|| anyhow::anyhow!("parallel() expects a list of spec dicts"))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter() {
        let dict = DictRef::from_value(item)
            .ok_or_else(|| anyhow::anyhow!("parallel() spec must be a dict"))?;
        let prompt = dict_str(&dict, "prompt")?
            .ok_or_else(|| anyhow::anyhow!("parallel() spec requires a `prompt` string"))?;
        let provider = dict_str(&dict, "provider")?.unwrap_or_else(|| "codex".to_string());
        let label = dict_str(&dict, "label")?;
        let phase = dict_str(&dict, "phase")?;
        let model = dict_str(&dict, "model")?;
        let isolation = dict_str(&dict, "isolation")?;
        let schema = dict_schema(&dict, "schema")?;
        out.push(AgentStepSpec {
            phase: ctx.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
            schema,
        });
    }
    Ok(out)
}

/// The workflow host functions exposed to the script.
// `agent()` exposes 8 host params (prompt + 6 kwargs + eval) — its expansion trips
// clippy's arg-count lint; the breadth is the documented host API surface.
#[allow(clippy::too_many_arguments)]
#[starlark_module]
fn workflow_globals(builder: &mut GlobalsBuilder) {
    /// Declare the workflow's mandatory meta: its `name` and a `design_intent`
    /// explaining WHY it is structured this way. Records both for the caller;
    /// `run_starlark` rejects the run if this is never called or the
    /// `design_intent` is blank / too short.
    fn workflow<'v>(
        #[starlark(require = pos)] name: String,
        #[starlark(require = pos)] design_intent: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        *ctx_of(eval).meta.borrow_mut() = Some((name, design_intent));
        Ok(NoneType)
    }

    /// Run one ephemeral provider worker synchronously.
    ///
    /// In TEXT mode (no `schema`) it returns the worker's output text (so the
    /// script can chain it). In STRUCTURED mode (`schema={...}`) it forces the
    /// worker to reply with a single JSON object carrying the schema's top-level
    /// keys and returns the parsed dict (e.g. `res["ok"]`); if the worker never
    /// produced valid JSON it returns `None` so the script can check/skip.
    fn agent<'v>(
        #[starlark(require = pos)] prompt: String,
        #[starlark(require = named, default = "codex".to_string())] provider: String,
        #[starlark(require = named)] label: Option<String>,
        #[starlark(require = named)] phase: Option<String>,
        #[starlark(require = named)] model: Option<String>,
        #[starlark(require = named)] isolation: Option<String>,
        #[starlark(require = named)] schema: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let schema_json = match schema {
            Some(value) if !value.is_none() => {
                if DictRef::from_value(value).is_none() {
                    return Err(anyhow::anyhow!("agent() `schema` must be a dict"));
                }
                Some(value_to_json(value))
            }
            _ => None,
        };
        let has_schema = schema_json.is_some();
        let result = ctx_of(eval).run_one(
            prompt,
            provider,
            label,
            phase,
            model,
            isolation,
            schema_json,
        );
        let heap = eval.heap();
        if has_schema {
            // Structured mode: hand the script the parsed dict, or `None` when the
            // worker never produced valid JSON (so the script can check/skip).
            match &result.structured {
                Some(structured) => Ok(json_to_value(heap, structured)),
                None => Ok(Value::new_none()),
            }
        } else {
            // Text mode: return the output summary string exactly as before.
            Ok(heap.alloc(result.output_summary.as_str()))
        }
    }

    /// Run a barrier fan-out: every spec runs concurrently and the call blocks
    /// until ALL of them finish (the barrier), then returns a list in input
    /// order. Each element is the parsed structured dict when that spec carried a
    /// `schema` and the worker produced valid JSON, else its output-summary
    /// string (schema-less specs stay backward compatible). `specs` is a list of
    /// dicts, each with a required `prompt` and optional `provider` (default
    /// "codex"), `label`, `phase`, `model`, `isolation`, and `schema`.
    fn parallel<'v>(
        #[starlark(require = pos)] specs: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let ctx = ctx_of(eval);
        // Extract every spec into PLAIN Rust before any threading — no Starlark
        // value may cross the barrier's thread boundary.
        let extracted = read_parallel_specs(ctx, specs)?;
        let results = ctx.run_parallel(extracted);
        let heap = eval.heap();
        let values: Vec<Value<'v>> = results
            .iter()
            .map(|result| match &result.structured {
                // A spec with a schema that parsed → hand the script the dict.
                Some(structured) => json_to_value(heap, structured),
                // Schema-less (or unparsed) → the summary string, as before.
                None => heap.alloc(result.output_summary.as_str()),
            })
            .collect();
        Ok(heap.alloc(values))
    }

    /// Set the default phase for subsequent steps that do not name their own.
    fn phase<'v>(
        #[starlark(require = pos)] name: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        *ctx_of(eval).current_phase.borrow_mut() = Some(name);
        Ok(NoneType)
    }

    /// Emit a progress line (collected for the run's narration).
    fn log<'v>(
        #[starlark(require = pos)] message: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        ctx_of(eval).logs.borrow_mut().push(message);
        Ok(NoneType)
    }
}

/// Recursively allocate a [`serde_json::Value`] as a Starlark value on `heap`, so a
/// run's `args` can be injected as a real Starlark global the script reads directly
/// (e.g. `args["area"]`). Numbers prefer i64, falling back to f64.
fn json_to_value<'v>(heap: Heap<'v>, value: &serde_json::Value) -> Value<'v> {
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
fn value_to_json(value: Value<'_>) -> serde_json::Value {
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
    // Snapshot the scheduler's lifetime spawn counter so the delta attributes this
    // run's agents.
    let spawned_before = scheduler_agents_spawned();

    let ctx = StarlarkCtx {
        driver,
        default_phase: name.to_string(),
        current_phase: RefCell::new(None),
        steps: RefCell::new(Vec::new()),
        logs: RefCell::new(Vec::new()),
        meta: RefCell::new(None),
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
    let outcome = outcome_from_steps(name, steps, spawned_before);
    Ok(StarlarkRun {
        outcome,
        meta: WorkflowMeta {
            name: meta_name,
            design_intent: trimmed.to_string(),
            source: script.to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::WorkflowRunStatus;
    use std::sync::Mutex;

    /// The mandatory meta header every test program must declare. Prepended to the
    /// per-test body so the run is not rejected for a missing `design_intent`.
    const HEADER: &str =
        "workflow(\"demo\", \"scan then fix: serialize so the fix builds on the scan output\")\n";

    /// A mock driver that always succeeds and records invocation order + prompts.
    fn recording_driver<'a>(
        seen: &'a Mutex<Vec<(String, String)>>,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
        move |spec: &AgentStepSpec| {
            seen.lock()
                .unwrap()
                .push((spec.label.clone(), spec.prompt.clone()));
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some(format!("session-{}", spec.label)),
                output_summary: format!("ok: {}", spec.prompt),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
            }
        }
    }

    #[test]
    fn two_serial_agents_produce_two_completed_steps() {
        let seen = Mutex::new(Vec::new());
        let script = r#"
phase("scan")
a = agent("scan the code")
phase("fix")
b = agent("fix what scan found: " + a, provider = "claude", label = "fixer")
"#;
        let outcome = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, "codex"); // default provider → default label
        assert_eq!(seen[1].0, "fixer");
        // The second prompt chained the first's output text.
        assert!(seen[1].1.contains("ok: scan the code"));
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(outcome.steps[0].phase, "scan");
        assert_eq!(outcome.steps[1].phase, "fix");
        assert_eq!(outcome.steps[1].provider, "claude");
    }

    #[test]
    fn args_are_injected_as_a_global() {
        let seen = Mutex::new(Vec::new());
        let script = r#"agent("audit " + args["area"])"#;
        let args = serde_json::json!({ "area": "checkout flow" });
        let outcome = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", Some(&args), &driver)
                .expect("run ok")
                .outcome
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].1.contains("audit checkout flow"));
        assert_eq!(outcome.steps.len(), 1);
    }

    #[test]
    fn a_failed_step_makes_the_run_failed() {
        // A driver that fails every step → 0 ok → Failed (outcome_from_steps rule).
        let driver = |spec: &AgentStepSpec| StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: false,
            provider_session_id: None,
            output_summary: "boom".to_string(),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
        };
        let outcome = run_starlark(&format!("{HEADER}agent(\"x\")"), "demo", None, &driver)
            .expect("run ok")
            .outcome;
        assert_eq!(outcome.status, WorkflowRunStatus::Failed);
        assert_eq!(outcome.steps.len(), 1);
    }

    #[test]
    fn parallel_data_driven_comprehension_runs_every_spec() {
        // A DATA-DRIVEN fan-out: the script builds the spec list from `args` via
        // a list comprehension, so N items → exactly N concurrent steps, all
        // collected by the barrier.
        let seen = Mutex::new(Vec::new());
        let script = r#"results = parallel([{"prompt": "fix " + x} for x in args["items"]])"#;
        let args = serde_json::json!({ "items": ["a", "b", "c", "d"] });
        let outcome = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", Some(&args), &driver)
                .expect("run ok")
                .outcome
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 4, "one step per comprehension item");
        assert_eq!(outcome.steps.len(), 4, "barrier collected every slot");
        // Every prompt flowed through and the run completed.
        let prompts: Vec<String> = seen.iter().map(|(_, prompt)| prompt.clone()).collect();
        for item in ["fix a", "fix b", "fix c", "fix d"] {
            assert!(prompts.contains(&item.to_string()), "missing prompt {item}");
        }
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
    }

    #[test]
    fn parallel_isolation_kwarg_flows_onto_the_spec() {
        // The `isolation` dict key must reach the AgentStepSpec the driver sees.
        let isolations = Mutex::new(Vec::<Option<String>>::new());
        let script = r#"parallel([{"prompt": "edit", "isolation": "worktree"}])"#;
        let outcome = {
            let driver = |spec: &AgentStepSpec| {
                isolations.lock().unwrap().push(spec.isolation.clone());
                StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: true,
                    provider_session_id: Some("s".to_string()),
                    output_summary: "ok".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                }
            };
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        let isolations = isolations.into_inner().unwrap();
        assert_eq!(isolations, vec![Some("worktree".to_string())]);
        assert_eq!(outcome.steps.len(), 1);
        assert_eq!(outcome.steps[0].isolation.as_deref(), Some("worktree"));
    }

    #[test]
    fn a_syntax_error_is_a_parse_error() {
        let seen = Mutex::new(Vec::new());
        let driver = recording_driver(&seen);
        let err = run_starlark("agent(", "demo", None, &driver).expect_err("should fail");
        assert!(matches!(err, StarlarkRunError::Parse(_)));
    }

    #[test]
    fn missing_workflow_header_is_rejected() {
        // A program that never calls `workflow(...)` is rejected fail-fast.
        let seen = Mutex::new(Vec::new());
        let driver = recording_driver(&seen);
        let err = run_starlark(r#"agent("x")"#, "demo", None, &driver).expect_err("rejected");
        assert!(matches!(err, StarlarkRunError::MissingDesignIntent(_)));
        assert!(err.to_string().contains("design_intent"));
    }

    #[test]
    fn blank_or_short_design_intent_is_rejected() {
        let seen = Mutex::new(Vec::new());
        let driver = recording_driver(&seen);
        // Too short (< MIN_DESIGN_INTENT_LEN) and blank both fail.
        for intent in ["too short", "   "] {
            let script = format!("workflow(\"demo\", \"{intent}\")\nagent(\"x\")");
            let err = run_starlark(&script, "demo", None, &driver).expect_err("rejected");
            assert!(
                matches!(err, StarlarkRunError::MissingDesignIntent(_)),
                "intent {intent:?} should be rejected"
            );
        }
    }

    #[test]
    fn captured_meta_is_returned_to_the_caller() {
        // A valid header is captured and returned alongside the outcome.
        let seen = Mutex::new(Vec::new());
        let script =
            "workflow(\"triage\", \"fan out one fix per defect the scan found\")\nagent(\"x\")";
        let run = {
            let driver = recording_driver(&seen);
            run_starlark(script, "demo", None, &driver).expect("run ok")
        };
        assert_eq!(run.meta.name, "triage");
        assert_eq!(
            run.meta.design_intent,
            "fan out one fix per defect the scan found"
        );
        assert_eq!(run.meta.source, script);
        assert_eq!(run.outcome.steps.len(), 1);
    }

    /// A driver that emulates the schema-mode contract: when the spec carries a
    /// schema it returns a `structured` object (each required key -> "v:<key>");
    /// otherwise it returns text only. Records the schema each step saw so a test
    /// can assert the schema threaded through onto the spec.
    fn structured_driver<'a>(
        schemas: &'a Mutex<Vec<Option<serde_json::Value>>>,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
        move |spec: &AgentStepSpec| {
            schemas.lock().unwrap().push(spec.schema.clone());
            let structured = spec.schema.as_ref().map(|schema| {
                let obj: serde_json::Map<String, serde_json::Value> = schema
                    .as_object()
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|k| (k.clone(), serde_json::Value::String(format!("v:{k}"))))
                    .collect();
                serde_json::Value::Object(obj)
            });
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some("s".to_string()),
                output_summary: format!("text: {}", spec.prompt),
                step_id: None,
                started_at: None,
                details: None,
                structured,
            }
        }
    }

    #[test]
    fn value_to_json_round_trips_with_json_to_value() {
        // A nested JSON value -> Starlark value -> JSON value must be identical.
        let original = serde_json::json!({
            "ok": true,
            "count": 3,
            "name": "audit",
            "tags": ["a", "b"],
            "nested": { "k": 1, "flag": false },
            "missing": serde_json::Value::Null,
        });
        Module::with_temp_heap(|module| {
            let value = json_to_value(module.heap(), &original);
            let back = value_to_json(value);
            assert_eq!(back, original);
            Ok::<(), starlark::Error>(())
        })
        .expect("round trip");
    }

    #[test]
    fn agent_with_schema_returns_a_dict_the_script_reads() {
        // agent(prompt, schema={...}) returns the parsed dict, so the script can
        // read a key off it (res["ok"]) and branch on it.
        let schemas = Mutex::new(Vec::new());
        let script = r#"
res = agent("audit it", schema = {"ok": "", "summary": ""})
if res["ok"] == "v:ok":
    log("structured ok: " + res["summary"])
"#;
        let outcome = {
            let driver = structured_driver(&schemas);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        let schemas = schemas.into_inner().unwrap();
        assert_eq!(schemas.len(), 1);
        // The schema dict threaded onto the spec as a JSON object.
        assert_eq!(
            schemas[0],
            Some(serde_json::json!({ "ok": "", "summary": "" }))
        );
        assert_eq!(outcome.steps.len(), 1);
        // The step carried the parsed structured object.
        assert_eq!(
            outcome.steps[0].structured,
            Some(serde_json::json!({ "ok": "v:ok", "summary": "v:summary" }))
        );
    }

    #[test]
    fn agent_without_schema_returns_the_text_summary() {
        // No schema -> the script gets the output_summary STRING exactly as today.
        let schemas = Mutex::new(Vec::new());
        let script = r#"
out = agent("scan it")
log("got: " + out)
"#;
        let outcome = {
            let driver = structured_driver(&schemas);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        assert_eq!(outcome.steps.len(), 1);
        assert!(outcome.steps[0].structured.is_none());
        assert_eq!(outcome.steps[0].output_summary, "text: scan it");
    }

    #[test]
    fn json_encode_decode_is_available_to_scripts() {
        // The `Json` library extension exposes json.encode/json.decode so a
        // program can serialize a structured value and inject it verbatim into a
        // downstream prompt (the forward-injection mechanism).
        let seen = Mutex::new(Vec::new());
        let script = r#"
data = {"verdict": "pass", "score": 100}
encoded = json.encode(data)
roundtrip = json.decode(encoded)
agent("use this: " + encoded + " score=" + str(roundtrip["score"]))
"#;
        let outcome = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 1);
        let prompt = &seen[0].1;
        assert!(
            prompt.contains("\"verdict\":\"pass\""),
            "encoded JSON injected into the prompt: {prompt}"
        );
        assert!(
            prompt.contains("score=100"),
            "decoded value usable in the script: {prompt}"
        );
        assert_eq!(outcome.steps.len(), 1);
    }

    #[test]
    fn parallel_returns_structured_dicts_and_summary_strings_per_spec() {
        // A mixed barrier: one spec has a schema (returns a dict), one does not
        // (returns its summary string). Both flow back through parallel().
        let schemas = Mutex::new(Vec::new());
        let script = r#"
results = parallel([
    {"prompt": "a", "schema": {"verdict": ""}},
    {"prompt": "b"},
])
log("first verdict: " + results[0]["verdict"])
log("second: " + results[1])
"#;
        let outcome = {
            let driver = structured_driver(&schemas);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        assert_eq!(outcome.steps.len(), 2);
        assert_eq!(
            outcome.steps[0].structured,
            Some(serde_json::json!({ "verdict": "v:verdict" }))
        );
        assert!(outcome.steps[1].structured.is_none());
    }
}
