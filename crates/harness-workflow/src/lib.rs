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
//! * A runtime JSON-IR ([`WorkflowSpec`] / [`WorkflowNode`]) and a
//!   [`dispatch_spec`] interpreter that walks the IR so an agent can author the
//!   workflow SHAPE at runtime without a compiled registry entry.
//!
//! See docs/research/dynamic-workflow-runtime-design.md for the full design.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};

use harness_core::{WorkflowRunStatus, WorkflowStepStatus};
use serde::{Deserialize, Serialize};

pub mod starlark_front;

/// The set of providers a workflow node may target. Each `agent()` node spins up
/// a NEW one-shot ephemeral provider process (codex exec / claude -p); the node
/// references a PROVIDER, not a pre-existing member. The runtime stays
/// provider-agnostic — it only carries the validated provider string through to
/// the injected delivery driver.
pub const SUPPORTED_PROVIDERS: [&str; 2] = ["codex", "claude"];

/// The only supported per-node isolation mode. An `agent()` node may opt in to
/// `isolation: "worktree"` (exactly like Claude Code's Workflow `isolation:
/// 'worktree'`): that node runs in its own throwaway git worktree whose diff is
/// the node's evidence. The worktree is auto-removed if unchanged and is NOT
/// auto-merged back. Absent isolation, the node edits the shared repo cwd.
pub const ISOLATION_WORKTREE: &str = "worktree";

/// A single agent step to run: spin up an ephemeral `provider` worker, deliver
/// `prompt`, grouped under `phase` and named by `label`. This is the
/// workflow-layer description of one `agent()` call; the runtime turns it into a
/// [`StepResult`]. `model` overrides the provider's default model; `isolation`
/// opts the node into a throwaway git worktree.
#[derive(Debug, Clone)]
pub struct AgentStepSpec {
    pub phase: String,
    pub label: String,
    /// The provider that runs this step ("codex" | "claude"). Each step spins up
    /// a fresh ephemeral worker; the runtime passes this through to the driver.
    pub provider: String,
    /// Optional model override; `None` uses the provider default.
    pub model: Option<String>,
    /// Optional per-node isolation. `Some("worktree")` runs the step in its own
    /// throwaway git worktree; `None` edits the shared repo cwd.
    pub isolation: Option<String>,
    pub prompt: String,
}

/// The outcome of one agent step. `ok == false` is the CC-spec `null` slot: a
/// failed step never aborts the run; it is collected like any other result.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            isolation: None,
            prompt: format!("Scope the investigation of: {topic}. List the modules to audit."),
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
            isolation: None,
            prompt: format!("Audit the code paths involved in: {topic}."),
        },
        AgentStepSpec {
            phase: "audit".to_string(),
            label: "audit-claude".to_string(),
            provider: "claude".to_string(),
            model: None,
            isolation: None,
            prompt: format!("Audit the recent diffs related to: {topic}."),
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

// ===========================================================================
// JSON-IR: a runtime-authored workflow SHAPE.
//
// An agent (Codex / Claude / other) writes a `WorkflowSpec` JSON document and
// the CLI feeds it through `dispatch_spec`, which walks the node tree applying
// phase / barrier / stream semantics. Each `agent()` node references a PROVIDER
// ("codex" | "claude") directly: the runtime spins up a NEW one-shot ephemeral
// worker per node and passes the validated provider (+ optional model /
// isolation) straight through to the injected delivery driver. There is no
// member-name resolution step — the runtime stays provider-agnostic.
// ===========================================================================

/// A runtime-authored workflow specification (the JSON-IR root).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSpec {
    /// Human-facing workflow name (journaled as the run's `workflow_name`).
    pub name: String,
    /// Optional JSON parameterization for the run. Carried opaquely in Stage 1;
    /// flowed into node prompts in Stage 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// The ordered node tree. Top-level nodes run serially, in order.
    pub nodes: Vec<WorkflowNode>,
}

