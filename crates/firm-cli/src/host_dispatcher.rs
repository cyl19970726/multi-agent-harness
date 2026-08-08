//! Host-attention scheduling for provider-native Host bindings.
//!
//! This module deliberately stops at scheduling. A foreground TeamRun
//! supervisor has no provider-neutral driver for the Host session, so merely
//! noticing ready attention must never claim it. The leased claim seam below
//! is reserved for a caller that supplies both an exact Dispatcher lease and
//! an execution consumer.

use harness_core::{
    HostAttention, HostBindingLease, HostBindingLeaseOwnerKind, HostDispatchConfig,
};
use harness_store::{HarnessStore, StoreError};

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
    pub handled: Vec<String>,
    pub escalated: Vec<String>,
    pub failed: Vec<String>,
    pub ready: Vec<String>,
    pub stale: Vec<String>,
}

impl DispatchOutcome {
    pub fn is_noop(&self) -> bool {
        self.ready.is_empty()
            && self.stale.is_empty()
            && self.handled.is_empty()
            && self.escalated.is_empty()
            && self.failed.is_empty()
    }
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
            outcome.inspected = attention_ids.len() + stale_attention_ids.len();
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

    let stale_attention_ids = store
        .reconcile_host_binding_stale_attentions(now_unix_ms, observed_at)?
        .into_iter()
        .filter(|attention| attention.team_run_id == team_run_id)
        .map(|attention| attention.id)
        .collect::<Vec<_>>();
    let cutoff =
        now_unix_ms.saturating_sub(config.attention_age_threshold_secs.saturating_mul(1_000));
    let attention_ids = store
        .actionable_attentions_older_than(cutoff)?
        .into_iter()
        .filter(|attention| attention.team_run_id == team_run_id)
        .map(|attention| attention.id)
        .collect::<Vec<_>>();

    if attention_ids.is_empty() && stale_attention_ids.is_empty() {
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
#[allow(dead_code)] // Kernel seam for the future headless driver (#415).
pub fn claim_dispatcher_batch_with_consumer<T, F>(
    store: &HarnessStore,
    lease: &HostBindingLease,
    older_than_unix_ms: u64,
    limit: usize,
    claim_id: &str,
    now_unix_ms: u64,
    updated_at: &str,
    consumer: F,
) -> Result<T, StoreError>
where
    F: FnOnce(&[HostAttention]) -> Result<T, StoreError>,
{
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
    match consumer(&claimed) {
        Ok(value) => Ok(value),
        Err(error) => {
            for attention in &claimed {
                store.fail_host_attention_claim(
                    &attention.id,
                    claim_id,
                    "dispatcher consumer rejected claimed batch",
                    updated_at,
                )?;
            }
            Err(error)
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
    fn expired_or_unleased_binding_is_dispatch_ready_without_claiming() {
        let (store, root, run) = fixture();
        let config = HostDispatchConfig {
            attention_age_threshold_secs: 0,
            ..HostDispatchConfig::default()
        };
        let decision =
            schedule_team_run(&store, &run.id, &config, 100, "unix-ms:100").expect("schedule");
        assert!(matches!(decision, ScheduleDecision::DispatchReady { .. }));
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
        let config = HostDispatchConfig {
            attention_age_threshold_secs: 0,
            ..HostDispatchConfig::default()
        };
        let outcome = poll_and_dispatch(&store, &ledger, "test", &config).expect("poll");
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
            &old,
            100,
            10,
            "claim-old",
            112,
            "unix-ms:112",
            |_| Ok(()),
        )
        .expect_err("old lease fenced");
        assert!(stale.to_string().contains("HOST_BINDING_LEASE_FENCED"));

        let error = claim_dispatcher_batch_with_consumer(
            &store,
            &current,
            100,
            10,
            "claim-current",
            112,
            "unix-ms:112",
            |batch| {
                assert!(!batch.is_empty());
                Err::<(), _>(StoreError::Conflict("consumer unavailable".into()))
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
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
