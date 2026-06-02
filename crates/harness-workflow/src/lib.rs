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
//! * The `parallel()` barrier primitive + a `pipeline()` stub (real streaming is
//!   deferred to Stage 2).
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

/// One member's role in a workflow run. The workflow names MEMBERS, not
/// providers (provider-neutrality is enforced by the delivery seam, not here).
#[derive(Debug, Clone)]
pub struct WorkflowMembers {
    /// Member that plays the "codex" auditor role in the built-in scenario.
    pub codex_member_id: String,
    /// Member that plays the "claude" synthesist role in the built-in scenario.
    pub claude_member_id: String,
}

/// A single agent step to run: deliver `prompt` to `member_id`, grouped under
/// `phase` and named by `label`. This is the workflow-layer description of one
/// `agent()` call; the runtime turns it into a [`StepResult`].
#[derive(Debug, Clone)]
pub struct AgentStepSpec {
    pub phase: String,
    pub label: String,
    pub member_id: String,
    pub prompt: String,
}

/// The outcome of one agent step. `ok == false` is the CC-spec `null` slot: a
/// failed step never aborts the run; it is collected like any other result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub phase: String,
    pub label: String,
    pub member_id: String,
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
                        member_id: spec.member_id.clone(),
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
                    member_id: spec.member_id.clone(),
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

/// A streaming pipeline primitive (stub; real streaming is Stage 2).
/// For now it falls back to `parallel()` semantics. The actual per-item streaming
/// implementation with overlapping stage windows is implemented in Stage 2.
type PipelineStage = Box<dyn Fn(&AgentStepSpec) -> Option<StepResult> + Send + Sync>;

#[allow(dead_code)]
pub fn pipeline(
    driver: &AgentStepFn<'_>,
    items: Vec<AgentStepSpec>,
    _stages: Vec<PipelineStage>,
) -> Vec<StepResult> {
    // Stage 1 stub: run items through the parallel barrier.
    if items.is_empty() {
        return Vec::new();
    }

    parallel(driver, &items)
}

/// Outcome of a whole workflow run, returned to the caller for journaling.
#[derive(Debug, Clone)]
pub struct WorkflowOutcome {
    pub steps: Vec<StepResult>,
    pub status: WorkflowRunStatus,
    pub summary: String,
}

/// The built-in `investigate` workflow (the §6 scenario). Demonstrates BOTH
/// control-flow forms:
///   1. SERIAL  — a single codex delivery (`scope` phase), awaited before the
///      parallel fan-out, so its result is available to the next phase.
///   2. PARALLEL — a barrier fan-out of two deliveries (`audit` phase): one to
///      the codex member and one to the claude member, joined before returning.
///
/// The run is "completed" iff the serial (required) step succeeds; a failed
/// required step transitions the run to "failed" but the parallel steps are
/// still collected (nulls tolerated).
pub fn investigate(
    driver: &AgentStepFn<'_>,
    members: &WorkflowMembers,
    topic: &str,
) -> WorkflowOutcome {
    let mut steps = Vec::new();

    // --- SERIAL phase: scope the investigation with the codex member. ---
    let scope_step = run_agent_step(
        driver,
        &AgentStepSpec {
            phase: "scope".to_string(),
            label: "scope-question".to_string(),
            member_id: members.codex_member_id.clone(),
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
            member_id: members.codex_member_id.clone(),
            prompt: format!("Audit the code paths involved in: {topic}."),
        },
        AgentStepSpec {
            phase: "audit".to_string(),
            label: "audit-claude".to_string(),
            member_id: members.claude_member_id.clone(),
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
    }
}

/// Signature of a registered built-in workflow body (§3 option C).
pub type WorkflowFn = fn(&AgentStepFn<'_>, &WorkflowMembers, &str) -> WorkflowOutcome;

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
// phase / barrier / stream semantics. Member references are NAMES; the caller
// resolves them to harness member ids via a passed map so the runtime stays
// provider-agnostic.
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
    /// A single `agent()` delivery: send `prompt` to `member`.
    Agent {
        /// Member NAME (resolved to a harness member id by the caller).
        member: String,
        /// The prompt delivered to the member.
        prompt: String,
        /// Optional phase grouping (defaults to the label / "agent").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        /// Optional step label (defaults to the member name).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// A named phase whose children run SERIALLY, in order.
    Phase {
        name: String,
        nodes: Vec<WorkflowNode>,
    },
    /// A barrier fan-out: all children run concurrently and are joined before
    /// the interpreter proceeds.
    Parallel { nodes: Vec<WorkflowNode> },
    /// A streaming pipeline. Stage 1 falls back to `parallel()` semantics;
    /// real per-item streaming arrives in Stage 2.
    Pipeline { stages: Vec<WorkflowNode> },
}

/// Resolve a member NAME (as written in a spec) to a harness member id.
pub type MemberResolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Errors the IR interpreter can raise before any delivery runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// A `member` name in the spec did not resolve to a harness member id.
    UnknownMember(String),
    /// A `Parallel` / `Pipeline` block nested a non-`Agent` child, which the
    /// Stage 1 interpreter does not support (barriers are flat fan-outs).
    NonAgentInBarrier(&'static str),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::UnknownMember(name) => {
                write!(f, "unknown member in workflow spec: {name}")
            }
            DispatchError::NonAgentInBarrier(kind) => {
                write!(f, "{kind} blocks may only contain agent nodes (Stage 1)")
            }
        }
    }
}

