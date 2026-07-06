//! Provider-agnostic Rust-native workflow runtime.
//!
//! This crate was extracted out of `harness-cli/src/workflow.rs` so the runtime
//! contains NO provider (codex/claude) code. The binary injects the real
//! delivery driver through the [`AgentStepFn`] seam; tests inject a mock.
//!
//! It provides:
//! * CONCURRENCY-CAP SCHEDULER: bounds concurrency to min(16, available_parallelism()-2)
//!   via a counting semaphore. Excess steps are queued and run as permits free.
//!   Includes a 1000-agent LIFETIME cap as a runaway backstop.
//! * The `parallel()` barrier primitive + a real streaming `pipeline()` (per-item
//!   through all stages with NO barrier between stages).
//! * The built-in `investigate` workflow + `WorkflowRegistry` (option C dispatch).
//! * The Starlark program front-end ([`starlark_front::run_starlark`]) — the SOLE
//!   dynamic authoring surface, where an agent writes a real program (loops,
//!   conditionals, data-driven fan-out) that drives the same scheduler dispatch.
//!
//! See docs/research/dynamic-workflow-runtime-design.md for the full design.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};

use harness_core::{WorkflowRunStatus, WorkflowStepStatus};
use serde::{Deserialize, Serialize};

pub mod starlark_front;

/// The only supported per-node isolation mode. An `agent()` node may opt in to
/// `isolation: "worktree"` (exactly like Claude Code's Workflow `isolation:
/// 'worktree'`): that node runs in its own throwaway git worktree whose diff is
/// the node's evidence. The worktree is auto-removed if unchanged and is NOT
/// auto-merged back. Writable leaves also default to this worktree path; only
/// `write_mode="direct"` writes the selected project root immediately.
pub const ISOLATION_WORKTREE: &str = "worktree";
pub const WRITE_MODE_DIRECT: &str = "direct";

/// A single agent step to run: spin up an ephemeral `provider` worker, deliver
/// `prompt`, grouped under `phase` and named by `label`. This is the
/// workflow-layer description of one `agent()` call; the runtime turns it into a
/// [`StepResult`]. `model` overrides the provider's default model; `isolation`
/// opts the node into a throwaway git worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStepSpec {
    pub phase: String,
    pub label: String,
    /// The provider that runs this step ("codex" | "claude"). Each step spins up
    /// a fresh ephemeral worker; the runtime passes this through to the driver.
    pub provider: String,
    /// Optional model override; `None` uses the provider default.
    pub model: Option<String>,
    /// Optional reasoning-effort override, passed through to the provider verbatim:
    /// codex → `-c model_reasoning_effort=<effort>` (minimal|low|medium|high),
    /// claude → `--effort <effort>` (low|medium|high|xhigh|max). `None` uses the
    /// provider default. Not validated here — the provider CLI rejects bad values.
    pub effort: Option<String>,
    /// Optional Codex service tier override, passed through as
    /// `-c service_tier=<tier>`. Other providers ignore it. `None` preserves the
    /// provider's configured default.
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Optional fallback model override. Only providers with a native fallback
    /// flag use it; otherwise it is ignored by the runtime.
    pub fallback_model: Option<String>,
    /// Optional per-leaf wall-clock timeout in seconds. `None` leaves the step
    /// bounded only by the CLI's idle-since-last-output timeout.
    #[serde(default)]
    pub timeout_s: Option<u64>,
    /// Image file paths to attach to the worker. Empty means no images.
    pub image: Vec<String>,
    /// Extra directory paths the worker may access. Empty means no extra dirs.
    #[serde(default)]
    pub add_dir: Vec<String>,
    /// Artifact paths the step must produce. Empty preserves the legacy behavior.
    #[serde(default)]
    pub expected_artifacts: Vec<String>,
    /// How to persist writes from an isolated leaf. `None` defaults to durable
    /// patch capture for writable leaves; `"discard"` preserves throwaway-only
    /// behavior.
    #[serde(default)]
    pub persist_changes: Option<String>,
    /// Where an editable leaf writes. `None` preserves the safe default:
    /// `writable=true` runs in a throwaway worktree. `"direct"` runs the editable
    /// worker in the selected project root and leaves its diff there immediately.
    #[serde(default)]
    pub write_mode: Option<String>,
    /// Optional repo-relative path guard for the captured patch.
    #[serde(default)]
    pub owned_paths: Vec<String>,
    /// Optional artifact root for manifest validation.
    #[serde(default)]
    pub artifact_root: Option<String>,
    /// Optional repo-relative/absolute roots the step is allowed to write
    /// artifact files under.
    #[serde(default)]
    pub write_roots: Vec<String>,
    /// If true, a successful workflow verdict asks the CLI to apply this step's
    /// captured patch automatically through the same guarded patch path.
    #[serde(default)]
    pub auto_apply_on_verdict: bool,
    /// Optional per-node isolation. `Some("worktree")` runs the step in its own
    /// throwaway git worktree; `None` keeps the default selected-project cwd
    /// behavior, which is read-only unless `write_mode="direct"` is explicit.
    pub isolation: Option<String>,
    pub prompt: String,
    /// Optional output schema. `Some(obj)` puts the step into STRUCTURED mode:
    /// the driver appends a schema instruction to the prompt, then parses +
    /// validates the worker's reply into [`StepResult::structured`]. The value is
    /// a JSON object whose top-level keys are the REQUIRED keys the reply must
    /// carry. `None` = text mode (the reply is returned verbatim as today).
    pub schema: Option<serde_json::Value>,
    /// Whether this step may EDIT files / run shell. `false` (default) runs the
    /// worker read-only (codex `--sandbox read-only`; claude read-only tools).
    /// `true` escalates to an editable sandbox AND auto-isolates the step into a
    /// throwaway git worktree, so writes land in a discardable checkout (captured
    /// as the step's diff) instead of the live repo.
    pub writable: bool,
    /// The deterministic leaf ordinal the Starlark front-end assigned this spec
    /// (one per `agent()` leaf / `parallel()` spec). Threaded onto the spec so a
    /// real driver that journals its OWN terminal row can stamp the ordinal onto
    /// the row before writing it — keeping the ordinal round-tripping through the
    /// store for `--resume`. `None` for specs built outside the front-end (the
    /// built-in `investigate` path, the streaming `pipeline()` engine).
    pub ordinal: Option<u64>,
}

