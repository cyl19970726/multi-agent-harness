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
//! * `agent(prompt, provider="codex", label=None, phase=None, model=None, timeout_s=None, write_mode=None, isolation=None, schema=None, schema_strict=False, return_status=False)`
//!   — run ONE ephemeral worker synchronously. In text mode (no `schema`) it
//!   returns the worker's output text (so the script can chain, e.g.
//!   `scan = agent(...)` then `scan.splitlines()`). In STRUCTURED mode
//!   (`schema={...}`) it forces the worker to reply with a single JSON object
//!   carrying the schema's top-level keys, then returns the parsed dict
//!   (`res["ok"]`), or `None` if the worker produced no valid JSON. With
//!   `schema_strict=True`, a candidate object whose top-level string fields are
//!   all empty is REJECTED (as if it did not parse) so a later meaningful object
//!   is selected instead — top-level type success is not semantic acceptance
//!   (#192). With `return_status=True`, it returns an inspectable status dict
//!   carrying `ok`, `reason`, `detail`, `failure`, `text`, and `structured` so
//!   scripts can branch on failed leaves.
//! * `parallel(specs)` — a barrier fan-out: run every spec concurrently and block until
//!   ALL finish, returning a list in input order where each element is the parsed
//!   structured dict (if that spec had a `schema` and parsed) else its
//!   output-summary string. `specs` is a list of dicts, each with a required
//!   `prompt` and optional `provider` (default "codex"), `label`, `phase`,
//!   `model`, `timeout_s`, `write_mode`, `isolation`, `schema`, `schema_strict`,
//!   and `return_status` — e.g.
//!   `parallel([{"prompt": "fix " + x} for x in args["items"]])`.
//! * `pipeline(items, stages)` — a STREAMING fan-out: every item flows through ALL
//!   `stages` in order with NO barrier between stages (item A may be in stage 3
//!   while item B is still in stage 1). Returns a list in input order, one element
//!   per item: the LAST stage's parsed structured dict (if it had a `schema` and
//!   parsed) else its output-summary string. `items` is a list of strings OR dicts;
//!   `stages` is a list of stage dicts (`prompt` TEMPLATE + optional `provider`,
//!   `label`, `phase`, `model`, `timeout_s`, `schema`, `schema_strict`,
//!   `writable`, `return_status`). Each stage `prompt` may contain the literal `{input}`
//!   placeholder, FORWARD-INJECTED with the item
//!   (stage 1) or the prior stage's output (stage N) — e.g.
//!   `pipeline(args["files"], [{"prompt": "scan {input}"}, {"prompt": "fix per {input}"}])`.
//! * `phase(name)` — set the default phase for subsequent steps.
//! * `log(message)` — emit a progress line.
//! * `args` — the run's JSON parameterization, injected as a module global value.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use starlark::any::ProvidesStaticType;
use starlark::environment::{GlobalsBuilder, LibraryExtension, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::none::NoneType;
use starlark::values::tuple::UnpackTuple;
use starlark::values::{Heap, Value};

use firm_core::WorkflowRunStatus;

use crate::{
    outcome_from_steps, run_agent_step, AgentStepFn, AgentStepSpec, StepResult, WRITE_MODE_DIRECT,
};
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
    /// The success criterion declared via `workflow(..., success_criterion=…)`,
    /// if any — the bar the run's `verdict()` is judged against.
    pub success_criterion: Option<String>,
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
    /// Patch decisions declared by workflow code after it reviewed a step.
    patch_actions: RefCell<Vec<serde_json::Value>>,
    /// Artifact manifest requests declared by workflow code.
    artifact_manifest_requests: RefCell<Vec<serde_json::Value>>,
    /// The `(name, design_intent)` captured by the mandatory `workflow()` header,
    /// or `None` until it is called. `run_starlark` enforces that it is set.
    meta: RefCell<Option<(String, String)>>,
    /// The per-run spend ceiling in USD, if any (the CLI `--max-budget-usd` or the
    /// smaller `workflow(budget_usd=…)` header). `None` = unbounded.
    budget_usd: Cell<Option<f64>>,
    /// Cumulative USD spent so far across this run's completed steps — real billed
    /// cost where the provider reports it (claude), else a token-based estimate.
    spent_usd: Cell<f64>,
    /// The typed run verdict declared via `verdict(ok, reason)`, if any. When set
    /// it makes the run status intent-relative (ok=false → Failed even if every
    /// worker ran). `None` = fall back to the step-success rule.
    verdict: RefCell<Option<(bool, String)>>,
    /// The success criterion from the `workflow(..., success_criterion=…)` header,
    /// surfaced in the run summary alongside the verdict.
    success_criterion: RefCell<Option<String>>,
    /// The run's declared RESULT — the first-class return value an author sets via
    /// `output(value)`. Persisted verbatim under `final_output.result` (NOT subject
    /// to the per-step `output_summary` cap), so the calling agent reads one
    /// unambiguous field instead of digging the answer out of a step by label. The
    /// last call wins. `None` = the script never declared one.
    output: RefCell<Option<serde_json::Value>>,
    /// Monotonic deterministic leaf-ordinal counter. Assigned ON THE EVAL THREAD
    /// (single-threaded, before any fan-out) so the Nth leaf of a re-run equals the
    /// Nth originally as long as control flow matches. One ordinal per `agent()`
    /// leaf, per `parallel()` spec (in input order), and per pipeline item×stage.
    ordinal_next: Cell<u64>,
    /// The replay cache for `--resume`: a map from leaf ordinal to the prior run's
    /// succeeded [`StepResult`] for that ordinal. When a leaf's ordinal hits, its
    /// cached result is returned WITHOUT dispatching the (paid) worker and WITHOUT
    /// tallying budget. Empty when not resuming. Read-only during eval.
    replay: HashMap<u64, StepResult>,
}