impl std::error::Error for DispatchError {}

/// Build an [`AgentStepSpec`] from an `Agent` node, resolving its member name.
fn agent_spec(
    resolver: &MemberResolver<'_>,
    default_phase: &str,
    member: &str,
    prompt: &str,
    phase: &Option<String>,
    label: &Option<String>,
) -> Result<AgentStepSpec, DispatchError> {
    let member_id =
        resolver(member).ok_or_else(|| DispatchError::UnknownMember(member.to_string()))?;
    let label = label.clone().unwrap_or_else(|| member.to_string());
    let phase = phase.clone().unwrap_or_else(|| default_phase.to_string());
    Ok(AgentStepSpec {
        phase,
        label,
        member_id,
        prompt: prompt.to_string(),
    })
}

/// Collect the flat list of `Agent` specs in a barrier block (`Parallel` /
/// `Pipeline`). Non-`Agent` children are rejected — Stage 1 barriers are flat.
fn barrier_specs(
    resolver: &MemberResolver<'_>,
    default_phase: &str,
    nodes: &[WorkflowNode],
    kind: &'static str,
) -> Result<Vec<AgentStepSpec>, DispatchError> {
    let mut specs = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            WorkflowNode::Agent {
                member,
                prompt,
                phase,
                label,
            } => {
                specs.push(agent_spec(
                    resolver,
                    default_phase,
                    member,
                    prompt,
                    phase,
                    label,
                )?);
            }
            _ => return Err(DispatchError::NonAgentInBarrier(kind)),
        }
    }
    Ok(specs)
}

/// Recursively walk one node, appending its [`StepResult`]s to `steps`.
fn walk_node(
    driver: &AgentStepFn<'_>,
    resolver: &MemberResolver<'_>,
    default_phase: &str,
    node: &WorkflowNode,
    steps: &mut Vec<StepResult>,
) -> Result<(), DispatchError> {
    match node {
        WorkflowNode::Agent {
            member,
            prompt,
            phase,
            label,
        } => {
            let spec = agent_spec(resolver, default_phase, member, prompt, phase, label)?;
            steps.push(run_agent_step(driver, &spec));
        }
        WorkflowNode::Phase { name, nodes } => {
            // Serial: each child fully completes before the next begins.
            for child in nodes {
                walk_node(driver, resolver, name, child, steps)?;
            }
        }
        WorkflowNode::Parallel { nodes } => {
            let specs = barrier_specs(resolver, default_phase, nodes, "parallel")?;
            steps.extend(parallel(driver, &specs));
        }
        WorkflowNode::Pipeline { stages } => {
            // Stage 1: fall back to the parallel barrier (real streaming = Stage 2).
            let specs = barrier_specs(resolver, default_phase, stages, "pipeline")?;
            steps.extend(parallel(driver, &specs));
        }
    }
    Ok(())
}