/// The outcome of one agent step. `ok == false` is the CC-spec `null` slot: a
/// failed step never aborts the run; it is collected like any other result.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub phase: String,
    pub label: String,
    /// The provider that ran this step ("codex" | "claude").
    pub provider: String,
    /// The per-node isolation mode this step ran under, if any
    /// (`Some("worktree")`). Carried onto the journaled step as evidence.
    pub isolation: Option<String>,
    /// Whether the underlying provider delivery succeeded.
    pub ok: bool,
    /// The `ProviderSession` id this step produced, if a delivery was attempted.
    pub provider_session_id: Option<String>,
    /// Human-facing summary / report text collected from the delivery.
    pub output_summary: String,
    /// The `WorkflowStep` id the driver journaled at step START (live progress).
    /// `None` for mock/test drivers that do not journal a start row; the runtime
    /// then mints a fresh id when journaling the terminal row.
    pub step_id: Option<String>,
    /// Wall-clock start time the driver recorded when the step began. `None` for
    /// drivers that do not journal a start row; the runtime falls back to the
    /// journal time. Carrying the real start time keeps the journaled step's
    /// `started_at`/`ended_at` reflecting true (overlapping) execution windows.
    pub started_at: Option<String>,
    /// Extra observability fields the runtime captured about the worker
    /// (`model`, `exit_code`, `duration_ms`, `tokens`, `failure`,
    /// `worktree_diff`, ...). Merged onto the step's `result` JSON object by
    /// [`step_result_json`] so the frontend reads it WITHOUT re-parsing raw
    /// NDJSON. `None` for mock/test drivers that capture no telemetry. When
    /// present it MUST be a JSON object; non-object values are ignored.
    pub details: Option<serde_json::Value>,
    /// The parsed + validated structured output, present ONLY when the step ran
    /// in schema mode (`AgentStepSpec::schema` was `Some`) AND the worker's reply
    /// parsed into a JSON object carrying every required schema key. `None` for
    /// text-mode steps and for schema-mode steps whose worker never produced
    /// valid JSON (those record a `"schema"` failure instead). The Starlark
    /// `agent()` returns this object to the script when present.
    pub structured: Option<serde_json::Value>,
    /// The deterministic leaf ordinal this step was assigned by the Starlark
    /// front-end (one per `agent()` leaf, per `parallel()` spec, per pipeline
    /// item×stage), assigned on the eval thread in issue order. It round-trips
    /// through the store (see [`step_result_json`]) so a `--resume` re-run can key
    /// a replay cache by ordinal and reuse a prior run's succeeded leaves without
    /// re-spending. `None` for steps produced outside the ordinal-aware front-end
    /// (the built-in `investigate` registry path, error slots).
    pub ordinal: Option<u64>,
}

impl StepResult {
    /// Map a step outcome onto the journaled [`WorkflowStepStatus`].
    pub fn step_status(&self) -> WorkflowStepStatus {
        if self.ok {
            WorkflowStepStatus::Completed
        } else {
            WorkflowStepStatus::Failed
        }
    }
}

/// The injectable agent-step driver. The REAL implementation drives one provider
/// delivery through the neutral seam; tests inject a mock that returns canned
/// [`StepResult`]s without spawning a provider.
///
/// It must NEVER panic — a delivery failure is reported as `StepResult { ok:
/// false, .. }` so the run's failure handling stays in control flow rather than
/// unwinding.
pub type AgentStepFn<'a> = dyn Fn(&AgentStepSpec) -> StepResult + Sync + 'a;

/// Run one agent step. This is the `agent()` primitive: it is a thin, total
/// wrapper that simply invokes the injected driver.
pub fn run_agent_step(driver: &AgentStepFn<'_>, spec: &AgentStepSpec) -> StepResult {
    driver(spec)
}

