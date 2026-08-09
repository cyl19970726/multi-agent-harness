//! Host-attention scheduling for provider-native Host bindings.
//!
//! This module deliberately stops at scheduling. A foreground TeamRun
//! supervisor has no provider-neutral driver for the Host session, so merely
//! noticing ready attention must never claim it. The leased claim seam below
//! is reserved for a caller that supplies both an exact Dispatcher lease and
//! an execution consumer.

use harness_core::{
    HostAttention, HostAttentionKind, HostBindingLease, HostBindingLeaseOwnerKind,
    HostDispatchConfig, TeamRunEventSourceKind,
};
use harness_store::{HarnessStore, StoreError};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

/// Build the bounded, triage-only turn delivered to the exact bound Host
/// session.  The provider transport is supplied by the CLI adapter layer; this
/// module owns only the permission contract and the durable attention facts.
pub fn build_headless_host_prompt(
    team_run_id: &str,
    objective: &str,
    attentions: &[HostAttention],
) -> String {
    let mut prompt = format!(
        "You are the headless triage Host for TeamRun {team_run_id}.\n\
         Objective: {objective}\n\n\
         This is a READ-ONLY TRIAGE turn. Inspect the durable facts below, run \
         read-only verification, reply or request clarification through Message \
         commands when useful, and leave terminal decisions for the interactive \
         Host. You MUST NOT accept, merge, cancel, close, reassign, or otherwise \
         mutate Work lifecycle state.\n\nPending Host attentions:\n"
    );
    for attention in attentions {
        prompt.push_str(&format!(
            "- id={} kind={:?} work_id={} work_version={} member_run_id={} source_event_ref={} attempt={}\n",
            attention.id,
            attention.kind,
            attention.work_id,
            attention.work_version,
            attention.member_run_id.as_deref().unwrap_or("-"),
            attention.source_event_ref,
            attention.attempt,
        ));
    }
    prompt.push_str(
        "\nEnd with a concise triage report describing what you verified, what \
         message you sent, and which decision still requires the interactive Host.",
    );
    prompt
}

/// Typed result of inspecting one TeamRun for Host work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleDecision {
    /// A live provider-native interactive Host owns the exact binding.
    SkipLiveInteractive {
        team_run_id: String,
        owner_id: String,
        lease_id: String,
        generation: u64,
        expires_unix_ms: u64,
    },
    /// There is work a future dispatcher may execute. This is an observation,
    /// not a claim and not evidence that any provider task was awakened.
    DispatchReady {
        team_run_id: String,
        attention_ids: Vec<String>,
        stale_attention_ids: Vec<String>,
    },
    /// The TeamRun does not name an exact provider-native Host task.
    NoBinding { team_run_id: String },
    /// Nothing is old enough yet; poll again later.
    Retry { team_run_id: String, reason: String },
    /// A Dispatcher lease exists, but this scheduling-only caller cannot use
    /// it without the exact lease and an execution consumer.
    Conflict { team_run_id: String, reason: String },
}

/// Log-facing compatibility summary for the foreground supervisor. `ready`
/// means only that scheduling found eligible work; it never means a Host task
/// was awakened or an attention was claimed.
#[derive(Debug)]
pub struct DispatchOutcome {
    pub inspected: usize,
    #[allow(dead_code)]
    pub handled: Vec<String>,
    #[allow(dead_code)]
    pub escalated: Vec<String>,
    pub failed: Vec<String>,
    pub ready: Vec<String>,
    pub stale: Vec<String>,
}