/// Interpret a [`WorkflowSpec`] IR, running its nodes through the runtime
/// primitives and collecting every [`StepResult`]. Top-level nodes run serially
/// in order; `Phase` is serial, `Parallel` is a barrier, `Pipeline` falls back
/// to a barrier in Stage 1.
///
/// The run is "completed" unless it has no successful step (and at least one
/// step ran), in which case it is "failed". A spec with zero steps completes
/// vacuously.
pub fn dispatch_spec(
    spec: &WorkflowSpec,
    resolver: &MemberResolver<'_>,
    driver: &AgentStepFn<'_>,
) -> Result<WorkflowOutcome, DispatchError> {
    let mut steps = Vec::new();
    for node in &spec.nodes {
        walk_node(driver, resolver, &spec.name, node, &mut steps)?;
    }

    let total = steps.len();
    let kept = steps.iter().filter(|step| step.ok).count();
    let (status, summary) = if total > 0 && kept == 0 {
        (
            WorkflowRunStatus::Failed,
            format!("{} failed: 0/{total} steps ok", spec.name),
        )
    } else {
        (
            WorkflowRunStatus::Completed,
            format!("{} completed: {kept}/{total} steps ok", spec.name),
        )
    };

    Ok(WorkflowOutcome {
        steps,
        status,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn members() -> WorkflowMembers {
        WorkflowMembers {
            codex_member_id: "member-codex".to_string(),
            claude_member_id: "member-claude".to_string(),
        }
    }

    /// A mock driver that always succeeds and records the order of invocation.
    fn recording_driver<'a>(
        order: &'a Mutex<Vec<String>>,
    ) -> impl Fn(&AgentStepSpec) -> StepResult + Sync + 'a {
        move |spec: &AgentStepSpec| {
            order.lock().unwrap().push(spec.label.clone());
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                member_id: spec.member_id.clone(),
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
            investigate(&driver, &members(), "failure X")
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
                member_id: spec.member_id.clone(),
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
                member_id: "m".to_string(),
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
                    member_id: spec.member_id.clone(),
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
                member_id: spec.member_id.clone(),
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
                member_id: "m".to_string(),
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
                member_id: spec.member_id.clone(),
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
        let outcome = investigate(&driver, &members(), "failure Y");
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
        let outcome = (def.run)(&driver, &members(), "failure Z");
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
                member_id: "m".to_string(),
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

    /// Resolver that maps spec member NAMES to harness member ids.
    fn name_resolver(map: &BTreeMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
        move |name: &str| map.get(name).cloned()
    }

    fn sample_map() -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("auditor".to_string(), "member-codex".to_string());
        map.insert("synthesist".to_string(), "member-claude".to_string());
        map
    }

    #[test]
    fn workflow_spec_round_trips_json() {
        let json = r#"{
          "name": "demo",
          "args": { "topic": "x" },
          "nodes": [
            { "type": "phase", "name": "scope", "nodes": [
              { "type": "agent", "member": "auditor", "prompt": "scope it" }
            ]},
            { "type": "parallel", "nodes": [
              { "type": "agent", "member": "auditor", "prompt": "audit code" },
              { "type": "agent", "member": "synthesist", "prompt": "audit diffs" }
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
        let map = sample_map();
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![
                WorkflowNode::Phase {
                    name: "scope".to_string(),
                    nodes: vec![WorkflowNode::Agent {
                        member: "auditor".to_string(),
                        prompt: "scope it".to_string(),
                        phase: None,
                        label: Some("scope-question".to_string()),
                    }],
                },
                WorkflowNode::Parallel {
                    nodes: vec![
                        WorkflowNode::Agent {
                            member: "auditor".to_string(),
                            prompt: "audit code".to_string(),
                            phase: Some("audit".to_string()),
                            label: Some("audit-codex".to_string()),
                        },
                        WorkflowNode::Agent {
                            member: "synthesist".to_string(),
                            prompt: "audit diffs".to_string(),
                            phase: Some("audit".to_string()),
                            label: Some("audit-claude".to_string()),
                        },
                    ],
                },
            ],
        };
        let outcome = {
            let driver = recording_driver(&order);
            let resolver = name_resolver(&map);
            dispatch_spec(&spec, &resolver, &driver).expect("dispatch ok")
        };
        let order = order.into_inner().unwrap();
        // Serial scope node completes BEFORE the barrier fans out.
        assert_eq!(order[0], "scope-question");
        assert!(order.contains(&"audit-codex".to_string()));
        assert!(order.contains(&"audit-claude".to_string()));
        assert_eq!(outcome.steps.len(), 3);
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
        // The barrier resolved both members to the right harness ids.
        assert_eq!(outcome.steps[0].member_id, "member-codex");
    }

    #[test]
    fn dispatch_spec_failed_node_keeps_parallel_siblings() {
        let map = sample_map();
        let driver = |spec: &AgentStepSpec| {
            let ok = spec.label != "audit-codex";
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                member_id: spec.member_id.clone(),
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
                        member: "auditor".to_string(),
                        prompt: "audit code".to_string(),
                        phase: Some("audit".to_string()),
                        label: Some("audit-codex".to_string()),
                    },
                    WorkflowNode::Agent {
                        member: "synthesist".to_string(),
                        prompt: "audit diffs".to_string(),
                        phase: Some("audit".to_string()),
                        label: Some("audit-claude".to_string()),
                    },
                ],
            }],
        };
        let resolver = name_resolver(&map);
        let outcome = dispatch_spec(&spec, &resolver, &driver).expect("dispatch ok");
        // Both siblings collected even though one failed.
        assert_eq!(outcome.steps.len(), 2);
        assert!(!outcome.steps[0].ok);
        assert!(outcome.steps[1].ok);
        // The run still completes (one step ok), not failed.
        assert_eq!(outcome.status, WorkflowRunStatus::Completed);
    }

    #[test]
    fn dispatch_spec_rejects_unknown_member() {
        let map = sample_map();
        let order = Mutex::new(Vec::new());
        let driver = recording_driver(&order);
        let spec = WorkflowSpec {
            name: "demo".to_string(),
            args: None,
            nodes: vec![WorkflowNode::Agent {
                member: "ghost".to_string(),
                prompt: "hi".to_string(),
                phase: None,
                label: None,
            }],
        };
        let resolver = name_resolver(&map);
        let err = dispatch_spec(&spec, &resolver, &driver).expect_err("unknown member rejected");
        assert_eq!(err, DispatchError::UnknownMember("ghost".to_string()));
    }
}