/// One node in the IR tree. The variant decides the control-flow semantics the
/// interpreter applies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowNode {
    /// A single `agent()` delivery: spin up a NEW one-shot ephemeral `provider`
    /// worker and deliver `prompt`. The worker CAN EDIT files (full sandbox) and
    /// by default shares the repo cwd with sibling nodes; opt into
    /// `isolation: "worktree"` to run it in a throwaway git worktree instead.
    Agent {
        /// The provider that runs this node ("codex" | "claude"). Validated
        /// against [`SUPPORTED_PROVIDERS`] before any delivery.
        provider: String,
        /// The prompt delivered to the ephemeral worker.
        prompt: String,
        /// Optional phase grouping (defaults to the enclosing phase / spec name).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        /// Optional step label (defaults to the provider name).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// Optional model override; absent uses the provider default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Optional per-node isolation. `Some("worktree")` runs the node in its
        /// own throwaway git worktree; absent edits the shared repo cwd.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        isolation: Option<String>,
    },
    /// A named phase whose children run SERIALLY, in order.
    Phase {
        name: String,
        nodes: Vec<WorkflowNode>,
    },
    /// A barrier fan-out: all children run concurrently and are joined before
    /// the interpreter proceeds.
    Parallel { nodes: Vec<WorkflowNode> },
    /// A streaming pipeline: the item flows through each stage agent in order
    /// with NO barrier; a stage that fails drops the item and skips the rest.
    Pipeline { stages: Vec<WorkflowNode> },
}

/// Errors the IR interpreter can raise before any delivery runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// A `provider` in the spec is not one of [`SUPPORTED_PROVIDERS`].
    UnknownProvider(String),
    /// An `isolation` value in the spec is not [`ISOLATION_WORKTREE`].
    UnknownIsolation(String),
    /// A `Parallel` / `Pipeline` block nested a non-`Agent` child, which the
    /// Stage 1 interpreter does not support (barriers are flat fan-outs).
    NonAgentInBarrier(&'static str),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::UnknownProvider(provider) => {
                write!(
                    f,
                    "unknown provider in workflow spec: {provider} (expected one of {:?})",
                    SUPPORTED_PROVIDERS
                )
            }
            DispatchError::UnknownIsolation(isolation) => {
                write!(
                    f,
                    "unknown isolation in workflow spec: {isolation} (expected \"{ISOLATION_WORKTREE}\")"
                )
            }
            DispatchError::NonAgentInBarrier(kind) => {
                write!(f, "{kind} blocks may only contain agent nodes (Stage 1)")
            }
        }
    }
}

impl std::error::Error for DispatchError {}

/// Substitute `{{key}}` placeholders in a prompt with the spec's `args`. Each
/// top-level key of the `args` object is a placeholder; scalar values render
/// without quotes (a string as-is, numbers/bools via their JSON text), nested
/// values via compact JSON. Unknown placeholders are left untouched. With no
/// `args`, the prompt is returned verbatim. This is how a spec parameterizes its
/// node prompts (e.g. `"Audit {{topic}}"` + `args: { "topic": "auth" }`).
fn interpolate_args(prompt: &str, args: Option<&serde_json::Value>) -> String {
    let Some(serde_json::Value::Object(map)) = args else {
        return prompt.to_string();
    };
    let mut out = prompt.to_string();
    for (key, value) in map {
        let rendered = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out = out.replace(&format!("{{{{{key}}}}}"), &rendered);
    }
    out
}

/// Validate a provider string against [`SUPPORTED_PROVIDERS`].
fn validate_provider(provider: &str) -> Result<(), DispatchError> {
    if SUPPORTED_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(DispatchError::UnknownProvider(provider.to_string()))
    }
}

/// Validate an optional isolation value against [`ISOLATION_WORKTREE`].
fn validate_isolation(isolation: &Option<String>) -> Result<(), DispatchError> {
    match isolation.as_deref() {
        None | Some(ISOLATION_WORKTREE) => Ok(()),
        Some(other) => Err(DispatchError::UnknownIsolation(other.to_string())),
    }
}

