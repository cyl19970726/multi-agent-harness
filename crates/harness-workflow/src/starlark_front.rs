//! Starlark workflow front-end.
//!
//! A THIRD authoring front-end (alongside the Rust built-ins like [`super::investigate`]
//! and the JSON-IR [`super::dispatch_spec`]) that lets an agent author a real *program*
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
//! * `agent(prompt, provider="codex", label=None, phase=None, model=None, isolation=None)`
//!   — run ONE ephemeral worker synchronously; returns its output text (so the script can
//!   chain, e.g. `scan = agent(...)` then `scan.splitlines()`).
//! * `parallel(specs)` — a barrier fan-out: run every spec concurrently and block until
//!   ALL finish, returning the list of their output-summary strings in input order.
//!   `specs` is a list of dicts, each with a required `prompt` and optional `provider`
//!   (default "codex"), `label`, `phase`, `model`, and `isolation` — e.g.
//!   `parallel([{"prompt": "fix " + x} for x in args["items"]])`.
//! * `phase(name)` — set the default phase for subsequent steps.
//! * `log(message)` — emit a progress line.
//! * `args` — the run's JSON parameterization, injected as a module global value.

use std::cell::RefCell;

use starlark::any::ProvidesStaticType;
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};

use crate::{outcome_from_steps, run_agent_step, AgentStepFn, AgentStepSpec, StepResult};
use crate::{scheduler_agents_spawned, WorkflowOutcome};

/// An error from authoring or evaluating a Starlark workflow program. Carries the
/// human-facing message Starlark produced (parse diagnostics or a runtime error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarlarkRunError {
    /// The script failed to parse (syntax error).
    Parse(String),
    /// The script raised an error during evaluation.
    Eval(String),
}

impl std::fmt::Display for StarlarkRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StarlarkRunError::Parse(msg) => write!(f, "workflow script parse error: {msg}"),
            StarlarkRunError::Eval(msg) => write!(f, "workflow script evaluation error: {msg}"),
        }
    }
}

impl std::error::Error for StarlarkRunError {}

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
}

impl StarlarkCtx<'_> {
    /// The phase a new step lands in: the last `phase()` call, else the default.
    fn phase_for(&self, explicit: Option<String>) -> String {
        explicit
            .or_else(|| self.current_phase.borrow().clone())
            .unwrap_or_else(|| self.default_phase.clone())
    }

    /// Run one agent step through the driver, record it, and return its result.
    fn run_one(
        &self,
        prompt: String,
        provider: String,
        label: Option<String>,
        phase: Option<String>,
        model: Option<String>,
        isolation: Option<String>,
    ) -> StepResult {
        let spec = AgentStepSpec {
            phase: self.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
        };
        let result = run_agent_step(self.driver, &spec);
        self.steps.borrow_mut().push(result.clone());
        result
    }

    /// Run a barrier fan-out over already-extracted plain specs. Drives the
    /// EXISTING crate-level [`crate::parallel`] (scheduler-backed), then records
    /// every [`StepResult`] in input order and returns their output summaries so
    /// the caller can build the script-visible return list. No Starlark value
    /// crosses a thread boundary — the specs were read off the heap before this.
    fn run_parallel(&self, specs: Vec<AgentStepSpec>) -> Vec<String> {
        let results = crate::parallel(self.driver, &specs);
        let summaries: Vec<String> = results
            .iter()
            .map(|result| result.output_summary.clone())
            .collect();
        self.steps.borrow_mut().extend(results);
        summaries
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
        out.push(AgentStepSpec {
            phase: ctx.phase_for(phase),
            label: label.unwrap_or_else(|| provider.clone()),
            provider,
            model,
            isolation,
            prompt,
        });
    }
    Ok(out)
}

/// The workflow host functions exposed to the script.
#[starlark_module]
fn workflow_globals(builder: &mut GlobalsBuilder) {
    /// Run one ephemeral provider worker synchronously and return its output text.
    fn agent<'v>(
        #[starlark(require = pos)] prompt: String,
        #[starlark(require = named, default = "codex".to_string())] provider: String,
        #[starlark(require = named)] label: Option<String>,
        #[starlark(require = named)] phase: Option<String>,
        #[starlark(require = named)] model: Option<String>,
        #[starlark(require = named)] isolation: Option<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<String> {
        let result = ctx_of(eval).run_one(prompt, provider, label, phase, model, isolation);
        Ok(result.output_summary)
    }

    /// Run a barrier fan-out: every spec runs concurrently and the call blocks
    /// until ALL of them finish (the barrier), then returns a list of their
    /// output-summary strings in input order. `specs` is a list of dicts, each
    /// with a required `prompt` and optional `provider` (default "codex"),
    /// `label`, `phase`, `model`, and `isolation`.
    fn parallel<'v>(
        #[starlark(require = pos)] specs: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let ctx = ctx_of(eval);
        // Extract every spec into PLAIN Rust before any threading — no Starlark
        // value may cross the barrier's thread boundary.
        let extracted = read_parallel_specs(ctx, specs)?;
        let summaries = ctx.run_parallel(extracted);
        let heap = eval.heap();
        let values: Vec<Value<'v>> = summaries.iter().map(|s| heap.alloc(s.as_str())).collect();
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

/// Evaluate a Starlark workflow program, driving every `agent()` call through
/// `driver`, and build a [`WorkflowOutcome`] from the steps it produced.
///
/// `name` is the workflow name (becomes the run's name and the default phase).
/// `args` is injected as the `args` global. The interpreter is hermetic: the script
/// has no access to the clock, randomness, or IO, so the orchestration is
/// deterministic — only the journaled `agent()` leaves are nondeterministic.
pub fn run_starlark(
    script: &str,
    name: &str,
    args: Option<&serde_json::Value>,
    driver: &AgentStepFn<'_>,
) -> Result<WorkflowOutcome, StarlarkRunError> {
    // Snapshot the scheduler's lifetime spawn counter so the delta attributes this
    // run's agents (matches `dispatch_spec`).
    let spawned_before = scheduler_agents_spawned();

    let ctx = StarlarkCtx {
        driver,
        default_phase: name.to_string(),
        current_phase: RefCell::new(None),
        steps: RefCell::new(Vec::new()),
        logs: RefCell::new(Vec::new()),
    };

    // `Extended` enables top-level statements (so an agent can write top-level
    // `for`/`if`), def, and lambdas — the expressive program shape we want.
    let ast = AstModule::parse("workflow.star", script.to_owned(), &Dialect::Extended)
        .map_err(|error| StarlarkRunError::Parse(error.to_string()))?;
    let globals = GlobalsBuilder::standard().with(workflow_globals).build();

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

    let steps = ctx.steps.into_inner();
    Ok(outcome_from_steps(name, steps, spawned_before))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::WorkflowRunStatus;
    use std::sync::Mutex;

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
            run_starlark(script, "demo", None, &driver).expect("run ok")
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
            run_starlark(script, "demo", Some(&args), &driver).expect("run ok")
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].1.contains("audit checkout flow"));
        assert_eq!(outcome.steps.len(), 1);
    }

    #[test]
    fn a_failed_step_makes_the_run_failed() {
        // A driver that fails every step → 0 ok → Failed (mirrors dispatch_spec).
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
        };
        let outcome = run_starlark(r#"agent("x")"#, "demo", None, &driver).expect("run ok");
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
            run_starlark(script, "demo", Some(&args), &driver).expect("run ok")
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
                }
            };
            run_starlark(script, "demo", None, &driver).expect("run ok")
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
}
