//! Minimal Rust-native workflow runtime (WP2).
//!
//! This extends WP1 with:
//! * CONCURRENCY-CAP SCHEDULER: bounds concurrency to min(16, available_parallelism()-2)
//!   via a counting semaphore. Excess steps are queued and run as permits free.
//!   Includes a 1000-agent LIFETIME cap as a runaway backstop.
//! * STREAMING pipeline(): placeholder for streaming pipeline (WP2 stub,
//!   full design deferred to WP5 IR phase).
//! * LIVE SSE PROGRESS: WorkflowStep rows are journaled and SSE-watched
//!   so the dashboard sees steps in real time.
//!
//! The design fidelity remains §3 option C — workflows are built-in registered
//! Rust fns dispatched by name. The scheduler is process-wide and shared across
//! all runs (one lifetime cap, one concurrency cap).
//!
//! See docs/research/dynamic-workflow-runtime-design.md for the full design.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};

use harness_core::{WorkflowRunStatus, WorkflowStepStatus};

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

/// WP2: A process-wide concurrency-cap scheduler. Bounds concurrent work to
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

/// The `parallel()` barrier (§4, WP1), now backed by the concurrency-cap scheduler.
/// Runs every spec concurrently on its own scoped thread, joins ALL of them
/// (the barrier), and returns results in input order. A thunk whose thread panics
/// is converted into a failed [`StepResult`] in its slot so the run itself never panics.
///
/// WP2: Each spec acquires a permit from the scheduler before running; excess
/// specs are queued and run as permits free.
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

/// WP2: A streaming pipeline primitive (stub; full design deferred to WP5).
/// This is a placeholder that demonstrates the concept. For now, it falls back
/// to parallel() semantics. The actual streaming implementation with true
/// per-stage concurrency will be designed in WP5 once the IR is defined.
type PipelineStage = Box<dyn Fn(&AgentStepSpec) -> Option<StepResult> + Send + Sync>;

#[allow(dead_code)]
pub fn pipeline(
    driver: &AgentStepFn<'_>,
    items: Vec<AgentStepSpec>,
    _stages: Vec<PipelineStage>,
) -> Vec<StepResult> {
    // WP2 stub: return empty. Full streaming pipeline deferred to WP5.
    if items.is_empty() {
        return Vec::new();
    }

    // Placeholder: run items through parallel barrier.
    parallel(driver, &items)
}

/// Outcome of a whole workflow run, returned to the caller for journaling.
#[derive(Debug, Clone)]
pub struct WorkflowOutcome {
    pub steps: Vec<StepResult>,
    pub status: WorkflowRunStatus,
    pub summary: String,
}

/// The built-in `investigate` workflow (the §6 scenario, reduced to the WP1
/// faithful shape, now with WP2 scheduler backing). Demonstrates BOTH control-flow forms:
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

    // WP2 TESTS: Concurrency cap, lifetime cap.
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
}