/// Build an [`AgentStepSpec`] from an `Agent` node, validating its provider /
/// isolation and interpolating any `{{key}}` placeholders in the prompt from the
/// spec `args`. The node carries the provider directly — there is no member
/// resolution step.
#[allow(clippy::too_many_arguments)]
fn agent_spec(
    args: Option<&serde_json::Value>,
    default_phase: &str,
    provider: &str,
    prompt: &str,
    phase: &Option<String>,
    label: &Option<String>,
    model: &Option<String>,
    isolation: &Option<String>,
) -> Result<AgentStepSpec, DispatchError> {
    validate_provider(provider)?;
    validate_isolation(isolation)?;
    let label = label.clone().unwrap_or_else(|| provider.to_string());
    let phase = phase.clone().unwrap_or_else(|| default_phase.to_string());
    Ok(AgentStepSpec {
        phase,
        label,
        provider: provider.to_string(),
        model: model.clone(),
        isolation: isolation.clone(),
        prompt: interpolate_args(prompt, args),
    })
}

/// Collect the flat list of `Agent` specs in a barrier block (`Parallel` /
/// `Pipeline`). Non-`Agent` children are rejected — Stage 1 barriers are flat.
fn barrier_specs(
    args: Option<&serde_json::Value>,
    default_phase: &str,
    nodes: &[WorkflowNode],
    kind: &'static str,
) -> Result<Vec<AgentStepSpec>, DispatchError> {
    let mut specs = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            WorkflowNode::Agent {
                provider,
                prompt,
                phase,
                label,
                model,
                isolation,
            } => {
                specs.push(agent_spec(
                    args,
                    default_phase,
                    provider,
                    prompt,
                    phase,
                    label,
                    model,
                    isolation,
                )?);
            }
            _ => return Err(DispatchError::NonAgentInBarrier(kind)),
        }
    }
    Ok(specs)
}

/// Recursively walk one node, appending its [`StepResult`]s to `steps`. `args`
/// is the spec's parameterization, flowed into every node prompt.
fn walk_node(
    driver: &AgentStepFn<'_>,
    args: Option<&serde_json::Value>,
    default_phase: &str,
    node: &WorkflowNode,
    steps: &mut Vec<StepResult>,
) -> Result<(), DispatchError> {
    match node {
        WorkflowNode::Agent {
            provider,
            prompt,
            phase,
            label,
            model,
            isolation,
        } => {
            let spec = agent_spec(
                args,
                default_phase,
                provider,
                prompt,
                phase,
                label,
                model,
                isolation,
            )?;
            steps.push(run_agent_step(driver, &spec));
        }
        WorkflowNode::Phase { name, nodes } => {
            // Serial: each child fully completes before the next begins.
            for child in nodes {
                walk_node(driver, args, name, child, steps)?;
            }
        }
        WorkflowNode::Parallel { nodes } => {
            let specs = barrier_specs(args, default_phase, nodes, "parallel")?;
            steps.extend(parallel(driver, &specs));
        }
        WorkflowNode::Pipeline { stages } => {
            // A streaming chain: the item flows through each stage agent in order
            // with NO barrier; a stage that FAILS drops the item and skips its
            // remaining stages (the CC-spec failure-drop). We validate each
            // stage's provider up front (so an unknown provider is a pre-flight
            // error, like the barrier path), then deliver each stage in turn,
            // halting at the first failure. Every stage that ran is journaled.
            let stage_specs = barrier_specs(args, default_phase, stages, "pipeline")?;
            for spec in &stage_specs {
                let result = run_agent_step(driver, spec);
                let ok = result.ok;
                steps.push(result);
                if !ok {
                    // Drop: this stage failed, so the remaining stages are skipped.
                    break;
                }
            }
        }
    }
    Ok(())
}

/// One step's structured payload for `WorkflowRun.final_output` / the step's
/// `result` field. Mirrors the human-facing summary with the machine-facing
/// status + linkage the dashboard / callers want.
pub fn step_result_json(result: &StepResult) -> serde_json::Value {
    serde_json::json!({
        "phase": result.phase,
        "label": result.label,
        "provider": result.provider,
        "isolation": result.isolation,
        "ok": result.ok,
        "provider_session_id": result.provider_session_id,
        "output_summary": result.output_summary,
    })
}

