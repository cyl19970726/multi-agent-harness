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
//! * `pipeline(items, stages)` — a STREAMING fan-out: every item flows through ALL
//!   `stages` in order with NO barrier between stages (item A may be in stage 3
//!   while item B is still in stage 1). Returns a list in input order, one element
//!   per item: the LAST stage's parsed structured dict (if it had a `schema` and
//!   parsed) else its output-summary string. `items` is a list of strings OR dicts;
//!   `stages` is a list of stage dicts (`prompt` TEMPLATE + optional `provider`,
//!   `label`, `phase`, `model`, `schema`, `writable`). Each stage `prompt` may
//!   contain the literal `{input}` placeholder, FORWARD-INJECTED with the item
//!   (stage 1) or the prior stage's output (stage N) — e.g.
//!   `pipeline(args["files"], [{"prompt": "scan {input}"}, {"prompt": "fix per {input}"}])`.
//! * `phase(name)` — set the default phase for subsequent steps.
//! * `log(message)` — emit a progress line.
//! * `args` — the run's JSON parameterization, injected as a module global value.

use std::cell::{Cell, RefCell};

use starlark::any::ProvidesStaticType;
use starlark::environment::{GlobalsBuilder, LibraryExtension, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};

use harness_core::WorkflowRunStatus;

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
        writable: bool,
    ) -> StepResult {
        let spec = AgentStepSpec {
            phase: self.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
            schema,
            writable,
        };
        // Short-circuit once the per-run spend ceiling is reached: record the step
        // as a budget skip without dispatching the (paid) worker.
        let result = if self.over_budget() {
            budget_skip_result(&spec, self.budget_usd.get(), self.spent_usd.get())
        } else {
            let r = run_agent_step(self.driver, &spec);
            self.add_spent(&r);
            r
        };
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
        // Budget is enforced at barrier granularity: if the ceiling is already
        // reached, skip the whole batch; otherwise run it and tally every result.
        let results = if self.over_budget() {
            let (budget, spent) = (self.budget_usd.get(), self.spent_usd.get());
            specs
                .iter()
                .map(|spec| budget_skip_result(spec, budget, spent))
                .collect()
        } else {
            let results = crate::parallel(self.driver, &specs);
            for result in &results {
                self.add_spent(result);
            }
            results
        };
        self.steps.borrow_mut().extend(results.clone());
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
                isolation: None,
                prompt: item.clone(),
                schema: None,
                writable: false,
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
    let (in_rate, out_rate) = price_per_mtok(&result.provider);
    (field("input") as f64 / 1e6) * in_rate + (field("output") as f64 / 1e6) * out_rate
}