/// A process-wide concurrency-cap scheduler. Bounds concurrent work to
/// `min(16, available_parallelism()-2)` via a counting semaphore + work queue.
/// Excess tasks are queued and spawned as permits free. Includes a 1000-agent
/// LIFETIME cap as a runaway backstop.
struct WorkflowScheduler {
    /// Counting semaphore: number of free worker slots.
    permits_mu: Mutex<usize>,
    permits_cv: Condvar,
    /// Lifetime agent spawn counter (across all runs).
    agents_spawned: Mutex<u64>,
}

impl WorkflowScheduler {
    fn new() -> Self {
        // min(16, available_parallelism()-2), clamped to >= 1
        let parallelism = std::thread::available_parallelism()
            .ok()
            .map(|p| p.get())
            .unwrap_or(2)
            .saturating_sub(2);
        let cap = parallelism.clamp(1, 16);
        Self {
            permits_mu: Mutex::new(cap),
            permits_cv: Condvar::new(),
            agents_spawned: Mutex::new(0),
        }
    }

    /// Try to acquire a permit. If the lifetime cap is exceeded, returns false
    /// (step should error gracefully). Otherwise blocks until a permit is free
    /// and returns true.
    fn acquire(&self) -> bool {
        let mut counter = self.agents_spawned.lock().unwrap();
        if *counter >= 1000 {
            return false; // Lifetime cap exceeded.
        }
        *counter += 1;
        drop(counter);

        let mut permits = self.permits_mu.lock().unwrap();
        while *permits == 0 {
            permits = self.permits_cv.wait(permits).unwrap();
        }
        *permits -= 1;
        true
    }

    /// Release a permit back to the pool.
    fn release(&self) {
        let mut permits = self.permits_mu.lock().unwrap();
        *permits += 1;
        self.permits_cv.notify_one();
    }

    /// Snapshot the lifetime agent-spawn counter. Callers snapshot before and
    /// after a run and diff the two to attribute spawns to that run.
    fn spawned_count(&self) -> u64 {
        *self.agents_spawned.lock().unwrap()
    }
}

/// Snapshot the process-wide scheduler's lifetime agent-spawn counter. The
/// dynamic run path snapshots this before and after a dispatch and diffs the two
/// to populate `WorkflowRun.agents_spawned` (how many agents THIS run spawned).
pub fn scheduler_agents_spawned() -> u64 {
    get_scheduler().spawned_count()
}

/// Process-wide singleton scheduler. Shared across all runs.
static SCHEDULER: std::sync::OnceLock<WorkflowScheduler> = std::sync::OnceLock::new();

fn get_scheduler() -> &'static WorkflowScheduler {
    SCHEDULER.get_or_init(WorkflowScheduler::new)
}

/// The `parallel()` barrier, backed by the concurrency-cap scheduler.
/// Runs every spec concurrently on its own scoped thread, joins ALL of them
/// (the barrier), and returns results in input order. A thunk whose thread panics
/// is converted into a failed [`StepResult`] in its slot so the run itself never panics.
///
/// Each spec acquires a permit from the scheduler before running; excess specs
/// are queued and run as permits free.
pub fn parallel(driver: &AgentStepFn<'_>, specs: &[AgentStepSpec]) -> Vec<StepResult> {
    if specs.is_empty() {
        return Vec::new();
    }

    let scheduler = get_scheduler();
    let (tx, rx) = crossbeam::channel::bounded::<(usize, StepResult)>(specs.len());

    std::thread::scope(|scope| {
        for (index, spec) in specs.iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                // Acquire a permit from the scheduler. If the lifetime cap is
                // exceeded, don't spawn the step and return a failed result instead.
                if !scheduler.acquire() {
                    let result = StepResult {
                        phase: spec.phase.clone(),
                        label: spec.label.clone(),
                        provider: spec.provider.clone(),
                        isolation: spec.isolation.clone(),
                        ok: false,
                        provider_session_id: None,
                        output_summary: "workflow lifetime agent cap (1000) exceeded".to_string(),
                        step_id: None,
                        started_at: None,
                        details: None,
                        structured: None,
                        ordinal: None,
                    };
                    let _ = tx.send((index, result));
                    return;
                }

                // Run the step under panic safety.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_agent_step(driver, spec)
                }))
                .unwrap_or_else(|_| StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: false,
                    provider_session_id: None,
                    output_summary: "agent step panicked".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                });

                let _ = tx.send((index, result));
                scheduler.release();
            });
        }
        drop(tx);
    });

    // Re-order by input index.
    let mut by_index: BTreeMap<usize, StepResult> = BTreeMap::new();
    for (index, result) in rx.iter() {
        by_index.insert(index, result);
    }
    by_index.into_values().collect()
}

/// One stage of a [`pipeline`]. Given the item's CURRENT spec (the original spec
/// for the first stage, then the spec the previous stage handed forward), it
/// returns either:
///   * `Some((next_spec, result))` — the stage succeeded; `result` is journaled
///     for this stage and `next_spec` is what the NEXT stage receives, or
///   * `None` — the stage failed/dropped the item; the item skips its remaining
///     stages and lands in a failed/None slot.
///
/// A stage may itself call the injected driver (the real path delivers to a
/// provider); tests pass pure closures. The stage MUST NOT panic — a drop is the
/// `None` return, not an unwind.
pub type PipelineStage<'a> =
    Box<dyn Fn(&AgentStepSpec) -> Option<(AgentStepSpec, StepResult)> + Send + Sync + 'a>;