/// Interpret a [`WorkflowSpec`] IR, running its nodes through the runtime
/// primitives and collecting every [`StepResult`]. Top-level nodes run serially
/// in order; `Phase` is serial, `Parallel` is a barrier, `Pipeline` is a
/// streaming chain (item flows through stages; a failed stage drops the rest).
///
/// The spec's `args` are interpolated into every node prompt (`{{key}}`). The
/// returned outcome carries `agents_spawned` (the scheduler counter delta across
/// this dispatch) and `final_output` (one JSON object per collected step).
///
/// The run is "completed" unless it has no successful step (and at least one
/// step ran), in which case it is "failed". A spec with zero steps completes
/// vacuously.
pub fn dispatch_spec(
    spec: &WorkflowSpec,
    driver: &AgentStepFn<'_>,
) -> Result<WorkflowOutcome, DispatchError> {
    // Snapshot the scheduler's lifetime spawn counter so we can attribute the
    // agents THIS run spawned (the delta) to `WorkflowRun.agents_spawned`.
    let spawned_before = scheduler_agents_spawned();

    let mut steps = Vec::new();
    let args = spec.args.as_ref();
    for node in &spec.nodes {
        walk_node(driver, args, &spec.name, node, &mut steps)?;
    }

    Ok(outcome_from_steps(&spec.name, steps, spawned_before))
}