impl StarlarkCtx<'_> {
    /// The phase a new step lands in: the last `phase()` call, else the default.
    fn phase_for(&self, explicit: Option<String>) -> String {
        explicit
            .or_else(|| self.current_phase.borrow().clone())
            .unwrap_or_else(|| self.default_phase.clone())
    }

    /// Allocate the next deterministic leaf ordinal. Called on the eval thread in
    /// issue order, so the Nth call returns N (0-based) on every hermetic re-run.
    fn next_ordinal(&self) -> u64 {
        let n = self.ordinal_next.get();
        self.ordinal_next.set(n + 1);
        n
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
        effort: Option<String>,
        service_tier: Option<String>,
        fallback_model: Option<String>,
        timeout_s: Option<u64>,
        image: Vec<String>,
        add_dir: Vec<String>,
        expected_artifacts: Vec<String>,
        persist_changes: Option<String>,
        write_mode: Option<String>,
        owned_paths: Vec<String>,
        artifact_root: Option<String>,
        write_roots: Vec<String>,
        auto_apply_on_verdict: bool,
        isolation: Option<String>,
        schema: Option<serde_json::Value>,
        schema_strict: bool,
        writable: bool,
    ) -> StepResult {
        // Assign this leaf's deterministic ordinal FIRST (before the replay lookup,
        // budget check, or dispatch) so it is stable across re-runs.
        let ord = self.next_ordinal();

        // Replay hit: reuse the prior run's succeeded result for this ordinal
        // WITHOUT dispatching the worker and WITHOUT tallying budget (no re-spend).
        if let Some(cached) = self.replay.get(&ord) {
            // Journal a MARKED copy (audit/[replayed] prefix) into ctx.steps, but
            // return an UNMARKED copy to the script: the prior run's original
            // `output_summary` is what `agent()` hands the program in text mode, so
            // a marker here would corrupt downstream prompts and can divert control
            // flow (branching on agent text) and desynchronize every later ordinal.
            let mut journaled = cached.clone();
            journaled.ordinal = Some(ord);
            mark_replayed(&mut journaled);
            self.steps.borrow_mut().push(journaled);

            let mut returned = cached.clone();
            returned.ordinal = Some(ord);
            return returned;
        }

        let spec = AgentStepSpec {
            phase: self.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            effort,
            service_tier,
            fallback_model,
            timeout_s,
            image,
            add_dir,
            expected_artifacts,
            persist_changes,
            write_mode,
            owned_paths,
            artifact_root,
            write_roots,
            auto_apply_on_verdict,
            isolation,
            prompt,
            schema,
            schema_strict,
            writable,
            // Thread the ordinal onto the spec so a real driver that journals its
            // own terminal row stamps the ordinal onto the stored step.
            ordinal: Some(ord),
        };
        // Short-circuit once the per-run spend ceiling is reached: record the step
        // as a budget skip without dispatching the (paid) worker.
        let mut result = if self.over_budget() {
            budget_skip_result(&spec, self.budget_usd.get(), self.spent_usd.get())
        } else {
            let r = run_agent_step(self.driver, &spec);
            self.add_spent(&r);
            r
        };
        result.ordinal = Some(ord);
        self.steps.borrow_mut().push(result.clone());
        result
    }

    /// True once the cumulative spend has reached the declared ceiling (if any).
    fn over_budget(&self) -> bool {
        self.budget_usd
            .get()
            .is_some_and(|b| self.spent_usd.get() >= b)
    }

    /// Add a completed step's (real or estimated) cost to the running tally.
    fn add_spent(&self, result: &StepResult) {
        self.spent_usd
            .set(self.spent_usd.get() + step_cost_usd(result));
    }

    /// Run a barrier fan-out over already-extracted plain specs. Drives the
    /// EXISTING crate-level [`crate::parallel`] (scheduler-backed), then records
    /// every [`StepResult`] in input order and returns the results so the caller
    /// can build the script-visible return list (the structured dict when a spec
    /// had a schema and parsed, else its summary string). No Starlark value
    /// crosses a thread boundary — the specs were read off the heap before this.
    fn run_parallel(&self, specs: Vec<AgentStepSpec>) -> Vec<StepResult> {
        // Assign one ordinal per spec in INPUT order on the eval thread, BEFORE any
        // fan-out — pinning each spec's ordinal deterministically even though the
        // dispatch runs on threads (the threads never touch the counter).
        let ords: Vec<u64> = specs.iter().map(|_| self.next_ordinal()).collect();

        // Partition specs into replay HITS (reuse the cached result, no dispatch, no
        // spend) and MISSES (dispatch for real). Misses keep their input index so we
        // can merge results back in input order and stamp the right ordinal.
        // `cached` holds the UNMARKED result that flows back to the script (the prior
        // run's original summary); `replayed_idx` tags which input slots are replays
        // so the journaled copy can carry the [replayed] marker without corrupting
        // the script-visible value (a marker in the return can divert text-branching
        // control flow and desynchronize later ordinals).
        let mut cached: Vec<(usize, StepResult)> = Vec::new();
        let mut replayed_idx: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut to_dispatch: Vec<(usize, AgentStepSpec)> = Vec::new();
        for (i, spec) in specs.iter().enumerate() {
            if let Some(hit) = self.replay.get(&ords[i]) {
                let mut r = hit.clone();
                r.ordinal = Some(ords[i]);
                cached.push((i, r));
                replayed_idx.insert(i);
            } else {
                // Stamp the spec's ordinal so a real driver journaling its own
                // terminal row carries the ordinal onto the stored step.
                let mut spec = spec.clone();
                spec.ordinal = Some(ords[i]);
                to_dispatch.push((i, spec));
            }
        }

        // Dispatch ONLY the misses. Budget is enforced at barrier granularity over
        // the to-dispatch subset: if the ceiling is already reached, every miss is a
        // budget skip; otherwise the engine runs and every dispatched result is
        // tallied. An empty to-dispatch skips the engine entirely (no threads).
        let dispatch_specs: Vec<AgentStepSpec> =
            to_dispatch.iter().map(|(_, s)| s.clone()).collect();
        let dispatched: Vec<StepResult> = if dispatch_specs.is_empty() {
            Vec::new()
        } else if self.over_budget() {
            let (budget, spent) = (self.budget_usd.get(), self.spent_usd.get());
            dispatch_specs
                .iter()
                .map(|spec| budget_skip_result(spec, budget, spent))
                .collect()
        } else {
            let results = crate::parallel(self.driver, &dispatch_specs);
            for result in &results {
                self.add_spent(result);
            }
            results
        };

        // Merge cached + dispatched back into INPUT order, stamping each dispatched
        // result's ordinal from its original input index.
        let mut merged: Vec<Option<StepResult>> = vec![None; specs.len()];
        for (i, r) in cached {
            merged[i] = Some(r);
        }
        for (k, mut r) in dispatched.into_iter().enumerate() {
            let input_index = to_dispatch[k].0;
            r.ordinal = Some(ords[input_index]);
            merged[input_index] = Some(r);
        }
        let results: Vec<StepResult> = merged
            .into_iter()
            .map(|slot| slot.expect("every spec slot is filled (cached or dispatched)"))
            .collect();

        // Journal a MARKED copy for replayed slots (audit/[replayed] prefix), but
        // return the UNMARKED `results` to the caller (script-visible summary stays
        // the prior run's original text).
        let journaled: Vec<StepResult> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut j = r.clone();
                if replayed_idx.contains(&i) {
                    mark_replayed(&mut j);
                }
                j
            })
            .collect();
        self.steps.borrow_mut().extend(journaled);
        results
    }

    /// Run a streaming pipeline: every `item` flows through ALL `stages` in order,
    /// items overlapping at stage boundaries (no barrier), via the crate-level
    /// [`crate::pipeline`] engine. Each stage forward-injects the prior value (the
    /// item for stage 1, the prior stage's output for stage N) into its prompt
    /// template, runs the injected driver, and forwards its own output. Records
    /// EVERY produced [`StepResult`] (item × stage) into `ctx.steps` and returns the
    /// per-item LAST-stage result for the script-visible return list.
    ///
    /// No Starlark value crosses a thread boundary — `items`/`stages` were read off
    /// the heap into plain data before this. Budget is enforced at pipeline
    /// granularity: if the ceiling is already reached, every item-stage is a budget
    /// skip; otherwise the engine runs, then every produced step is tallied AFTER
    /// the threaded engine returns (on the eval thread — `Cell` is not thread-safe).
    fn run_pipeline(&self, items: Vec<String>, stages: Vec<StageTemplate>) -> Vec<StepResult> {
        if items.is_empty() || stages.is_empty() {
            return Vec::new();
        }

        // Advance the global ordinal counter item-major (items × stages) on the eval
        // thread so the ordinals of any LATER run_one/run_parallel leaves stay aligned
        // with the prior run. pipeline() leaves are EXCLUDED from replay in v1 (the
        // cache is never consulted for them and their StepResults keep `ordinal: None`)
        // because a partial-replay pipeline can diverge — a cached stage-N result
        // changes the value forward-injected into stage N+1. A follow-up can add full
        // pipeline replay; until then the counter advances so the scheme never desyncs.
        for _item in 0..items.len() {
            for _stage in 0..stages.len() {
                let _ = self.next_ordinal();
            }
        }

        if self.over_budget() {
            // Already over budget: short-circuit every item-stage to a budget skip,
            // recording one skipped step per item × stage without dispatching.
            let (budget, spent) = (self.budget_usd.get(), self.spent_usd.get());
            let mut last_per_item = Vec::with_capacity(items.len());
            for prior in &items {
                let mut last = None;
                for stage in &stages {
                    let spec = stage.spec_for(prior);
                    let skip = budget_skip_result(&spec, budget, spent);
                    self.steps.borrow_mut().push(skip.clone());
                    last = Some(skip);
                }
                last_per_item.push(last.expect("stages is non-empty"));
            }
            return last_per_item;
        }

        // Every produced step (item × stage) is recorded here by the Send + Sync
        // stage closures, then drained + tallied on the eval thread after the engine
        // returns. A plain `Mutex<Vec<..>>` is the only thread-safe sink the stage
        // closures may capture (no Starlark value, no `Cell`).
        let produced: std::sync::Mutex<Vec<StepResult>> = std::sync::Mutex::new(Vec::new());
        let driver = self.driver;

        // Build one PipelineStage closure per template. Each receives the prior
        // value on the incoming spec's `prompt` field, builds its concrete prompt
        // from its template, runs the driver, records the result, and forwards its
        // own output as the next stage's prior value (again carried on `prompt`).
        let pipeline_stages: Vec<crate::PipelineStage<'_>> = stages
            .iter()
            .map(|template| {
                let template = template.clone();
                let produced = &produced;
                let stage: crate::PipelineStage<'_> = Box::new(move |incoming: &AgentStepSpec| {
                    let spec = template.spec_for(&incoming.prompt);
                    let result = run_agent_step(driver, &spec);
                    produced.lock().expect("pipeline sink").push(result.clone());
                    // Carry this stage's output forward as the next stage's input.
                    let next = AgentStepSpec {
                        prompt: forward_value(&result),
                        ..spec
                    };
                    Some((next, result))
                });
                stage
            })
            .collect();

        // The crate engine seeds stage 1 from each item's `AgentStepSpec.prompt`, so
        // the placeholder is the raw item string; the first stage substitutes it.
        let seeds: Vec<AgentStepSpec> = items
            .iter()
            .map(|item| AgentStepSpec {
                phase: self.default_phase.clone(),
                label: String::new(),
                provider: String::new(),
                model: None,
                effort: None,
                service_tier: None,
                fallback_model: None,
                timeout_s: None,
                image: Vec::new(),
                add_dir: Vec::new(),
                expected_artifacts: Vec::new(),
                persist_changes: None,
                write_mode: None,
                owned_paths: Vec::new(),
                artifact_root: None,
                write_roots: Vec::new(),
                auto_apply_on_verdict: false,
                isolation: None,
                prompt: item.clone(),
                schema: None,
                schema_strict: false,
                writable: false,
                ordinal: None,
            })
            .collect();

        let last_per_item = crate::pipeline(seeds, pipeline_stages);

        // Tally every produced step on the eval thread (Cell is not thread-safe).
        let produced = produced.into_inner().expect("pipeline sink");
        for result in &produced {
            self.add_spent(result);
        }
        self.steps.borrow_mut().extend(produced);
        last_per_item
    }
}