/// Foreground-supervisor compatibility seam. The supervisor does not own a
/// headless Host driver, so this function only schedules and reports.
pub fn poll_and_dispatch(
    store: &HarnessStore,
    ledger: &std::sync::Arc<crate::TeamRunLedger>,
    _objective: &str,
    config: &HostDispatchConfig,
) -> Result<DispatchOutcome, crate::CliError> {
    let decision = schedule_team_run(
        store,
        &ledger.run_id,
        config,
        crate::current_unix_ms_u64(),
        &crate::now_string(),
    )?;
    let mut outcome = DispatchOutcome {
        inspected: 0,
        handled: vec![],
        escalated: vec![],
        failed: vec![],
        ready: vec![],
        stale: vec![],
    };
    match decision {
        ScheduleDecision::DispatchReady {
            attention_ids,
            stale_attention_ids,
            ..
        } => {
            outcome.inspected = attention_ids.len();
            let current = store
                .host_attentions()?
                .into_iter()
                .map(|attention| (attention.id.clone(), attention))
                .collect::<std::collections::HashMap<_, _>>();
            for attention_id in &attention_ids {
                let attention = current.get(attention_id).ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "eligible HostAttention disappeared: {attention_id}"
                    ))
                })?;
                ledger.fold_event_once(
                    &format!(
                        "host-dispatch-ready:{}:{}:{:?}:{}",
                        ledger.run_id, attention.id, attention.status, attention.attempt
                    ),
                    TeamRunEventSourceKind::Host,
                    None,
                    "host_attention",
                    &attention.id,
                    "dispatch_ready",
                    &format!(
                        "HostAttention {} is dispatch-ready in status {:?}, attempt {}",
                        attention.id, attention.status, attention.attempt
                    ),
                )?;
            }
            outcome.ready = attention_ids;
            outcome.stale = stale_attention_ids;
        }
        ScheduleDecision::NoBinding { team_run_id } => {
            outcome.failed.push(format!(
                "TeamRun {team_run_id} has no exact Host binding; scheduler did not claim attention"
            ));
        }
        ScheduleDecision::Conflict { reason, .. } => outcome.failed.push(reason),
        ScheduleDecision::Retry { .. } | ScheduleDecision::SkipLiveInteractive { .. } => {}
    }
    Ok(outcome)
}

/// Inspect one TeamRun without claiming, completing, acknowledging, escalating,
/// or mutating any Work. Lease-stale attention materialization is the only
/// durable write performed here.
pub fn schedule_team_run(
    store: &HarnessStore,
    team_run_id: &str,
    config: &HostDispatchConfig,
    now_unix_ms: u64,
    observed_at: &str,
) -> Result<ScheduleDecision, StoreError> {
    let inbox = store.host_attention_inbox_for_team_run(team_run_id, true)?;
    if inbox.host_thread_id.is_none() {
        return Ok(ScheduleDecision::NoBinding {
            team_run_id: team_run_id.to_string(),
        });
    }

    if let Some(lease) = store.effective_host_binding_lease_at(team_run_id, now_unix_ms)? {
        return Ok(match lease.owner_kind {
            HostBindingLeaseOwnerKind::Interactive => ScheduleDecision::SkipLiveInteractive {
                team_run_id: team_run_id.to_string(),
                owner_id: lease.owner_id,
                lease_id: lease.lease_id,
                generation: lease.generation,
                expires_unix_ms: lease.expires_unix_ms,
            },
            HostBindingLeaseOwnerKind::Dispatcher => ScheduleDecision::Conflict {
                team_run_id: team_run_id.to_string(),
                reason: format!(
                    "Dispatcher lease {} generation {} is current; an exact leased execution consumer is required",
                    lease.lease_id, lease.generation
                ),
            },
        });
    }

    store.reconcile_host_binding_stale_attentions(now_unix_ms, observed_at)?;
    let cutoff =
        now_unix_ms.saturating_sub(config.attention_age_threshold_secs.saturating_mul(1_000));
    let eligible = store
        .actionable_attentions_older_than(cutoff)?
        .into_iter()
        .filter(|attention| attention.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    let stale_attention_ids = eligible
        .iter()
        .filter(|attention| attention.kind == HostAttentionKind::HostBindingStale)
        .map(|attention| attention.id.clone())
        .collect::<Vec<_>>();
    let attention_ids = eligible
        .into_iter()
        .map(|attention| attention.id)
        .collect::<Vec<_>>();

    if attention_ids.is_empty() {
        Ok(ScheduleDecision::Retry {
            team_run_id: team_run_id.to_string(),
            reason: "no aged actionable Host attention".to_string(),
        })
    } else {
        Ok(ScheduleDecision::DispatchReady {
            team_run_id: team_run_id.to_string(),
            attention_ids,
            stale_attention_ids,
        })
    }
}

/// Atomically claim a batch only for an exact, live Dispatcher lease and hand
/// it immediately to an execution consumer. If the consumer rejects the batch,
/// every claim is returned to `Actionable`, preventing stranded rows.
pub struct DispatcherBatchRequest<'a> {
    pub lease: &'a HostBindingLease,
    pub older_than_unix_ms: u64,
    pub limit: usize,
    pub claim_id: &'a str,
    pub now_unix_ms: u64,
    pub updated_at: &'a str,
}