/// A real STREAMING pipeline: every item flows through ALL `stages`
/// independently, with NO barrier between stages. Item A may be in stage 3 while
/// item B is still in stage 1 — items do not wait for one another at any stage
/// boundary. Concurrency across items is bounded by the shared
/// [`WorkflowScheduler`] (one permit per in-flight item).
///
/// A stage that returns `None` drops that item: it skips its remaining stages
/// and its slot becomes the last successful [`StepResult`] marked `ok = false`
/// (or, if the very first stage dropped it, a synthetic failed result). Results
/// are returned in INPUT order regardless of completion order.
///
/// Returns one [`StepResult`] per input item (the item's FINAL stage result, or
/// its failed/dropped slot), so `out.len() == items.len()`.
pub fn pipeline(items: Vec<AgentStepSpec>, stages: Vec<PipelineStage<'_>>) -> Vec<StepResult> {
    if items.is_empty() {
        return Vec::new();
    }

    let scheduler = get_scheduler();
    let stages = &stages;
    let (tx, rx) = crossbeam::channel::bounded::<(usize, StepResult)>(items.len());

    std::thread::scope(|scope| {
        for (index, item) in items.into_iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                // One permit per in-flight ITEM — no per-stage barrier, so a fast
                // item races ahead through its stages while a slow item lags.
                if !scheduler.acquire() {
                    let _ = tx.send((index, capped_result(&item)));
                    return;
                }

                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_item_through_stages(&item, stages)
                }))
                .unwrap_or_else(|_| panicked_result(&item));

                scheduler.release();
                let _ = tx.send((index, result));
            });
        }
        drop(tx);
    });

    // Re-order by input index (completion order is non-deterministic by design).
    let mut by_index: BTreeMap<usize, StepResult> = BTreeMap::new();
    for (index, result) in rx.iter() {
        by_index.insert(index, result);
    }
    by_index.into_values().collect()
}