/// Approximate USD cost of one completed step: the provider's billed figure when
/// it reports one (claude's `cost_usd`), else a token-based estimate via a coarse
/// per-provider price table (codex/gpt-class emits no dollar amount). Used only to
/// bound cumulative spend, never for billing.
fn step_cost_usd(result: &StepResult) -> f64 {
    let details = result.details.as_ref();
    if let Some(cost) = details
        .and_then(|d| d.get("cost_usd"))
        .and_then(|v| v.as_f64())
    {
        return cost;
    }
    let tokens = details.and_then(|d| d.get("tokens"));
    let field = |key: &str| {
        tokens
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let (in_rate, out_rate) = firm_core::provider_price_per_mtok(&result.provider);
    (field("input") as f64 / 1e6) * in_rate + (field("output") as f64 / 1e6) * out_rate
}

/// The `output_summary` prefix marking a replayed (cache-hit) step. Lets a human
/// (and the test suite) tell a reused leaf from a freshly dispatched one at a glance.
const REPLAYED_PREFIX: &str = "[replayed] ";

/// Mark a [`StepResult`] as REPLAYED from a prior run's cache: set
/// `details["replayed"] = true` (creating a `details` object if absent) and prefix
/// `output_summary` with [`REPLAYED_PREFIX`] (idempotently). Both markers round-trip
/// through the store — `details` via [`crate::step_result_json`]'s merge, the prefix
/// on the summary — so the resumed run has a complete, auditable record.
fn mark_replayed(result: &mut StepResult) {
    match result.details.as_mut() {
        Some(serde_json::Value::Object(map)) => {
            map.insert("replayed".to_string(), serde_json::Value::Bool(true));
        }
        _ => {
            result.details = Some(serde_json::json!({ "replayed": true }));
        }
    }
    if !result.output_summary.starts_with(REPLAYED_PREFIX) {
        result.output_summary = format!("{REPLAYED_PREFIX}{}", result.output_summary);
    }
}

/// A [`StepResult`] standing in for a step SKIPPED because the per-run budget
/// ceiling was already reached — recorded as a failed step (reason `budget`) so
/// the run finalizes degraded and the dashboard shows why it stopped spending.
fn budget_skip_result(spec: &AgentStepSpec, budget: Option<f64>, spent: f64) -> StepResult {
    let budget = budget.unwrap_or(0.0);
    StepResult {
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider: spec.provider.clone(),
        isolation: spec.isolation.clone(),
        ok: false,
        output_summary: format!(
            "skipped: per-run budget ${budget:.2} reached (spent ${spent:.2}) before this step ran"
        ),
        step_id: None,
        started_at: None,
        details: Some(serde_json::json!({
            "failure": {
                "failed": true,
                "reason": "budget",
                "detail": format!("per-run budget ${budget:.2} exceeded (spent ${spent:.2})"),
            }
        })),
        structured: None,
        ordinal: None,
    }
}

fn failure_value(result: &StepResult) -> Option<&serde_json::Value> {
    result
        .details
        .as_ref()
        .and_then(|details| details.get("failure"))
}

fn failure_reason(result: &StepResult) -> Option<String> {
    failure_value(result)
        .and_then(|failure| failure.get("reason"))
        .and_then(|reason| reason.as_str())
        .map(str::to_string)
        .or_else(|| (!result.ok).then(|| "failed".to_string()))
}

fn failure_detail(result: &StepResult) -> Option<String> {
    failure_value(result)
        .and_then(|failure| failure.get("detail"))
        .and_then(|detail| detail.as_str())
        .map(str::to_string)
        .or_else(|| (!result.ok).then(|| result.output_summary.clone()))
}

fn status_json(result: &StepResult) -> serde_json::Value {
    serde_json::json!({
        "ok": result.ok,
        "reason": failure_reason(result),
        "detail": failure_detail(result),
        "failure": failure_value(result).cloned(),
        "text": result.output_summary.as_str(),
        "structured": result.structured.clone(),
        "label": result.label.as_str(),
        "phase": result.phase.as_str(),
        "provider": result.provider.as_str(),
        "isolation": result.isolation.clone(),
        "ordinal": result.ordinal,
    })
}

fn result_value<'v>(
    heap: Heap<'v>,
    result: &StepResult,
    has_schema: bool,
    return_status: bool,
) -> Value<'v> {
    if return_status {
        return json_to_value(heap, &status_json(result));
    }
    if has_schema {
        match &result.structured {
            Some(structured) => json_to_value(heap, structured),
            None => Value::new_none(),
        }
    } else {
        heap.alloc(result.output_summary.as_str())
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

fn reject_direct_write_mode(write_mode: Option<&str>, context: &str) -> anyhow::Result<()> {
    if write_mode == Some(WRITE_MODE_DIRECT) {
        return Err(anyhow::anyhow!(
            "{context} does not support write_mode=\"direct\"; use a single serial agent() step for direct shared-repo edits"
        ));
    }
    Ok(())
}

/// Validate a leaf's persistence-related knobs at parse time (D3c), rejecting
/// nonsensical or silently-ignored combinations before the run starts:
/// * `auto_apply_on_verdict=True` or `persist_changes="patch"` on a `writable=False`
///   leaf — a read-only leaf produces no authorized diff to persist/apply, so
///   asking to capture or auto-apply one is a program error, not a silent no-op.
/// * an unknown `persist_changes` (only `"patch"`/`"discard"` are meaningful) or
///   `write_mode` (only `"direct"`, or absent) — arbitrary strings used to fall
///   back to defaults silently, hiding typos like `persist_changes="patchh"`.
fn validate_persistence_config(
    context: &str,
    label: Option<&str>,
    writable: bool,
    persist_changes: Option<&str>,
    write_mode: Option<&str>,
    auto_apply_on_verdict: bool,
) -> anyhow::Result<()> {
    let who = match label {
        Some(l) => format!("{context} leaf `{l}`"),
        None => context.to_string(),
    };
    if let Some(persist) = persist_changes {
        if persist != "patch" && persist != "discard" {
            return Err(anyhow::anyhow!(
                "{who}: unknown persist_changes={persist:?} (allowed: \"patch\", \"discard\")"
            ));
        }
    }
    if let Some(mode) = write_mode {
        if mode != WRITE_MODE_DIRECT {
            return Err(anyhow::anyhow!(
                "{who}: unknown write_mode={mode:?} (allowed: \"direct\", or omit)"
            ));
        }
    }
    if !writable {
        if auto_apply_on_verdict {
            return Err(anyhow::anyhow!(
                "{who}: auto_apply_on_verdict=True requires writable=True (a read-only leaf produces no patch to apply)"
            ));
        }
        if persist_changes == Some("patch") {
            return Err(anyhow::anyhow!(
                "{who}: persist_changes=\"patch\" requires writable=True (a read-only leaf produces no diff to persist)"
            ));
        }
    }
    Ok(())
}

/// Reject `schema_strict=True` on a spec that carries no `schema` (#192). Strict
/// mode only affects structured extraction, so requesting it without a schema is a
/// silent no-op that hides a program error — fail fast like the other config
/// hygiene checks. Same shape as [`validate_persistence_config`]'s `who` prefix.
fn validate_schema_strict(
    context: &str,
    label: Option<&str>,
    has_schema: bool,
    schema_strict: bool,
) -> anyhow::Result<()> {
    if schema_strict && !has_schema {
        let who = match label {
            Some(l) => format!("{context} leaf `{l}`"),
            None => context.to_string(),
        };
        return Err(anyhow::anyhow!(
            "{who}: schema_strict=True requires a schema (strict mode only \
             affects structured-output extraction)"
        ));
    }
    Ok(())
}

/// Read an optional bool field off a spec dict. Absent / Starlark `None` → false;
/// errors when present-but-not-a-bool.
fn dict_bool(dict: &DictRef<'_>, key: &str) -> anyhow::Result<bool> {
    match dict.get_str(key) {
        None => Ok(false),
        Some(value) if value.is_none() => Ok(false),
        Some(value) => value
            .unpack_bool()
            .ok_or_else(|| anyhow::anyhow!("parallel() spec field `{key}` must be a bool")),
    }
}

/// Read a positive Starlark integer as a wall-clock timeout in seconds.
fn value_positive_u64(value: Value<'_>, field: &str) -> anyhow::Result<u64> {
    let Some(seconds) = value.unpack_i32() else {
        return Err(anyhow::anyhow!("{field} must be a positive integer"));
    };
    if seconds <= 0 {
        return Err(anyhow::anyhow!("{field} must be greater than 0 seconds"));
    }
    Ok(seconds as u64)
}

/// Read an optional positive integer field off a spec dict. Absent / Starlark
/// `None` -> no wall-clock cap; errors when present but non-positive/non-int.
fn dict_positive_u64(dict: &DictRef<'_>, key: &str, context: &str) -> anyhow::Result<Option<u64>> {
    match dict.get_str(key) {
        None => Ok(None),
        Some(value) if value.is_none() => Ok(None),
        Some(value) => value_positive_u64(value, &format!("{context} field `{key}`")).map(Some),
    }
}

/// Read a Starlark list of strings. Used for `image`, whose host-function value
/// cannot be unpacked directly into `Vec<String>` on the Starlark version we use.
fn value_str_list(value: Value<'_>, field: &str) -> anyhow::Result<Vec<String>> {
    let list =
        ListRef::from_value(value).ok_or_else(|| anyhow::anyhow!("{field} must be a list"))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter() {
        let s = item
            .unpack_str()
            .ok_or_else(|| anyhow::anyhow!("{field} must be a list of strings"))?;
        out.push(s.to_string());
    }
    Ok(out)
}

/// Read an optional list-of-strings field off a spec dict. Absent / Starlark
/// `None` -> empty; errors when present-but-not-a-list or with non-string items.
fn dict_str_list(dict: &DictRef<'_>, key: &str) -> anyhow::Result<Vec<String>> {
    match dict.get_str(key) {
        None => Ok(Vec::new()),
        Some(value) if value.is_none() => Ok(Vec::new()),
        Some(value) => value_str_list(value, &format!("parallel() spec field `{key}`")),
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
#[derive(Debug, Clone)]
struct ParallelSpec {
    spec: AgentStepSpec,
    return_status: bool,
}

fn read_parallel_specs(
    ctx: &StarlarkCtx<'_>,
    specs: Value<'_>,
) -> anyhow::Result<Vec<ParallelSpec>> {
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
        let effort = dict_str(&dict, "effort")?;
        let service_tier = dict_str(&dict, "service_tier")?;
        let fallback_model = dict_str(&dict, "fallback_model")?;
        let timeout_s = dict_positive_u64(&dict, "timeout_s", "parallel() spec")?;
        let image = dict_str_list(&dict, "image")?;
        let add_dir = dict_str_list(&dict, "add_dir")?;
        let expected_artifacts = dict_str_list(&dict, "expected_artifacts")?;
        let persist_changes = dict_str(&dict, "persist_changes")?;
        let write_mode = dict_str(&dict, "write_mode")?;
        reject_direct_write_mode(write_mode.as_deref(), "parallel()")?;
        let owned_paths = dict_str_list(&dict, "owned_paths")?;
        let artifact_root = dict_str(&dict, "artifact_root")?;
        let write_roots = dict_str_list(&dict, "write_roots")?;
        let auto_apply_on_verdict = dict_bool(&dict, "auto_apply_on_verdict")?;
        let isolation = dict_str(&dict, "isolation")?;
        let schema = dict_schema(&dict, "schema")?;
        let schema_strict = dict_bool(&dict, "schema_strict")?;
        let writable = dict_bool(&dict, "writable")?;
        let return_status = dict_bool(&dict, "return_status")?;
        // D3c: reject nonsensical persistence config before the run starts.
        validate_persistence_config(
            "parallel()",
            label.as_deref(),
            writable,
            persist_changes.as_deref(),
            write_mode.as_deref(),
            auto_apply_on_verdict,
        )?;
        // #192: schema_strict without a schema is a no-op that hides a program
        // error (the field only bites in structured mode), so reject it up front.
        validate_schema_strict(
            "parallel()",
            label.as_deref(),
            schema.is_some(),
            schema_strict,
        )?;
        out.push(ParallelSpec {
            spec: AgentStepSpec {
                phase: ctx.phase_for(phase),
                label: label.unwrap_or_else(|| provider.clone()),
                provider,
                model,
                effort,
                service_tier,
                fallback_model,
                timeout_s,
                image,
                add_dir,
                expected_artifacts,
                persist_changes,
                write_mode,
                owned_paths,
                artifact_root,
                write_roots,
                auto_apply_on_verdict,
                isolation,
                prompt,
                schema,
                schema_strict,
                writable,
                ordinal: None,
            },
            return_status,
        });
    }
    Ok(out)
}

/// The placeholder token a `pipeline()` stage prompt template may contain. Stage 1
/// substitutes it with the (string-rendered) input item; stage N with the prior
/// stage's output (its parsed structured JSON serialized, else its summary text).
const PIPELINE_INPUT_PLACEHOLDER: &str = "{input}";

/// A PLAIN-data template for one `pipeline()` stage. Read off the Starlark heap on
/// the eval thread BEFORE any threading, so the per-item stage closures (which are
/// `Send + Sync`) capture ONLY this — no Starlark value crosses a thread boundary.
/// The `prompt_template` may contain [`PIPELINE_INPUT_PLACEHOLDER`], replaced with
/// the forward-injected prior value when the stage builds its concrete prompt.
#[derive(Debug, Clone)]
struct StageTemplate {
    prompt_template: String,
    provider: String,
    /// `None` until resolved against `ctx.phase_for(..)` when building the spec.
    label: Option<String>,
    phase: String,
    model: Option<String>,
    effort: Option<String>,
    service_tier: Option<String>,
    fallback_model: Option<String>,
    timeout_s: Option<u64>,
    image: Vec<String>,
    add_dir: Vec<String>,
    expected_artifacts: Vec<String>,
    persist_changes: Option<String>,
    write_mode: Option<String>,
    owned_paths: Vec<String>,
    artifact_root: Option<String>,
    write_roots: Vec<String>,
    auto_apply_on_verdict: bool,
    isolation: Option<String>,
    schema: Option<serde_json::Value>,
    schema_strict: bool,
    writable: bool,
    return_status: bool,
}

impl StageTemplate {
    /// Build the concrete [`AgentStepSpec`] this stage runs for one item, forward-
    /// injecting `prior` (the item for stage 1, the prior stage's output for stage
    /// N) wherever the template carries [`PIPELINE_INPUT_PLACEHOLDER`].
    fn spec_for(&self, prior: &str) -> AgentStepSpec {
        let prompt = self
            .prompt_template
            .replace(PIPELINE_INPUT_PLACEHOLDER, prior);
        AgentStepSpec {
            phase: self.phase.clone(),
            label: self.label.clone().unwrap_or_else(|| self.provider.clone()),
            provider: self.provider.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            service_tier: self.service_tier.clone(),
            fallback_model: self.fallback_model.clone(),
            timeout_s: self.timeout_s,
            image: self.image.clone(),
            add_dir: self.add_dir.clone(),
            expected_artifacts: self.expected_artifacts.clone(),
            persist_changes: self.persist_changes.clone(),
            write_mode: self.write_mode.clone(),
            owned_paths: self.owned_paths.clone(),
            artifact_root: self.artifact_root.clone(),
            write_roots: self.write_roots.clone(),
            auto_apply_on_verdict: self.auto_apply_on_verdict,
            isolation: self.isolation.clone(),
            prompt,
            schema: self.schema.clone(),
            schema_strict: self.schema_strict,
            writable: self.writable,
            ordinal: None,
        }
    }
}

/// The value a stage forwards to the next stage: a step's parsed structured JSON
/// serialized to a compact string when present, else its plain summary text.
fn forward_value(result: &StepResult) -> String {
    match &result.structured {
        Some(structured) => {
            serde_json::to_string(structured).unwrap_or_else(|_| result.output_summary.clone())
        }
        None => result.output_summary.clone(),
    }
}

/// Read a `pipeline()` `items` list (each element a string OR a dict) into PLAIN
/// strings to forward-inject into stage 1. A string item is used verbatim; a dict
/// (or any non-string) item is serialized to compact JSON. Happens on the eval
/// thread BEFORE any threading, so no Starlark value crosses a thread boundary.
fn read_pipeline_items(items: Value<'_>) -> anyhow::Result<Vec<String>> {
    let list = ListRef::from_value(items)
        .ok_or_else(|| anyhow::anyhow!("pipeline() expects a list of items (strings or dicts)"))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter() {
        if let Some(s) = item.unpack_str() {
            out.push(s.to_string());
        } else {
            let json = value_to_json(item);
            out.push(serde_json::to_string(&json).unwrap_or_default());
        }
    }
    Ok(out)
}

/// Read a `pipeline()` `stages` list (a Starlark list of stage dicts) into PLAIN
/// [`StageTemplate`]s, resolving phase defaults via `ctx`. Happens on the eval
/// thread BEFORE any threading, mirroring [`read_parallel_specs`].
fn read_pipeline_stages(
    ctx: &StarlarkCtx<'_>,
    stages: Value<'_>,
) -> anyhow::Result<Vec<StageTemplate>> {
    let list = ListRef::from_value(stages)
        .ok_or_else(|| anyhow::anyhow!("pipeline() expects a list of stage dicts"))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list.iter() {
        let dict = DictRef::from_value(item)
            .ok_or_else(|| anyhow::anyhow!("pipeline() stage must be a dict"))?;
        let prompt_template = dict_str(&dict, "prompt")?
            .ok_or_else(|| anyhow::anyhow!("pipeline() stage requires a `prompt` string"))?;
        let provider = dict_str(&dict, "provider")?.unwrap_or_else(|| "codex".to_string());
        let label = dict_str(&dict, "label")?;
        let phase = dict_str(&dict, "phase")?;
        let model = dict_str(&dict, "model")?;
        let effort = dict_str(&dict, "effort")?;
        let service_tier = dict_str(&dict, "service_tier")?;
        let fallback_model = dict_str(&dict, "fallback_model")?;
        let timeout_s = dict_positive_u64(&dict, "timeout_s", "pipeline() stage")?;
        let image = dict_str_list(&dict, "image")?;
        let add_dir = dict_str_list(&dict, "add_dir")?;
        let expected_artifacts = dict_str_list(&dict, "expected_artifacts")?;
        let persist_changes = dict_str(&dict, "persist_changes")?;
        let write_mode = dict_str(&dict, "write_mode")?;
        reject_direct_write_mode(write_mode.as_deref(), "pipeline()")?;
        let owned_paths = dict_str_list(&dict, "owned_paths")?;
        let artifact_root = dict_str(&dict, "artifact_root")?;
        let write_roots = dict_str_list(&dict, "write_roots")?;
        let auto_apply_on_verdict = dict_bool(&dict, "auto_apply_on_verdict")?;
        let isolation = dict_str(&dict, "isolation")?;
        let schema = dict_schema(&dict, "schema")?;
        let schema_strict = dict_bool(&dict, "schema_strict")?;
        let writable = dict_bool(&dict, "writable")?;
        let return_status = dict_bool(&dict, "return_status")?;
        // D3c: reject nonsensical persistence config before the run starts.
        validate_persistence_config(
            "pipeline()",
            label.as_deref(),
            writable,
            persist_changes.as_deref(),
            write_mode.as_deref(),
            auto_apply_on_verdict,
        )?;
        // #192: schema_strict only bites in structured mode.
        validate_schema_strict(
            "pipeline()",
            label.as_deref(),
            schema.is_some(),
            schema_strict,
        )?;
        out.push(StageTemplate {
            prompt_template,
            provider,
            label,
            phase: ctx.phase_for(phase),
            model,
            effort,
            service_tier,
            fallback_model,
            timeout_s,
            image,
            add_dir,
            expected_artifacts,
            persist_changes,
            write_mode,
            owned_paths,
            artifact_root,
            write_roots,
            auto_apply_on_verdict,
            isolation,
            schema,
            schema_strict,
            writable,
            return_status,
        });
    }
    Ok(out)
}

/// The workflow host functions exposed to the script.
// `agent()` exposes a broad host API surface, and its expansion trips clippy's
// arg-count lint.
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
        #[starlark(require = named)] budget_usd: Option<Value<'v>>,
        #[starlark(require = named)] success_criterion: Option<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let ctx = ctx_of(eval);
        *ctx.meta.borrow_mut() = Some((name, design_intent));
        if let Some(criterion) = success_criterion.filter(|s| !s.trim().is_empty()) {
            *ctx.success_criterion.borrow_mut() = Some(criterion);
        }
        // The program may declare a spend ceiling; the operator's CLI
        // `--max-budget-usd` (already in `budget_usd`) wins by taking the smaller.
        // Starlark has no f64 UnpackValue, so accept any number Value (int or
        // float) and read it back through the JSON bridge.
        let declared = budget_usd
            .filter(|v| !v.is_none())
            .and_then(|v| value_to_json(v).as_f64())
            .filter(|b| *b > 0.0);
        if let Some(declared) = declared {
            let effective = match ctx.budget_usd.get() {
                Some(cli) => cli.min(declared),
                None => declared,
            };
            ctx.budget_usd.set(Some(effective));
        }
        Ok(NoneType)
    }

    /// Run one ephemeral provider worker synchronously.
    ///
    /// In TEXT mode (no `schema`) it returns the worker's output text (so the
    /// script can chain it). In STRUCTURED mode (`schema={...}`) it forces the
    /// worker to reply with a single JSON object carrying the schema's top-level
    /// keys and returns the parsed dict (e.g. `res["ok"]`); if the worker never
    /// produced valid JSON it returns `None` so the script can check/skip. With
    /// `return_status=True`, return a dict with `ok`, `reason`, `detail`, raw
    /// `failure`, `text`, and `structured` regardless of text/schema mode.
    fn agent<'v>(
        #[starlark(require = pos)] prompt: String,
        #[starlark(require = named, default = "codex".to_string())] provider: String,
        #[starlark(require = named)] label: Option<String>,
        #[starlark(require = named)] phase: Option<String>,
        #[starlark(require = named)] model: Option<String>,
        #[starlark(require = named)] effort: Option<String>,
        #[starlark(require = named)] service_tier: Option<String>,
        #[starlark(require = named)] fallback_model: Option<String>,
        #[starlark(require = named)] timeout_s: Option<Value<'v>>,
        #[starlark(require = named)] image: Option<Value<'v>>,
        #[starlark(require = named)] add_dir: Option<Value<'v>>,
        #[starlark(require = named)] expected_artifacts: Option<Value<'v>>,
        #[starlark(require = named)] persist_changes: Option<String>,
        #[starlark(require = named)] write_mode: Option<String>,
        #[starlark(require = named)] owned_paths: Option<Value<'v>>,
        #[starlark(require = named)] artifact_root: Option<String>,
        #[starlark(require = named)] write_roots: Option<Value<'v>>,
        #[starlark(require = named, default = false)] auto_apply_on_verdict: bool,
        #[starlark(require = named)] isolation: Option<String>,
        #[starlark(require = named)] schema: Option<Value<'v>>,
        #[starlark(require = named, default = false)] schema_strict: bool,
        #[starlark(require = named, default = false)] writable: bool,
        #[starlark(require = named, default = false)] return_status: bool,
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
        let image = match image {
            Some(value) if !value.is_none() => value_str_list(value, "agent() `image`")?,
            _ => Vec::new(),
        };
        let add_dir = match add_dir {
            Some(value) if !value.is_none() => value_str_list(value, "agent() `add_dir`")?,
            _ => Vec::new(),
        };
        let expected_artifacts = match expected_artifacts {
            Some(value) if !value.is_none() => {
                value_str_list(value, "agent() `expected_artifacts`")?
            }
            _ => Vec::new(),
        };
        let owned_paths = match owned_paths {
            Some(value) if !value.is_none() => value_str_list(value, "agent() `owned_paths`")?,
            _ => Vec::new(),
        };
        let write_roots = match write_roots {
            Some(value) if !value.is_none() => value_str_list(value, "agent() `write_roots`")?,
            _ => Vec::new(),
        };
        let timeout_s = match timeout_s {
            Some(value) if !value.is_none() => {
                Some(value_positive_u64(value, "agent() `timeout_s`")?)
            }
            _ => None,
        };
        // D3c: reject nonsensical persistence config before the run starts.
        validate_persistence_config(
            "agent()",
            label.as_deref(),
            writable,
            persist_changes.as_deref(),
            write_mode.as_deref(),
            auto_apply_on_verdict,
        )?;
        // #192: schema_strict only bites in structured mode.
        validate_schema_strict(
            "agent()",
            label.as_deref(),
            schema_json.is_some(),
            schema_strict,
        )?;
        let has_schema = schema_json.is_some();
        let result = ctx_of(eval).run_one(
            prompt,
            provider,
            label,
            phase,
            model,
            effort,
            service_tier,
            fallback_model,
            timeout_s,
            image,
            add_dir,
            expected_artifacts,
            persist_changes,
            write_mode,
            owned_paths,
            artifact_root,
            write_roots,
            auto_apply_on_verdict,
            isolation,
            schema_json,
            schema_strict,
            writable,
        );
        Ok(result_value(
            eval.heap(),
            &result,
            has_schema,
            return_status,
        ))
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
        let return_shapes: Vec<_> = extracted
            .iter()
            .map(|item| (item.spec.schema.is_some(), item.return_status))
            .collect();
        let results = ctx.run_parallel(extracted.into_iter().map(|item| item.spec).collect());
        let heap = eval.heap();
        let values: Vec<Value<'v>> = results
            .iter()
            .zip(return_shapes.iter())
            .map(|(result, (has_schema, return_status))| {
                result_value(heap, result, *has_schema, *return_status)
            })
            .collect();
        Ok(heap.alloc(values))
    }

    /// Run a STREAMING pipeline: every item in `items` flows through ALL `stages`
    /// in order, with NO barrier between stages (item A may be in stage 3 while item
    /// B is still in stage 1). Returns a list in input order, one element per item:
    /// the LAST stage's parsed structured dict (when that stage carried a `schema`
    /// and the worker produced valid JSON) else its output-summary string.
    ///
    /// `items` is a list whose elements are strings OR dicts (a dict is serialized
    /// to compact JSON). `stages` is a list of stage dicts, each with a required
    /// `prompt` TEMPLATE and optional `provider` (default "codex"), `label`,
    /// `phase`, `model`, `schema`, and `writable`. Each stage's `prompt` template
    /// may contain the literal `{input}` placeholder: stage 1 substitutes it with
    /// the item; stage N with stage N-1's output (its parsed structured JSON
    /// serialized, else its summary text) — the forward-injection that lets a stage
    /// build on its predecessor.
    fn pipeline<'v>(
        #[starlark(require = pos)] items: Value<'v>,
        // Accept BOTH the canonical list form `pipeline(items, [s1, s2])` (what the
        // skill examples use) AND the bare-positional form `pipeline(items, s1, s2,
        // ...)` — the latter used to fail with a cryptic "Wrong number of positional
        // arguments" before the body even ran (issue #139 item 4). Collecting the
        // stages as varargs lets us normalize either shape into the stage list.
        #[starlark(args)] stages: UnpackTuple<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        // A single list argument IS the stage list; multiple positional stages get
        // wrapped into one. (A lone dict — `pipeline(items, {..})` — also wraps.)
        let stage_values = stages.items;
        let stages_value: Value<'v> =
            if stage_values.len() == 1 && ListRef::from_value(stage_values[0]).is_some() {
                stage_values[0]
            } else {
                eval.heap().alloc(stage_values)
            };
        let ctx = ctx_of(eval);
        // Extract BOTH items and stage templates into PLAIN Rust before any
        // threading — no Starlark value may cross the streaming engine's threads.
        let items = read_pipeline_items(items)?;
        let stages = read_pipeline_stages(ctx, stages_value)?;
        let return_shape = stages
            .last()
            .map(|stage| (stage.schema.is_some(), stage.return_status))
            .unwrap_or((false, false));
        let results = ctx.run_pipeline(items, stages);
        let heap = eval.heap();
        let values: Vec<Value<'v>> = results
            .iter()
            .map(|result| result_value(heap, result, return_shape.0, return_shape.1))
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

    /// Declare the run's typed verdict: whether it met its intent (`ok`) and a
    /// short `reason`. Makes the run status intent-relative — `ok=false` finalizes
    /// the run as Failed even if every worker step ran, so "workers ran" no longer
    /// means "intent satisfied". The last call wins.
    fn verdict<'v>(
        #[starlark(require = pos)] ok: bool,
        // Accept `reason` either positionally (`verdict(ok, "why")`) or by keyword
        // (`verdict(ok, reason="why")`) — the bare positional form is the natural
        // thing to write and used to error (issue #139 item 6).
        #[starlark(default = String::new())] reason: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        *ctx_of(eval).verdict.borrow_mut() = Some((ok, reason));
        Ok(NoneType)
    }

    /// Declare the run's RESULT — the first-class answer the calling agent reads
    /// back. The `value` (a string, dict, or any Starlark value) is persisted
    /// verbatim under `final_output.result`, so a caller reads ONE unambiguous field
    /// instead of guessing which step's `output_summary` holds the answer. Unlike a
    /// step summary it is NOT capped, so a structured `value` carries full fidelity
    /// (a free-text `agent()` return was already capped at the worker boundary — pass
    /// a `schema=`'d dict for a large answer). The last call wins.
    fn output<'v>(
        #[starlark(require = pos)] value: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        *ctx_of(eval).output.borrow_mut() = Some(value_to_json(value));
        Ok(NoneType)
    }

    /// Ask the CLI post-processor to apply the patch produced by a prior step.
    ///
    /// `label` matches the step label. The actual git operation is performed
    /// after the workflow has journaled its patches, so the operation is guarded
    /// and auditable.
    fn apply_patch<'v>(
        #[starlark(require = pos)] label: String,
        #[starlark(default = String::new())] reason: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        ctx_of(eval)
            .patch_actions
            .borrow_mut()
            .push(serde_json::json!({
                "action": "apply",
                "label": label,
                "reason": reason,
            }));
        Ok(NoneType)
    }

    /// Ask the CLI post-processor to reject the patch produced by a prior step.
    fn reject_patch<'v>(
        #[starlark(require = pos)] label: String,
        #[starlark(default = String::new())] reason: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        ctx_of(eval)
            .patch_actions
            .borrow_mut()
            .push(serde_json::json!({
                "action": "reject",
                "label": label,
                "reason": reason,
            }));
        Ok(NoneType)
    }

    /// Declare workflow artifacts that should be validated into a durable manifest.
    fn artifact_manifest<'v>(
        #[starlark(require = pos)] paths: Value<'v>,
        #[starlark(require = named)] label: Option<String>,
        #[starlark(require = named)] artifact_root: Option<String>,
        #[starlark(require = named)] write_roots: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let paths = value_str_list(paths, "artifact_manifest() `paths`")?;
        let write_roots = match write_roots {
            Some(value) if !value.is_none() => {
                value_str_list(value, "artifact_manifest() `write_roots`")?
            }
            _ => Vec::new(),
        };
        ctx_of(eval)
            .artifact_manifest_requests
            .borrow_mut()
            .push(serde_json::json!({
                "paths": paths,
                "label": label,
                "artifact_root": artifact_root,
                "write_roots": write_roots,
            }));
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
mod runner;
use runner::{json_to_value, value_to_json};
pub use runner::{run_starlark, run_starlark_with_budget};

#[cfg(test)]
#[path = "starlark_front_tests/mod.rs"]
mod tests;