/// A consumer may report success only together with the provider-native
/// receipt that durably proves delivery of the entire claimed batch.
pub struct DispatcherConsumerSuccess<T> {
    value: T,
    provider_receipt_id: String,
}

impl<T> DispatcherConsumerSuccess<T> {
    #[allow(dead_code)] // Public kernel constructor for the future headless driver (#415).
    pub fn new(value: T, provider_receipt_id: impl Into<String>) -> Result<Self, StoreError> {
        let provider_receipt_id = provider_receipt_id.into();
        if provider_receipt_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "dispatcher consumer provider receipt id must not be empty".to_string(),
            ));
        }
        Ok(Self {
            value,
            provider_receipt_id,
        })
    }
}

#[allow(dead_code)] // Kernel seam for the future headless driver (#415).
pub fn claim_dispatcher_batch_with_consumer<T, F>(
    store: &HarnessStore,
    request: DispatcherBatchRequest<'_>,
    consumer: F,
) -> Result<T, StoreError>
where
    F: FnOnce(&[HostAttention]) -> Result<DispatcherConsumerSuccess<T>, StoreError>,
{
    let DispatcherBatchRequest {
        lease,
        older_than_unix_ms,
        limit,
        claim_id,
        now_unix_ms,
        updated_at,
    } = request;
    if lease.owner_kind != HostBindingLeaseOwnerKind::Dispatcher {
        return Err(StoreError::Conflict(format!(
            "HOST_BINDING_INTERACTIVE_SUPPRESSES_DISPATCH: TeamRun {} lease is not Dispatcher-owned",
            lease.team_run_id
        )));
    }
    let claimed = store.claim_dispatcher_host_attention_batch(
        lease,
        older_than_unix_ms,
        limit.max(1),
        claim_id,
        now_unix_ms,
        updated_at,
    )?;
    if claimed.is_empty() {
        return Err(StoreError::Conflict(format!(
            "HOST_DISPATCH_NOTHING_CLAIMED: TeamRun {} has no eligible HostAttention after the dispatcher lease was acquired",
            lease.team_run_id
        )));
    }
    match catch_unwind(AssertUnwindSafe(|| consumer(&claimed))) {
        Ok(Ok(success)) => {
            let DispatcherConsumerSuccess {
                value,
                provider_receipt_id,
            } = success;
            // JSONL has no multi-row transaction. If a later append fails,
            // earlier Delivered rows remain durable and only the still-owned
            // suffix is returned to Actionable for a conservative retry.
            for (index, attention) in claimed.iter().enumerate() {
                if let Err(error) = store.complete_host_attention_claim(
                    &attention.id,
                    claim_id,
                    &provider_receipt_id,
                    updated_at,
                ) {
                    let cleanup = requeue_dispatcher_claims(
                        store,
                        &claimed[index..],
                        claim_id,
                        "dispatcher batch completion was only partially durable",
                        updated_at,
                    );
                    return Err(with_cleanup_error(error, cleanup));
                }
            }
            Ok(value)
        }
        Ok(Err(error)) => {
            let cleanup = requeue_dispatcher_claims(
                store,
                &claimed,
                claim_id,
                "dispatcher consumer rejected claimed batch",
                updated_at,
            );
            Err(with_cleanup_error(error, cleanup))
        }
        Err(payload) => {
            let _ = requeue_dispatcher_claims(
                store,
                &claimed,
                claim_id,
                "dispatcher consumer panicked",
                updated_at,
            );
            resume_unwind(payload)
        }
    }
}