/// Rough public list price ($ per 1M tokens) `(input, output)` per provider — an
/// ESTIMATE used only to bound spend when the provider reports no dollar cost.
fn price_per_mtok(provider: &str) -> (f64, f64) {
    match provider {
        "claude" => (3.0, 15.0),
        _ => (1.25, 10.0), // codex / gpt-5-class default
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
        provider_session_id: None,
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
        let writable = dict_bool(&dict, "writable")?;
        out.push(AgentStepSpec {
            phase: ctx.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
            schema,
            writable,
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
    isolation: Option<String>,
    schema: Option<serde_json::Value>,
    writable: bool,
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
            isolation: self.isolation.clone(),
            prompt,
            schema: self.schema.clone(),
            writable: self.writable,
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
        let isolation = dict_str(&dict, "isolation")?;
        let schema = dict_schema(&dict, "schema")?;
        let writable = dict_bool(&dict, "writable")?;
        out.push(StageTemplate {
            prompt_template,
            provider,
            label,
            phase: ctx.phase_for(phase),
            model,
            isolation,
            schema,
            writable,
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
    /// produced valid JSON it returns `None` so the script can check/skip.
    fn agent<'v>(
        #[starlark(require = pos)] prompt: String,
        #[starlark(require = named, default = "codex".to_string())] provider: String,
        #[starlark(require = named)] label: Option<String>,
        #[starlark(require = named)] phase: Option<String>,
        #[starlark(require = named)] model: Option<String>,
        #[starlark(require = named)] isolation: Option<String>,
        #[starlark(require = named)] schema: Option<Value<'v>>,
        #[starlark(require = named, default = false)] writable: bool,
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
            writable,
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
        #[starlark(require = pos)] stages: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let ctx = ctx_of(eval);
        // Extract BOTH items and stage templates into PLAIN Rust before any
        // threading — no Starlark value may cross the streaming engine's threads.
        let items = read_pipeline_items(items)?;
        let stages = read_pipeline_stages(ctx, stages)?;
        let results = ctx.run_pipeline(items, stages);
        let heap = eval.heap();
        let values: Vec<Value<'v>> = results
            .iter()
            .map(|result| match &result.structured {
                // The last stage carried a schema that parsed → hand back the dict.
                Some(structured) => json_to_value(heap, structured),
                // Schema-less (or unparsed) → the last stage's summary string.
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

    /// Declare the run's typed verdict: whether it met its intent (`ok`) and a
    /// short `reason`. Makes the run status intent-relative — `ok=false` finalizes
    /// the run as Failed even if every worker step ran, so "workers ran" no longer
    /// means "intent satisfied". The last call wins.
    fn verdict<'v>(
        #[starlark(require = pos)] ok: bool,
        #[starlark(require = named, default = String::new())] reason: String,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        *ctx_of(eval).verdict.borrow_mut() = Some((ok, reason));
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
    run_starlark_with_budget(script, name, args, driver, None)
}

/// Like [`run_starlark`] but with an optional per-run spend ceiling in USD (the
/// CLI `--max-budget-usd`, also lowerable by a `workflow(budget_usd=…)` header).
/// Once cumulative step cost reaches it, further `agent()` / `parallel()` calls
/// are short-circuited into failed `budget` steps instead of dispatching workers.
pub fn run_starlark_with_budget(
    script: &str,
    name: &str,
    args: Option<&serde_json::Value>,
    driver: &AgentStepFn<'_>,
    budget_usd: Option<f64>,
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
        budget_usd: Cell::new(budget_usd),
        spent_usd: Cell::new(0.0),
        verdict: RefCell::new(None),
        success_criterion: RefCell::new(None),
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
    let mut outcome = outcome_from_steps(name, steps, spawned_before);
    let success_criterion = ctx.success_criterion.into_inner();
    // A declared verdict makes the run status INTENT-RELATIVE: mechanical
    // step-success becomes necessary-but-not-sufficient, so a run whose workers all
    // ran but whose self-check failed reports Failed (not a misleading Completed).
    if let Some((ok, reason)) = ctx.verdict.into_inner() {
        outcome.status = if ok {
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
            if ok { "met" } else { "NOT met" }
        );
    }
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

    /// A driver that "spends" a fixed USD per call (via `details.cost_usd`) and
    /// counts dispatches, for budget-ceiling tests.
    fn spending_driver(
        calls: &std::sync::atomic::AtomicUsize,
        cost: f64,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + '_ {
        move |spec: &AgentStepSpec| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some("s".into()),
                output_summary: "ok".into(),
                step_id: None,
                started_at: None,
                details: Some(serde_json::json!({ "cost_usd": cost })),
                structured: None,
            }
        }
    }

    #[test]
    fn budget_ceiling_short_circuits_further_steps() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let driver = spending_driver(&calls, 0.6);
        // CLI budget $1.00: step1 (spent 0 -> runs, 0.6), step2 (0.6 -> runs, 1.2),
        // step3 (1.2 >= 1.0 -> SKIPPED). The driver dispatches exactly twice.
        let script = "\nagent(\"a\")\nagent(\"b\")\nagent(\"c\")\n";
        let outcome = run_starlark_with_budget(
            &format!("{HEADER}{script}"),
            "demo",
            None,
            &driver,
            Some(1.0),
        )
        .expect("run ok")
        .outcome;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the third step must be skipped once the budget is reached"
        );
        assert_eq!(outcome.steps.len(), 3);
        assert!(outcome.steps[0].ok && outcome.steps[1].ok);
        assert!(!outcome.steps[2].ok, "third step is a budget skip");
        assert!(outcome.steps[2].output_summary.contains("budget"));
    }

    #[test]
    fn workflow_header_budget_lowers_the_ceiling() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let driver = spending_driver(&calls, 0.6);
        // Header budget_usd=0.5, no CLI budget: step1 runs (spends 0.6); step2 sees
        // 0.6 >= 0.5 and is skipped.
        let script = "workflow(\"demo\", \"declare a tight budget so the run stops early\", budget_usd = 0.5)\nagent(\"a\")\nagent(\"b\")\n";
        let outcome = run_starlark_with_budget(script, "demo", None, &driver, None)
            .expect("run ok")
            .outcome;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "only the first step runs");
        assert!(!outcome.steps[1].ok);
    }

    #[test]
    fn verdict_false_makes_status_failed_even_when_steps_ran() {
        // A successful step + verdict(False) -> the run is Failed: "workers ran"
        // is no longer "intent satisfied".
        let seen = Mutex::new(Vec::new());
        let script =
            "\nagent(\"do the work\")\nverdict(False, reason = \"a regression slipped through\")\n";
        let run = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
        };
        assert_eq!(run.outcome.status, WorkflowRunStatus::Failed);
        assert!(run.outcome.summary.contains("NOT met"));
        assert!(run.outcome.summary.contains("regression"));
        // The step itself still ran fine — the verdict overrides the status only.
        assert_eq!(run.outcome.steps.len(), 1);
        assert!(run.outcome.steps[0].ok);
    }

    #[test]
    fn verdict_true_keeps_completed_and_surfaces_header_criterion() {
        let seen = Mutex::new(Vec::new());
        let script = "workflow(\"demo\", \"run and self-assess against a declared bar\", success_criterion = \"all checks green\")\nagent(\"x\")\nverdict(True, reason = \"all green\")\n";
        let run = {
            let driver = recording_driver(&seen);
            run_starlark(script, "demo", None, &driver).expect("run ok")
        };
        assert_eq!(run.outcome.status, WorkflowRunStatus::Completed);
        assert!(run.outcome.summary.contains("met"));
        assert!(run.outcome.summary.contains("all checks green"));
        assert_eq!(
            run.meta.success_criterion.as_deref(),
            Some("all checks green")
        );
    }

    /// A driver that records each spec's (label, writable) so writable-flow tests
    /// can assert the kwarg reached the plain spec.
    fn writable_recording_driver(
        seen: &Mutex<Vec<(String, bool)>>,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + '_ {
        move |spec: &AgentStepSpec| {
            seen.lock()
                .unwrap()
                .push((spec.label.clone(), spec.writable));
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some("s".into()),
                output_summary: "ok".into(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
            }
        }
    }

    #[test]
    fn agent_writable_kwarg_flows_onto_the_spec_default_false() {
        let seen = Mutex::new(Vec::new());
        let script =
            "\nagent(\"read it\", label = \"reader\")\nagent(\"fix it\", label = \"fixer\", writable = True)\n";
        {
            let driver = writable_recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
        }
        let seen = seen.into_inner().unwrap();
        assert_eq!(
            seen,
            vec![("reader".to_string(), false), ("fixer".to_string(), true)]
        );
    }

    #[test]
    fn parallel_writable_spec_field_flows() {
        let seen = Mutex::new(Vec::new());
        let script =
            "\nparallel([{\"prompt\": \"a\", \"label\": \"x\"}, {\"prompt\": \"b\", \"label\": \"y\", \"writable\": True}])\n";
        {
            let driver = writable_recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
        }
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(
            seen,
            vec![("x".to_string(), false), ("y".to_string(), true)]
        );
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

    #[test]
    fn pipeline_flows_every_item_through_all_stages_in_order() {
        // 2 items x 2 stages: each item must visit BOTH stages, and stage 2's prompt
        // must carry the forward-injected output of stage 1 (proving the no-barrier
        // streaming engine threads the prior value into the next stage's template).
        let seen = Mutex::new(Vec::new());
        let script = r#"
results = pipeline(
    ["alpha", "beta"],
    [
        {"prompt": "scan {input}", "label": "s1"},
        {"prompt": "fix per {input}", "label": "s2"},
    ],
)
log("alpha last: " + results[0])
log("beta last: " + results[1])
"#;
        let outcome = {
            let driver = recording_driver(&seen);
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        let seen = seen.into_inner().unwrap();
        // 2 items x 2 stages = 4 driver dispatches.
        assert_eq!(seen.len(), 4);
        // Every (label, prompt) pair that must have run, regardless of interleaving.
        // recording_driver returns "ok: <prompt>", so stage 2 sees stage 1's output.
        let pairs: std::collections::HashSet<(String, String)> = seen.into_iter().collect();
        assert!(pairs.contains(&("s1".to_string(), "scan alpha".to_string())));
        assert!(pairs.contains(&("s1".to_string(), "scan beta".to_string())));
        assert!(pairs.contains(&("s2".to_string(), "fix per ok: scan alpha".to_string())));
        assert!(pairs.contains(&("s2".to_string(), "fix per ok: scan beta".to_string())));

        // Every produced step (item x stage) is journaled.
        assert_eq!(outcome.steps.len(), 4);
        assert!(outcome.steps.iter().all(|s| s.ok));
        // The script-visible return is the LAST stage's summary, in input order.
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
    }

    #[test]
    fn pipeline_forward_injects_structured_output_into_next_stage() {
        // Stage 1 carries a schema; its parsed structured JSON (serialized) must be
        // forward-injected into stage 2's `{input}` placeholder.
        let schemas = Mutex::new(Vec::new());
        let prompts = Mutex::new(Vec::new());
        let script = r#"
pipeline(
    ["item-x"],
    [
        {"prompt": "classify {input}", "schema": {"verdict": ""}},
        {"prompt": "act on {input}", "label": "s2"},
    ],
)
"#;
        let outcome = {
            // Wrap structured_driver so we also capture stage 2's concrete prompt.
            let inner = structured_driver(&schemas);
            let driver = |spec: &AgentStepSpec| {
                if spec.label == "s2" {
                    prompts.lock().unwrap().push(spec.prompt.clone());
                }
                inner(spec)
            };
            run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
                .expect("run ok")
                .outcome
        };
        assert_eq!(outcome.steps.len(), 2);
        // Stage 1 produced a structured dict.
        assert!(outcome.steps.iter().any(|s| s.structured.is_some()));
        // Stage 2's prompt carries stage 1's serialized structured JSON.
        let prompts = prompts.into_inner().unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(
            prompts[0].contains("verdict"),
            "stage 2 prompt must carry stage 1's structured JSON, got: {}",
            prompts[0]
        );
    }
}