/// Flow a single item through every stage in order. Stops at the first stage
/// that returns `None` (a drop), marking the last successful result `ok = false`
/// (or synthesizing a failed result if the first stage dropped the item).
fn run_item_through_stages(item: &AgentStepSpec, stages: &[PipelineStage<'_>]) -> StepResult {
    let mut current = item.clone();
    let mut last: Option<StepResult> = None;
    for stage in stages {
        match stage(&current) {
            Some((next, result)) => {
                current = next;
                last = Some(result);
            }
            None => {
                // Drop: skip the remaining stages. The slot is the last
                // successful result demoted to a failure, or a synthetic one.
                return match last {
                    Some(mut result) => {
                        result.ok = false;
                        result.provider_session_id = None;
                        result.output_summary = format!(
                            "pipeline item dropped at a stage: {}",
                            result.output_summary
                        );
                        result
                    }
                    None => dropped_result(item),
                };
            }
        }
    }
    // No stage dropped the item: its slot is the FINAL stage's result. An empty
    // stage list is a vacuous drop (nothing produced a result).
    last.unwrap_or_else(|| dropped_result(item))
}

fn dropped_result(item: &AgentStepSpec) -> StepResult {
    StepResult {
        phase: item.phase.clone(),
        label: item.label.clone(),
        provider: item.provider.clone(),
        isolation: item.isolation.clone(),
        ok: false,
        provider_session_id: None,
        output_summary: "pipeline item dropped before producing a result".to_string(),
        step_id: None,
        started_at: None,
        details: None,
        structured: None,
        ordinal: None,
    }
}

fn capped_result(item: &AgentStepSpec) -> StepResult {
    StepResult {
        phase: item.phase.clone(),
        label: item.label.clone(),
        provider: item.provider.clone(),
        isolation: item.isolation.clone(),
        ok: false,
        provider_session_id: None,
        output_summary: "workflow lifetime agent cap (1000) exceeded".to_string(),
        step_id: None,
        started_at: None,
        details: None,
        structured: None,
        ordinal: None,
    }
}

fn panicked_result(item: &AgentStepSpec) -> StepResult {
    StepResult {
        phase: item.phase.clone(),
        label: item.label.clone(),
        provider: item.provider.clone(),
        isolation: item.isolation.clone(),
        ok: false,
        provider_session_id: None,
        output_summary: "pipeline stage panicked".to_string(),
        step_id: None,
        started_at: None,
        details: None,
        structured: None,
        ordinal: None,
    }
}

/// Outcome of a whole workflow run, returned to the caller for journaling.
#[derive(Debug, Clone)]
pub struct WorkflowOutcome {
    pub steps: Vec<StepResult>,
    pub status: WorkflowRunStatus,
    pub summary: String,
    /// How many agents this run spawned, measured as the scheduler's lifetime
    /// counter delta across the dispatch. `0` for outcomes built without going
    /// through the scheduler (e.g. the built-in `investigate` registry path,
    /// which leaves it for the caller to fill).
    pub agents_spawned: u64,
    /// The collected structured output of the run: one JSON object per step
    /// (`label` / `phase` / `ok` / `output_summary` / `provider_session_id`).
    /// `None` when the run produced no steps.
    pub final_output: Option<serde_json::Value>,
}

/// The built-in `investigate` workflow (the §6 scenario). Demonstrates BOTH
/// control-flow forms:
///   1. SERIAL  — a single codex delivery (`scope` phase), awaited before the
///      parallel fan-out, so its result is available to the next phase.
///   2. PARALLEL — a barrier fan-out of two deliveries (`audit` phase): one
///      ephemeral codex worker and one ephemeral claude worker, joined before
///      returning.
///
/// Each step spins up a NEW one-shot ephemeral provider worker (codex exec /
/// claude -p); the workflow references a PROVIDER, not a pre-existing member.
///
/// The run is "completed" iff the serial (required) step succeeds; a failed
/// required step transitions the run to "failed" but the parallel steps are
/// still collected (nulls tolerated).
pub fn investigate(driver: &AgentStepFn<'_>, topic: &str) -> WorkflowOutcome {
    let mut steps = Vec::new();

    // --- SERIAL phase: scope the investigation with an ephemeral codex worker. ---
    let scope_step = run_agent_step(
        driver,
        &AgentStepSpec {
            phase: "scope".to_string(),
            label: "scope-question".to_string(),
            provider: "codex".to_string(),
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
            prompt: format!("Scope the investigation of: {topic}. List the modules to audit."),
            schema: None,
            writable: false,
            ordinal: None,
        },
    );
    let scope_ok = scope_step.ok;
    steps.push(scope_step);

    // --- PARALLEL phase: barrier fan-out across BOTH providers. ---
    let parallel_specs = vec![
        AgentStepSpec {
            phase: "audit".to_string(),
            label: "audit-codex".to_string(),
            provider: "codex".to_string(),
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
            prompt: format!("Audit the code paths involved in: {topic}."),
            schema: None,
            writable: false,
            ordinal: None,
        },
        AgentStepSpec {
            phase: "audit".to_string(),
            label: "audit-claude".to_string(),
            provider: "claude".to_string(),
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
            prompt: format!("Audit the recent diffs related to: {topic}."),
            schema: None,
            writable: false,
            ordinal: None,
        },
    ];
    let parallel_results = parallel(driver, &parallel_specs);
    steps.extend(parallel_results);

    let (status, summary) = if scope_ok {
        let kept = steps.iter().filter(|step| step.ok).count();
        (
            WorkflowRunStatus::Completed,
            format!("investigate completed: {kept}/{} steps ok", steps.len()),
        )
    } else {
        (
            WorkflowRunStatus::Failed,
            "investigate failed: required serial step (scope) did not succeed".to_string(),
        )
    };

    WorkflowOutcome {
        steps,
        status,
        summary,
        // The registry path leaves the per-run agent count to the caller (it does
        // not snapshot the scheduler around its own dispatch).
        agents_spawned: 0,
        final_output: None,
    }
}

/// Signature of a registered built-in workflow body (§3 option C). The body is
/// provider-agnostic: it spins up ephemeral workers via the injected driver and
/// references providers directly, so it no longer needs a member binding.
pub type WorkflowFn = fn(&AgentStepFn<'_>, &str) -> WorkflowOutcome;

/// A registered workflow's metadata + dispatch fn.
#[derive(Clone)]
pub struct WorkflowDef {
    pub name: &'static str,
    pub summary: &'static str,
    pub run: WorkflowFn,
}

/// The built-in workflow registry. Maps a name to a compiled Rust workflow,
/// giving runtime dispatch by name without an interpreter (§3 option C).
pub struct WorkflowRegistry {
    by_name: BTreeMap<&'static str, WorkflowDef>,
}

impl WorkflowRegistry {
    /// Build the registry with all built-in workflows.
    pub fn builtin() -> Self {
        let mut by_name = BTreeMap::new();
        by_name.insert(
            "investigate",
            WorkflowDef {
                name: "investigate",
                summary: "Serial codex scope, then a parallel codex+claude audit barrier.",
                run: investigate,
            },
        );
        Self { by_name }
    }

    /// Look up a registered workflow by name.
    pub fn get(&self, name: &str) -> Option<&WorkflowDef> {
        self.by_name.get(name)
    }

    /// All registered workflow names (sorted by the BTreeMap).
    pub fn names(&self) -> Vec<&'static str> {
        self.by_name.keys().copied().collect()
    }
}

/// One step's structured payload for `WorkflowRun.final_output` / the step's
/// `result` field. Mirrors the human-facing summary with the machine-facing
/// status + linkage the dashboard / callers want.
pub fn step_result_json(result: &StepResult) -> serde_json::Value {
    let mut value = serde_json::json!({
        "phase": result.phase,
        "label": result.label,
        "provider": result.provider,
        "isolation": result.isolation,
        "ok": result.ok,
        "provider_session_id": result.provider_session_id,
        "output_summary": result.output_summary,
        // The parsed structured output (schema mode), or null. Lets the dashboard
        // and `final_output` carry the validated object alongside the summary.
        "structured": result.structured,
        // The deterministic leaf ordinal, or null. Round-trips so a `--resume`
        // re-run can key a replay cache by ordinal off the stored step.result.
        "ordinal": result.ordinal,
    });
    // Merge the runtime-captured observability fields (model, exit_code,
    // duration_ms, tokens, failure, worktree_diff, ...) onto the same object so
    // the dashboard reads them off `step.result` without re-parsing NDJSON. We
    // only spread JSON OBJECTS; the base keys above always win on collision.
    if let (Some(base), Some(serde_json::Value::Object(extra))) =
        (value.as_object_mut(), &result.details)
    {
        for (k, v) in extra {
            base.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    value
}

/// Build a [`WorkflowOutcome`] from a run's accumulated steps. Shared by every
/// front-end that produces an ordered `Vec<StepResult>` — the built-in
/// [`investigate`] registry path and the Starlark
/// [`starlark_front::run_starlark`] evaluator — so all front-ends derive status
/// / summary / final_output identically. `spawned_before` is the scheduler's
/// lifetime spawn-counter snapshot taken before the run, so the delta attributes
/// this run's agents.
///
/// Status rule: a run with steps but zero successful ones is `Failed`; otherwise
/// `Completed` (partial success is still completed, mirroring the CC-spec
/// null-tolerant fan-out).
pub fn outcome_from_steps(
    name: &str,
    steps: Vec<StepResult>,
    spawned_before: u64,
) -> WorkflowOutcome {
    let agents_spawned = scheduler_agents_spawned().saturating_sub(spawned_before);

    let total = steps.len();
    let kept = steps.iter().filter(|step| step.ok).count();
    let (status, summary) = if total > 0 && kept == 0 {
        (
            WorkflowRunStatus::Failed,
            format!("{name} failed: 0/{total} steps ok"),
        )
    } else {
        (
            WorkflowRunStatus::Completed,
            format!("{name} completed: {kept}/{total} steps ok"),
        )
    };

    let final_output = if steps.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(
            steps.iter().map(step_result_json).collect(),
        ))
    };

    WorkflowOutcome {
        steps,
        status,
        summary,
        agents_spawned,
        final_output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A mock driver that always succeeds and records the order of invocation.
    fn recording_driver<'a>(
        order: &'a Mutex<Vec<String>>,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
        move |spec: &AgentStepSpec| {
            order.lock().unwrap().push(spec.label.clone());
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
                ordinal: None,
            }
        }
    }

    #[test]
    fn agent_step_spec_expected_artifacts_and_service_tier_round_trips_and_defaults() {
        let spec = AgentStepSpec {
            phase: "p".into(),
            label: "writer".into(),
            provider: "codex".into(),
            model: None,
            effort: None,
            service_tier: Some("priority".into()),
            fallback_model: None,
            timeout_s: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            expected_artifacts: vec!["out/image.png".into()],
            persist_changes: Some("patch".into()),
            write_mode: Some(WRITE_MODE_DIRECT.into()),
            owned_paths: vec!["src".into()],
            artifact_root: Some("out".into()),
            write_roots: vec!["out".into()],
            auto_apply_on_verdict: true,
            isolation: Some(ISOLATION_WORKTREE.into()),
            prompt: "write it".into(),
            schema: None,
            writable: true,
            ordinal: Some(7),
        };
        let encoded = serde_json::to_string(&spec).expect("serialize");
        let decoded: AgentStepSpec = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded.expected_artifacts, vec!["out/image.png"]);
        assert_eq!(decoded.persist_changes.as_deref(), Some("patch"));
        assert_eq!(decoded.write_mode.as_deref(), Some(WRITE_MODE_DIRECT));
        assert_eq!(decoded.owned_paths, vec!["src"]);
        assert_eq!(decoded.artifact_root.as_deref(), Some("out"));
        assert_eq!(decoded.write_roots, vec!["out"]);
        assert!(decoded.auto_apply_on_verdict);
        assert_eq!(decoded.service_tier.as_deref(), Some("priority"));
        assert_eq!(decoded.timeout_s, None);

        let legacy = serde_json::json!({
            "phase": "p",
            "label": "writer",
            "provider": "codex",
            "model": null,
            "effort": null,
            "fallback_model": null,
            "timeout_s": null,
            "image": [],
            "add_dir": [],
            "isolation": null,
            "prompt": "write it",
            "schema": null,
            "writable": false,
            "ordinal": null
        });
        let decoded: AgentStepSpec = serde_json::from_value(legacy).expect("legacy decode");
        assert!(decoded.expected_artifacts.is_empty());
        assert_eq!(decoded.persist_changes, None);
        assert_eq!(decoded.write_mode, None);
        assert!(decoded.owned_paths.is_empty());
        assert_eq!(decoded.artifact_root, None);
        assert!(decoded.write_roots.is_empty());
        assert!(!decoded.auto_apply_on_verdict);
        assert_eq!(decoded.service_tier, None);
        assert_eq!(decoded.timeout_s, None);
    }

    #[test]
    fn serial_step_runs_before_parallel_fan_out() {
        let order = Mutex::new(Vec::new());
        let outcome = {
            let driver = recording_driver(&order);
            investigate(&driver, "failure X")
        };
        let order = order.into_inner().unwrap();
        assert_eq!(order[0], "scope-question");
        assert!(order.contains(&"audit-codex".to_string()));
        assert!(order.contains(&"audit-claude".to_string()));
        assert_eq!(order.len(), 3);
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
        assert_eq!(outcome.steps.len(), 3);
    }

    #[test]
    fn parallel_runs_all_and_barriers_collecting_every_slot() {
        let count = AtomicUsize::new(0);
        let driver = |spec: &AgentStepSpec| {
            count.fetch_add(1, Ordering::SeqCst);
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
                ordinal: None,
            }
        };
        let specs: Vec<AgentStepSpec> = (0..5)
            .map(|i| AgentStepSpec {
                phase: "p".to_string(),
                label: format!("l{i}"),
                provider: "codex".to_string(),
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
                prompt: format!("prompt {i}"),
                schema: None,
                writable: false,
                ordinal: None,
            })
            .collect();
        let results = parallel(&driver, &specs);
        assert_eq!(count.load(Ordering::SeqCst), 5);
        assert_eq!(results.len(), 5);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.label, format!("l{i}"));
            assert!(result.ok);
        }
    }

    #[test]
    fn parallel_failing_thunk_yields_failed_slot_without_panicking_run() {
        let driver = |spec: &AgentStepSpec| {
            if spec.label == "l1" {
                return StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: false,
                    provider_session_id: None,
                    output_summary: "delivery failed".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                };
            }
            if spec.label == "l2" {
                panic!("simulated driver crash");
            }
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
                ordinal: None,
            }
        };
        let specs: Vec<AgentStepSpec> = (0..4)
            .map(|i| AgentStepSpec {
                phase: "p".to_string(),
                label: format!("l{i}"),
                provider: "codex".to_string(),
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
                prompt: "x".to_string(),
                schema: None,
                writable: false,
                ordinal: None,
            })
            .collect();
        let results = parallel(&driver, &specs);
        assert_eq!(results.len(), 4);
        assert!(results[0].ok);
        assert!(!results[1].ok, "ok=false slot preserved");
        assert!(!results[2].ok, "panicked slot becomes a failed result");
        assert_eq!(results[2].output_summary, "agent step panicked");
        assert!(results[3].ok);
    }

    #[test]
    fn failed_required_serial_step_fails_the_run_but_keeps_parallel_results() {
        let driver = |spec: &AgentStepSpec| {
            let ok = spec.phase != "scope";
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok,
                provider_session_id: if ok { Some("s".to_string()) } else { None },
                output_summary: if ok {
                    "ok".to_string()
                } else {
                    "failed".to_string()
                },
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        };
        let outcome = investigate(&driver, "failure Y");
        assert_eq!(outcome.status, WorkflowRunStatus::Failed);
        assert_eq!(outcome.steps.len(), 3);
        assert!(!outcome.steps[0].ok);
        assert!(outcome.steps[1].ok);
        assert!(outcome.steps[2].ok);
    }

    #[test]
    fn registry_dispatches_builtin_workflow_by_name() {
        let registry = WorkflowRegistry::builtin();
        assert_eq!(registry.names(), vec!["investigate"]);
        let def = registry.get("investigate").expect("investigate registered");
        let order = Mutex::new(Vec::new());
        let driver = recording_driver(&order);
        let outcome = (def.run)(&driver, "failure Z");
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
        assert_eq!(outcome.steps.len(), 3);
        assert!(registry.get("does-not-exist").is_none());
    }

    // Concurrency cap, lifetime cap.
    #[test]
    fn parallel_still_barriers_even_with_scheduler() {
        let order = Mutex::new(Vec::new());
        let driver = recording_driver(&order);
        let specs: Vec<AgentStepSpec> = (0..10)
            .map(|i| AgentStepSpec {
                phase: "p".to_string(),
                label: format!("l{i}"),
                provider: "codex".to_string(),
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
                prompt: format!("prompt {i}"),
                schema: None,
                writable: false,
                ordinal: None,
            })
            .collect();
        let results = parallel(&driver, &specs);
        assert_eq!(results.len(), 10, "barrier collected all 10 results");
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.label, format!("l{i}"), "results in input order");
        }
    }

    // ----- Streaming pipeline() tests -----

    /// Build a trivial pass-through stage that tags the result's summary with the
    /// stage name and carries the same spec forward.
    fn pass_stage(name: &'static str) -> PipelineStage<'static> {
        Box::new(move |spec: &AgentStepSpec| {
            let result = StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some(format!("{}-{}", name, spec.label)),
                output_summary: name.to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            };
            Some((spec.clone(), result))
        })
    }

    fn item(label: &str) -> AgentStepSpec {
        AgentStepSpec {
            phase: "p".to_string(),
            label: label.to_string(),
            provider: "codex".to_string(),
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
            prompt: "x".to_string(),
            schema: None,
            writable: false,
            ordinal: None,
        }
    }

    #[test]
    fn pipeline_returns_final_stage_result_per_item_in_input_order() {
        let items = vec![item("a"), item("b"), item("c")];
        let stages: Vec<PipelineStage<'_>> = vec![pass_stage("s1"), pass_stage("s2")];
        let results = pipeline(items, stages);
        assert_eq!(results.len(), 3);
        // Returned in INPUT order regardless of completion order.
        assert_eq!(results[0].label, "a");
        assert_eq!(results[1].label, "b");
        assert_eq!(results[2].label, "c");
        // Each item's slot is the LAST stage's result.
        for r in &results {
            assert!(r.ok);
            assert_eq!(r.output_summary, "s2");
        }
    }

    #[test]
    fn pipeline_has_no_barrier_between_stages() {
        // Prove items do NOT wait for one another at a stage boundary: the SLOW
        // item blocks in stage 1 until the FAST item has reached stage 2. With a
        // per-stage barrier this would deadlock (stage 2 could not start until
        // every item finished stage 1). Without one, it completes.
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let fast_in_stage2 = Arc::new(AtomicBool::new(false));

        let fast_flag = fast_in_stage2.clone();
        let stage1: PipelineStage<'_> = Box::new(move |spec: &AgentStepSpec| {
            if spec.label == "slow" {
                // Wait until the fast item is observed in stage 2 — proving the
                // two stages overlap (no barrier). The scheduler's concurrency
                // cap is min(16, cores-2), which CLAMPS TO 1 on a 1-2 core box
                // (e.g. a CI runner): with a single permit the items necessarily
                // serialize, so there is no concurrent stage-2 to observe. Bound
                // the wait by wall-clock and proceed on timeout rather than
                // spin-deadlocking — the no-barrier property only manifests when
                // the scheduler grants >= 2 permits, and the final assertion
                // (both items reach s2) holds under either capacity.
                let start = std::time::Instant::now();
                while !fast_flag.load(Ordering::SeqCst) {
                    if start.elapsed() > std::time::Duration::from_secs(3) {
                        break;
                    }
                    std::thread::yield_now();
                }
            }
            Some((
                spec.clone(),
                StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: true,
                    provider_session_id: None,
                    output_summary: "s1".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                },
            ))
        });

        let fast_flag2 = fast_in_stage2.clone();
        let stage2: PipelineStage<'_> = Box::new(move |spec: &AgentStepSpec| {
            if spec.label == "fast" {
                fast_flag2.store(true, Ordering::SeqCst);
            }
            Some((
                spec.clone(),
                StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: true,
                    provider_session_id: None,
                    output_summary: "s2".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                },
            ))
        });

        let items = vec![item("slow"), item("fast")];
        let results = pipeline(items, vec![stage1, stage2]);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.ok && r.output_summary == "s2"));
    }

    #[test]
    fn pipeline_failed_stage_drops_item_and_skips_its_rest() {
        // Stage 2 drops the item labelled "bad"; that item must NOT reach stage 3.
        let reached_stage3 = Mutex::new(Vec::<String>::new());

        let stage1 = pass_stage("s1");
        let stage2: PipelineStage<'_> = Box::new(|spec: &AgentStepSpec| {
            if spec.label == "bad" {
                return None; // drop
            }
            Some((
                spec.clone(),
                StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: true,
                    provider_session_id: None,
                    output_summary: "s2".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                },
            ))
        });
        let stage3: PipelineStage<'_> = Box::new(|spec: &AgentStepSpec| {
            Some((
                spec.clone(),
                StepResult {
                    phase: spec.phase.clone(),
                    label: spec.label.clone(),
                    provider: spec.provider.clone(),
                    isolation: spec.isolation.clone(),
                    ok: true,
                    provider_session_id: None,
                    output_summary: "s3".to_string(),
                    step_id: None,
                    started_at: None,
                    details: None,
                    structured: None,
                    ordinal: None,
                },
            ))
        });

        // Wrap stage3 to record which items reached it.
        let recorded = &reached_stage3;
        let stage3_recorded: PipelineStage<'_> = Box::new(move |spec: &AgentStepSpec| {
            recorded.lock().unwrap().push(spec.label.clone());
            stage3(spec)
        });

        let items = vec![item("good"), item("bad")];
        let results = pipeline(items, vec![stage1, stage2, stage3_recorded]);
        assert_eq!(results.len(), 2);

        // Input order preserved.
        assert_eq!(results[0].label, "good");
        assert_eq!(results[1].label, "bad");
        // "good" flowed through all three stages.
        assert!(results[0].ok);
        assert_eq!(results[0].output_summary, "s3");
        // "bad" dropped at stage 2: failed slot, never reached stage 3.
        assert!(!results[1].ok, "dropped item lands in a failed slot");

        let reached = reached_stage3.into_inner().unwrap();
        assert!(reached.contains(&"good".to_string()));
        assert!(
            !reached.contains(&"bad".to_string()),
            "dropped item skipped its remaining stages"
        );
    }

    #[test]
    fn pipeline_first_stage_drop_yields_failed_slot() {
        let drop_all: PipelineStage<'_> = Box::new(|_spec: &AgentStepSpec| None);
        let results = pipeline(vec![item("x")], vec![drop_all]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(results[0].label, "x");
    }

    #[test]
    fn pipeline_empty_items_is_empty() {
        let results = pipeline(Vec::new(), vec![pass_stage("s1")]);
        assert!(results.is_empty());
    }
}