fn requeue_dispatcher_claims(
    store: &HarnessStore,
    claimed: &[HostAttention],
    claim_id: &str,
    reason: &str,
    updated_at: &str,
) -> Result<(), StoreError> {
    let mut failures = Vec::new();
    for attention in claimed {
        if let Err(error) =
            store.fail_host_attention_claim(&attention.id, claim_id, reason, updated_at)
        {
            failures.push(format!("{}: {error}", attention.id));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(StoreError::Conflict(format!(
            "dispatcher could not requeue every claimed attention: {}",
            failures.join("; ")
        )))
    }
}

fn with_cleanup_error(error: StoreError, cleanup: Result<(), StoreError>) -> StoreError {
    match cleanup {
        Ok(()) => error,
        Err(cleanup_error) => {
            StoreError::Conflict(format!("{error}; cleanup also failed: {cleanup_error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::{AgentTeamRun, HostAttentionStatus, HostControlMode, TeamRunStatus};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (HarnessStore, PathBuf, AgentTeamRun) {
        let root = std::env::temp_dir().join(format!(
            "firm-host-dispatcher-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = HarnessStore::new(root.clone());
        store.init().expect("init");
        let run = AgentTeamRun {
            id: "run-1".into(),
            definition_id: None,
            agent_team_id: None,
            previous_run_id: None,
            mission_id: None,
            wave_id: None,
            project_binding_id: None,
            host_surface: "codex".into(),
            host_thread_id: Some("thread-1".into()),
            host_actor: None,
            host_control_mode: HostControlMode::External,
            objective: "test".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append run");
        (store, root, run)
    }

    #[test]
    fn headless_prompt_is_bounded_and_forbids_terminal_mutations() {
        let (_, root, run) = fixture();
        let attention = HostAttention {
            id: "attention-1".into(),
            team_run_id: run.id.clone(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: "work-1".into(),
            work_version: 3,
            source_event_ref: "work-event:3".into(),
            member_run_id: Some("member-1".into()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        let prompt = build_headless_host_prompt(&run.id, &run.objective, &[attention]);
        assert!(prompt.contains("READ-ONLY TRIAGE"));
        assert!(prompt.contains("MUST NOT accept, merge, cancel, close, reassign"));
        assert!(prompt.contains("id=attention-1"));
        assert!(prompt.contains("work_version=3"));
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn live_interactive_lease_suppresses_dispatch() {
        let (store, root, run) = fixture();
        store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Interactive,
                "host-1",
                "lease-1",
                100,
                100,
            )
            .expect("lease");
        let decision = schedule_team_run(
            &store,
            &run.id,
            &HostDispatchConfig::default(),
            150,
            "unix-ms:150",
        )
        .expect("schedule");
        assert!(matches!(
            decision,
            ScheduleDecision::SkipLiveInteractive { .. }
        ));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn new_stale_waits_for_cutoff_then_dispatches_once() {
        let (store, root, run) = fixture();
        let config = HostDispatchConfig::default();
        let first = schedule_team_run(&store, &run.id, &config, 100, "unix-ms:100")
            .expect("first schedule");
        assert!(matches!(first, ScheduleDecision::Retry { .. }));
        let decision = schedule_team_run(&store, &run.id, &config, 300_101, "unix-ms:300101")
            .expect("aged schedule");
        let ScheduleDecision::DispatchReady {
            attention_ids,
            stale_attention_ids,
            ..
        } = decision
        else {
            panic!("aged attention must be ready")
        };
        assert_eq!(attention_ids.len(), 1);
        assert_eq!(stale_attention_ids, attention_ids);
        let current = store
            .host_attentions()
            .expect("attentions")
            .into_iter()
            .find(|row| row.team_run_id == run.id)
            .expect("stale attention remains");
        assert_eq!(current.status, HostAttentionStatus::Actionable);
        assert!(current.claim_id.is_none());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn foreground_poll_reports_ready_without_stranding_attention() {
        let (store, root, run) = fixture();
        let ledger = std::sync::Arc::new(crate::TeamRunLedger::without_supervisor(&store, &run.id));
        store
            .reconcile_host_binding_stale_attentions(50, "unix-ms:50")
            .expect("seed stale attention");
        let config = HostDispatchConfig {
            attention_age_threshold_secs: 0,
            ..HostDispatchConfig::default()
        };
        let outcome = poll_and_dispatch(&store, &ledger, "test", &config).expect("poll");
        let repeated = poll_and_dispatch(&store, &ledger, "test", &config).expect("repeat poll");
        assert_eq!(outcome.inspected, 1, "stale subset is not double counted");
        assert_eq!(repeated.inspected, 1);
        assert!(!outcome.stale.is_empty());
        assert!(outcome.handled.is_empty());
        assert!(outcome.escalated.is_empty());
        let row = store
            .host_attentions()
            .expect("attentions")
            .into_iter()
            .find(|row| outcome.stale.iter().any(|id| id == &row.id))
            .expect("stale attention");
        assert_eq!(row.status, HostAttentionStatus::Actionable);
        assert!(row.claim_id.is_none());
        let events = store.team_run_events().expect("events");
        assert_eq!(events.len(), 1, "repeat poll reuses the durable event");
        assert_eq!(events[0].entity_id, row.id);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn stable_no_binding_and_dispatcher_conflict_do_not_emit_events() {
        let (store, root, run) = fixture();
        let ledger = std::sync::Arc::new(crate::TeamRunLedger::without_supervisor(&store, &run.id));
        let mut unbound = run.clone();
        unbound.host_thread_id = None;
        unbound.updated_at = "unix-ms:2".into();
        store
            .compare_and_append_team_run(&run, &unbound)
            .expect("unbind run");
        for _ in 0..2 {
            let outcome =
                poll_and_dispatch(&store, &ledger, "test", &HostDispatchConfig::default())
                    .expect("no binding is a stable observation");
            assert_eq!(outcome.inspected, 0);
        }
        assert!(store.team_run_events().unwrap().is_empty());
        let mut rebound = unbound.clone();
        rebound.host_thread_id = Some("thread-1".into());
        rebound.updated_at = "unix-ms:3".into();
        store
            .compare_and_append_team_run(&unbound, &rebound)
            .expect("rebind run");
        store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher",
                "lease-current",
                100,
                100,
            )
            .expect("dispatcher lease");
        for _ in 0..2 {
            let outcome =
                poll_and_dispatch(&store, &ledger, "test", &HostDispatchConfig::default())
                    .expect("conflict is a stable observation");
            assert_eq!(outcome.inspected, 0);
        }
        assert!(store.team_run_events().unwrap().is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn claim_requires_current_dispatcher_lease_and_consumer_failure_requeues() {
        let (store, root, run) = fixture();
        let attention_id = store
            .reconcile_host_binding_stale_attentions(50, "unix-ms:50")
            .expect("stale attention")
            .into_iter()
            .find(|row| row.team_run_id == run.id)
            .expect("run stale attention")
            .id;
        let old = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher-1",
                "lease-old",
                100,
                10,
            )
            .expect("old lease");
        let current = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher-2",
                "lease-current",
                111,
                100,
            )
            .expect("takeover");
        let stale = claim_dispatcher_batch_with_consumer(
            &store,
            DispatcherBatchRequest {
                lease: &old,
                older_than_unix_ms: 100,
                limit: 10,
                claim_id: "claim-old",
                now_unix_ms: 112,
                updated_at: "unix-ms:112",
            },
            |_| DispatcherConsumerSuccess::new((), "stale-receipt"),
        )
        .expect_err("old lease fenced");
        assert!(stale.to_string().contains("HOST_BINDING_LEASE_FENCED"));

        let error = claim_dispatcher_batch_with_consumer(
            &store,
            DispatcherBatchRequest {
                lease: &current,
                older_than_unix_ms: 100,
                limit: 10,
                claim_id: "claim-current",
                now_unix_ms: 112,
                updated_at: "unix-ms:112",
            },
            |batch| {
                assert!(!batch.is_empty());
                Err::<DispatcherConsumerSuccess<()>, _>(StoreError::Conflict(
                    "consumer unavailable".into(),
                ))
            },
        )
        .expect_err("consumer failure");
        assert!(error.to_string().contains("consumer unavailable"));
        let row = store
            .host_attentions()
            .expect("attentions")
            .into_iter()
            .find(|row| row.id == attention_id)
            .expect("attention");
        assert_eq!(row.status, HostAttentionStatus::Actionable);
        assert!(row.claim_id.is_none());

        let value = claim_dispatcher_batch_with_consumer(
            &store,
            DispatcherBatchRequest {
                lease: &current,
                older_than_unix_ms: 100,
                limit: 10,
                claim_id: "claim-success",
                now_unix_ms: 113,
                updated_at: "unix-ms:113",
            },
            |batch| {
                assert_eq!(batch.len(), 1);
                DispatcherConsumerSuccess::new("consumer-value", "provider-receipt-1")
            },
        )
        .expect("consumer success completes batch");
        assert_eq!(value, "consumer-value");
        let delivered = store
            .host_attentions()
            .expect("attentions")
            .into_iter()
            .find(|row| row.id == attention_id)
            .expect("attention");
        assert_eq!(delivered.status, HostAttentionStatus::Delivered);
        assert_eq!(
            delivered.provider_receipt_id.as_deref(),
            Some("provider-receipt-1")
        );

        let consumer_called = std::cell::Cell::new(false);
        let empty = claim_dispatcher_batch_with_consumer(
            &store,
            DispatcherBatchRequest {
                lease: &current,
                older_than_unix_ms: 100,
                limit: 10,
                claim_id: "claim-empty",
                now_unix_ms: 114,
                updated_at: "unix-ms:114",
            },
            |_| {
                consumer_called.set(true);
                DispatcherConsumerSuccess::new((), "impossible-receipt")
            },
        )
        .expect_err("empty batch must not invoke consumer");
        assert!(empty.to_string().contains("HOST_DISPATCH_NOTHING_CLAIMED"));
        assert!(!consumer_called.get());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn consumer_panic_requeues_every_claim_before_resuming_unwind() {
        let (store, root, run) = fixture();
        let attention_id = store
            .reconcile_host_binding_stale_attentions(50, "unix-ms:50")
            .expect("stale attention")
            .into_iter()
            .find(|row| row.team_run_id == run.id)
            .expect("run stale attention")
            .id;
        let lease = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher",
                "lease-panic",
                100,
                100,
            )
            .expect("lease");
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = claim_dispatcher_batch_with_consumer::<(), _>(
                &store,
                DispatcherBatchRequest {
                    lease: &lease,
                    older_than_unix_ms: 100,
                    limit: 10,
                    claim_id: "claim-panic",
                    now_unix_ms: 101,
                    updated_at: "unix-ms:101",
                },
                |_| panic!("consumer panic"),
            );
        }));
        assert!(panic.is_err());
        let row = store
            .host_attentions()
            .expect("attentions")
            .into_iter()
            .find(|row| row.id == attention_id)
            .expect("attention");
        assert_eq!(row.status, HostAttentionStatus::Actionable);
        assert!(row.claim_id.is_none());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn expired_dispatcher_claim_is_recovered_once_after_takeover() {
        let (store, root, run) = fixture();
        let attention_id = store
            .reconcile_host_binding_stale_attentions(50, "unix-ms:50")
            .expect("stale attention")
            .into_iter()
            .find(|row| row.team_run_id == run.id)
            .expect("run stale attention")
            .id;
        let old = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher-old",
                "lease-old-crash",
                100,
                10,
            )
            .expect("old lease");
        let crashed = store
            .claim_dispatcher_host_attention_batch(
                &old,
                100,
                10,
                "claim-before-crash",
                101,
                "unix-ms:101",
            )
            .expect("claim before simulated crash");
        assert_eq!(crashed.len(), 1);
        let still_claimed = store
            .claim_dispatcher_host_attention_batch(
                &old,
                100,
                10,
                "claim-while-live",
                102,
                "unix-ms:102",
            )
            .expect("live lease does not steal its claim");
        assert!(still_claimed.is_empty());

        let current = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-1",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher-current",
                "lease-current-recovery",
                110,
                100,
            )
            .expect("take over expired lease");
        assert!(store
            .complete_host_attention_claim(
                &attention_id,
                "claim-before-crash",
                "stale-receipt",
                "unix-ms:111",
            )
            .expect_err("stale lease cannot complete")
            .to_string()
            .contains("LEASE_FENCED"));
        assert!(store
            .fail_host_attention_claim(
                &attention_id,
                "claim-before-crash",
                "stale failure",
                "unix-ms:111",
            )
            .expect_err("stale lease cannot fail")
            .to_string()
            .contains("LEASE_FENCED"));

        let recovered = store
            .claim_dispatcher_host_attention_batch(
                &current,
                100,
                10,
                "claim-after-takeover",
                111,
                "unix-ms:111",
            )
            .expect("recover and claim");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, attention_id);
        assert_eq!(recovered[0].attempt, 2);
        let repeat = store
            .claim_dispatcher_host_attention_batch(
                &current,
                100,
                10,
                "claim-repeat",
                112,
                "unix-ms:112",
            )
            .expect("idempotent recovery");
        assert!(repeat.is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