/// Build a [`WorkflowOutcome`] from a run's accumulated steps. Shared by every
/// front-end that produces an ordered `Vec<StepResult>` — the JSON-IR
/// [`dispatch_spec`] walker and the Starlark [`starlark_front::run_starlark`]
/// evaluator — so all front-ends derive status / summary / final_output
/// identically. `spawned_before` is the scheduler's lifetime spawn-counter
/// snapshot taken before the run, so the delta attributes this run's agents.
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
            }
        }
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
            }
        };
        let specs: Vec<AgentStepSpec> = (0..5)
            .map(|i| AgentStepSpec {
                phase: "p".to_string(),
                label: format!("l{i}"),
                provider: "codex".to_string(),
                model: None,
                isolation: None,
                prompt: format!("prompt {i}"),
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
            }
        };
        let specs: Vec<AgentStepSpec> = (0..4)
            .map(|i| AgentStepSpec {
                phase: "p".to_string(),
                label: format!("l{i}"),
                provider: "codex".to_string(),
                model: None,
                isolation: None,
                prompt: "x".to_string(),
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
                isolation: None,
                prompt: format!("prompt {i}"),
            })
            .collect();
        let results = parallel(&driver, &specs);
        assert_eq!(results.len(), 10, "barrier collected all 10 results");
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.label, format!("l{i}"), "results in input order");
        }
    }

    // ----- JSON-IR + dispatch_spec tests -----

    #[test]
    fn workflow_spec_round_trips_json() {
        let json = r#"{
          "name": "demo",
          "args": { "topic": "x" },
          "nodes": [
            { "type": "phase", "name": "scope", "nodes": [
              { "type": "agent", "provider": "codex", "prompt": "scope it" }
            ]},
            { "type": "parallel", "nodes": [
              { "type": "agent", "provider": "codex", "prompt": "audit code", "isolation": "worktree" },
              { "type": "agent", "provider": "claude", "prompt": "audit diffs", "model": "opus" }
            ]}
          ]
        }"#;
        let spec: WorkflowSpec = serde_json::from_str(json).expect("parse spec");
        assert_eq!(spec.name, "demo");
        assert_eq!(spec.nodes.len(), 2);
        let reser = serde_json::to_value(&spec).expect("serialize");
        let again: WorkflowSpec = serde_json::from_value(reser).expect("round trip");
        assert_eq!(spec, again);
    }

    #[test]
    fn dispatch_spec_runs_serial_then_parallel_with_barrier() {
        let order = Mutex::new(Vec::new());
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![
                WorkflowNode::Phase {
                    name: "scope".to_string(),
                    nodes: vec![WorkflowNode::Agent {
                        provider: "codex".to_string(),
                        prompt: "scope it".to_string(),
                        phase: None,
                        label: Some("scope-question".to_string()),
                        model: None,
                        isolation: None,
                    }],
                },
                WorkflowNode::Parallel {
                    nodes: vec![
                        WorkflowNode::Agent {
                            provider: "codex".to_string(),
                            prompt: "audit code".to_string(),
                            phase: Some("audit".to_string()),
                            label: Some("audit-codex".to_string()),
                            model: None,
                            isolation: None,
                        },
                        WorkflowNode::Agent {
                            provider: "claude".to_string(),
                            prompt: "audit diffs".to_string(),
                            phase: Some("audit".to_string()),
                            label: Some("audit-claude".to_string()),
                            model: None,
                            isolation: None,
                        },
                    ],
                },
            ],
        };
        let outcome = {
            let driver = recording_driver(&order);
            dispatch_spec(&spec, &driver).expect("dispatch ok")
        };
        let order = order.into_inner().unwrap();
        // Serial scope node completes BEFORE the barrier fans out.
        assert_eq!(order[0], "scope-question");
        assert!(order.contains(&"audit-codex".to_string()));
        assert!(order.contains(&"audit-claude".to_string()));
        assert_eq!(outcome.steps.len(), 3);
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
        // The node carries its provider straight onto the step result.
        assert_eq!(outcome.steps[0].provider, "codex");
    }

    #[test]
    fn dispatch_spec_failed_node_keeps_parallel_siblings() {
        let driver = |spec: &AgentStepSpec| {
            let ok = spec.label != "audit-codex";
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok,
                provider_session_id: if ok { Some("s".to_string()) } else { None },
                output_summary: "x".to_string(),
                step_id: None,
                started_at: None,
            }
        };
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Parallel {
                nodes: vec![
                    WorkflowNode::Agent {
                        provider: "codex".to_string(),
                        prompt: "audit code".to_string(),
                        phase: Some("audit".to_string()),
                        label: Some("audit-codex".to_string()),
                        model: None,
                        isolation: None,
                    },
                    WorkflowNode::Agent {
                        provider: "claude".to_string(),
                        prompt: "audit diffs".to_string(),
                        phase: Some("audit".to_string()),
                        label: Some("audit-claude".to_string()),
                        model: None,
                        isolation: None,
                    },
                ],
            }],
        };
        let outcome = dispatch_spec(&spec, &driver).expect("dispatch ok");
        // Both siblings collected even though one failed.
        assert_eq!(outcome.steps.len(), 2);
        assert!(!outcome.steps[0].ok);
        assert!(outcome.steps[1].ok);
        // The run still completes (one step ok), not failed.
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
    }

    #[test]
    fn dispatch_spec_rejects_unknown_provider() {
        let order = Mutex::new(Vec::new());
        let driver = recording_driver(&order);
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Agent {
                provider: "ghost".to_string(),
                prompt: "hi".to_string(),
                phase: None,
                label: None,
                model: None,
                isolation: None,
            }],
        };
        let err = dispatch_spec(&spec, &driver).expect_err("unknown provider rejected");
        assert_eq!(err, DispatchError::UnknownProvider("ghost".to_string()));
    }

    #[test]
    fn dispatch_spec_rejects_unknown_isolation() {
        let order = Mutex::new(Vec::new());
        let driver = recording_driver(&order);
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Agent {
                provider: "codex".to_string(),
                prompt: "hi".to_string(),
                phase: None,
                label: None,
                model: None,
                isolation: Some("sandbox".to_string()),
            }],
        };
        let err = dispatch_spec(&spec, &driver).expect_err("unknown isolation rejected");
        assert_eq!(err, DispatchError::UnknownIsolation("sandbox".to_string()));
    }

    #[test]
    fn dispatch_spec_accepts_worktree_isolation_and_model() {
        let order = Mutex::new(Vec::new());
        let seen = Mutex::new(Vec::<(Option<String>, Option<String>)>::new());
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Agent {
                provider: "claude".to_string(),
                prompt: "fix it".to_string(),
                phase: None,
                label: Some("fixer".to_string()),
                model: Some("opus".to_string()),
                isolation: Some("worktree".to_string()),
            }],
        };
        let outcome = {
            let driver = |spec: &AgentStepSpec| {
                seen.lock()
                    .unwrap()
                    .push((spec.model.clone(), spec.isolation.clone()));
                order.lock().unwrap().push(spec.label.clone());
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
            dispatch_spec(&spec, &driver).expect("dispatch ok")
        };
        let seen = seen.into_inner().unwrap();
        assert_eq!(
            seen,
            vec![(Some("opus".to_string()), Some("worktree".to_string()))]
        );
        assert_eq!(outcome.steps[0].isolation.as_deref(), Some("worktree"));
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
            isolation: None,
            prompt: "x".to_string(),
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
                // Block until the fast item is observed in stage 2.
                let mut spins = 0;
                while !fast_flag.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                    spins += 1;
                    assert!(
                        spins < 50_000_000,
                        "no-barrier deadlock: fast item never reached stage 2"
                    );
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

    // ----- args interpolation + IR pipeline tests -----

    #[test]
    fn dispatch_spec_args_flow_into_node_prompts() {
        let seen = Mutex::new(Vec::<String>::new());
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: Some(serde_json::json!({ "topic": "auth bug", "n": 3 })),
            nodes: vec![WorkflowNode::Agent {
                provider: "codex".to_string(),
                prompt: "Audit {{topic}} ({{n}} modules)".to_string(),
                phase: None,
                label: None,
                model: None,
                isolation: None,
            }],
        };
        {
            let driver = |s: &AgentStepSpec| {
                seen.lock().unwrap().push(s.prompt.clone());
                StepResult {
                    phase: s.phase.clone(),
                    label: s.label.clone(),
                    provider: s.provider.clone(),
                    isolation: s.isolation.clone(),
                    ok: true,
                    provider_session_id: Some("s".to_string()),
                    output_summary: "ok".to_string(),
                    step_id: None,
                    started_at: None,
                }
            };
            dispatch_spec(&spec, &driver).expect("dispatch ok");
        }
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen, vec!["Audit auth bug (3 modules)".to_string()]);
    }

    #[test]
    fn dispatch_spec_pipeline_streams_stages_and_drops_on_failure() {
        let order = Mutex::new(Vec::<String>::new());
        // The middle stage fails, so the third stage must be skipped.
        let driver = |s: &AgentStepSpec| {
            order.lock().unwrap().push(s.label.clone());
            let ok = s.label != "stage-2";
            StepResult {
                phase: s.phase.clone(),
                label: s.label.clone(),
                provider: s.provider.clone(),
                isolation: s.isolation.clone(),
                ok,
                provider_session_id: if ok { Some("s".to_string()) } else { None },
                output_summary: "x".to_string(),
                step_id: None,
                started_at: None,
            }
        };
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Pipeline {
                stages: vec![
                    WorkflowNode::Agent {
                        provider: "codex".to_string(),
                        prompt: "1".to_string(),
                        phase: Some("pipe".to_string()),
                        label: Some("stage-1".to_string()),
                        model: None,
                        isolation: None,
                    },
                    WorkflowNode::Agent {
                        provider: "codex".to_string(),
                        prompt: "2".to_string(),
                        phase: Some("pipe".to_string()),
                        label: Some("stage-2".to_string()),
                        model: None,
                        isolation: None,
                    },
                    WorkflowNode::Agent {
                        provider: "codex".to_string(),
                        prompt: "3".to_string(),
                        phase: Some("pipe".to_string()),
                        label: Some("stage-3".to_string()),
                        model: None,
                        isolation: None,
                    },
                ],
            }],
        };
        let outcome = dispatch_spec(&spec, &driver).expect("dispatch ok");
        let order = order.into_inner().unwrap();
        // Stage 3 is skipped because stage 2 dropped the item.
        assert_eq!(order, vec!["stage-1", "stage-2"]);
        assert_eq!(outcome.steps.len(), 2);
        assert!(outcome.steps[0].ok);
        assert!(!outcome.steps[1].ok);
        // final_output carries one entry per collected step.
        let out = outcome.final_output.expect("final_output present");
        assert_eq!(out.as_array().expect("array").len(), 2);
    }
}
