//! #779: a mid-turn member must come back from a NodeDaemon drain by itself.
//!
//! DEV-171 (#748) proved the drained Session *can* resume and DEV-179 (#756)
//! proved its in-flight Work comes back — but both settled the lane by hand
//! before asking. The live r5b run showed what happens when nothing does: the
//! member runner opens its provider handle and publishes an Attached residency
//! while the lane is still `Interrupted`, so the first cycle's own
//! `Interrupted -> Idle -> Active` projection is refused by the drain fence,
//! the runner journals `Blocked`, and every later pass reads that row as
//! operator lifecycle control ("member provider start was superseded by
//! lifecycle control") and holds adoption forever.
//!
//! The first test drives the real adoption seam and the runner's own durable
//! sequence, with no Host verb anywhere. The second covers the one Host verb
//! that repairs a member already wedged that way.

use super::drain_inflight_work_tests::assign_work;
use super::drain_recovery_tests::{
    agent_session, drain_fixture, member_named, DrainFixture, DRAIN_SPACE_ID, IDLE_MEMBER,
    MID_TURN_MEMBER,
};
use super::*;

use crate::claim_canonical_work_for_member;
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSessionStatus, MutationContext, RuntimeActivity,
    RuntimeCommandStatus, RuntimeEffectCertainty, RuntimeResidency, WorkDeliveryStatus,
};
use harness_core::{MemberRunStatus, ProviderRuntimeProjection};

/// Every MemberRun of this TeamRun, by name and status.
fn member_statuses(fixture: &DrainFixture) -> Vec<(String, MemberRunStatus)> {
    crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("member runs")
        .into_iter()
        .filter(|member| member.team_run_id == fixture.run_id)
        .map(|member| (member.name.clone(), member.status))
        .collect()
}

/// Take the successor NodeDaemon generation and reattach each drained Session
/// to it — and stop there, without the adoption seam's resume.
///
/// This is the pre-fix shape of `readopt`: the lane is on the live generation
/// but still `Interrupted`, which is exactly the state the member runner used
/// to be handed. Nothing else can reproduce it once the adoption seam resumes
/// the lane, so the runner's own refusal handling has no other way to be
/// exercised.
pub(super) fn reattach_without_resuming(fixture: &DrainFixture) -> u64 {
    let successor = fixture
        .store
        .acquire_node_daemon_lease(
            fixture.node_id(),
            fixture.daemon_id(),
            "drain-instance-2",
            crate::current_unix_ms_u64(),
            600_000,
        )
        .expect("successor NodeDaemon generation");
    assert!(successor.generation > fixture.daemon_generation);
    for session in fixture
        .store
        .fabric_agent_sessions(DRAIN_SPACE_ID)
        .expect("agent sessions")
    {
        if session.node_daemon_generation == successor.generation {
            continue;
        }
        fixture
            .store
            .reattach_agent_session_to_node_daemon(
                &MutationContext {
                    execution_space_id: DRAIN_SPACE_ID.to_string(),
                    authenticated_actor: ActorRef {
                        kind: ActorKind::Service,
                        id: successor.daemon_id.clone(),
                    },
                    authority_actor: None,
                    command_name: "runtime_fabric.session.reattach_node_daemon".into(),
                    idempotency_key: format!(
                        "session-daemon-reattach:{}:{}:{}",
                        session.id, session.node_daemon_generation, successor.generation
                    ),
                    expected_version: session.version,
                    request_fingerprint: None,
                },
                &session.id,
                session.runtime_generation,
                session.node_daemon_generation,
                &successor.daemon_id,
                successor.generation,
                &crate::now_string(),
            )
            .expect("reattach the drained Session without resuming it");
    }
    successor.generation
}

/// The exact durable sequence the member runner performs when it resumes a
/// member, and the exact disposition it applies to whatever that meets.
///
/// Mirrors `run_codex_member_shared` + the failure frame of
/// `run_member_orchestration`: admit the `ResumeNativeSession` process effect,
/// settle it applied once the handle is open, publish the attached residency,
/// then let the first cycle project the Session `Active`. On failure the handle
/// is released and the runner's own classifier decides what the attempt meant
/// for the member — the seam this regression is about.
fn drive_first_resumed_cycle(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> crate::MemberOutcome {
    let effect = crate::prepare_provider_process_effect(ledger, member, 1)
        .expect("the successor generation admits one ResumeNativeSession command");
    crate::settle_provider_effect(
        ledger,
        &effect,
        true,
        Some(serde_json::json!({
            "provider": "codex",
            "phase": "runtime_attached",
        })),
        None,
    )
    .expect("the resume command settles applied once the handle is open");
    crate::transition_provider_session_runtime_control(
        ledger,
        member,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("the runner publishes its attached provider handle");
    let cycle =
        crate::transition_provider_session_for_member(ledger, member, AgentSessionStatus::Active);
    // The runner publishes the adapter handle's release in both directions,
    // before it interprets the outcome, so the lane a successor generation
    // finds is never left claiming an attached handle.
    crate::settle_provider_attempt_release(ledger, member)
        .expect("the local handle release is published");
    match cycle {
        Ok(()) => crate::MemberOutcome::new(
            member,
            MemberRunStatus::Running,
            "the first resumed cycle opened".to_string(),
        ),
        Err(error) => {
            // An attempt-scoped drain-fence refusal leaves the member
            // startable; anything else is the ordinary Blocked verdict.
            let latest = ledger
                .latest_member_run(&member.id)
                .expect("member run read")
                .expect("member run");
            let reason = error.to_string();
            if crate::provider_failure_awaits_drain_lane_resume(ledger, &latest, &error) {
                crate::journal_member_awaiting_drain_lane_resume(ledger, &latest, 1, &reason)
            } else {
                crate::journal_provider_attempt_exhausted_block(
                    ledger,
                    &latest,
                    &error,
                    &harness_application::ProviderEffectOutcome::Accepted {
                        receipt_id: effect.command_id.clone(),
                    },
                    1,
                    &reason,
                )
            }
        }
    }
}

#[test]
fn drained_member_returns_to_a_startable_lane_without_any_host_verb() {
    let fixture = drain_fixture("drain-blocked-member");
    let ledger = fixture.supervise("supervisor-blocked-1", fixture.daemon_generation);
    let drained_work = assign_work(
        &fixture,
        MID_TURN_MEMBER,
        "blocked",
        "Work the member is mid-turn on when the daemon drains",
        "unix-ms:3",
    );
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("dispatch one canonical Work")
        .expect("the assigned Work is dispatched");
    assert_eq!(claimed.work.id, drained_work.id);
    ledger
        .complete_work_delivery(&claimed, "provider-receipt:drain-blocked")
        .expect("the provider receives the Work before the drain");
    fixture.start_cycle_for(&ledger, &claimed.delivery.id);
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    assert!(
        !crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &member),
        "a live mid-turn lane must never read as a dead runtime"
    );
    drop(ledger);

    fixture.drain("supervisor-blocked-1", 1);
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Interrupted,
        "the drain cuts the mid-turn cycle"
    );

    // ── the successor daemon generation adopts. No Host verb from here on. ──
    let successor_generation = fixture.readopt();
    let first_pass = fixture.supervise("supervisor-blocked-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let outcome = drive_first_resumed_cycle(&first_pass, &mid_turn);
    drop(first_pass);
    fixture
        .store
        .release_team_supervisor_lease(
            &fixture.run_id,
            "supervisor-blocked-2",
            2,
            crate::current_unix_ms_u64(),
        )
        .expect("the first pass releases its Supervisor lease when it ends");

    // ── one pass later, exactly as the daemon rescans ──
    let second_pass = fixture.supervise("supervisor-blocked-3", successor_generation);
    let after_first_pass = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let claim = crate::claim_member_provider_start(&second_pass, &after_first_pass)
        .expect("the start claim is readable");
    let claim_label = match &claim {
        crate::MemberProviderStartClaim::Claimed(_) => "claimed".to_string(),
        crate::MemberProviderStartClaim::Superseded(latest) => format!(
            "superseded: \"member provider start was superseded by lifecycle control\" at status {:?}",
            latest.status
        ),
        crate::MemberProviderStartClaim::Retry => "retry".to_string(),
    };
    assert!(
        matches!(claim, crate::MemberProviderStartClaim::Claimed(_)),
        "a drained member must still be startable one pass later, with no Host verb. \
         member status={:?}; first-pass outcome={:?}: {}; start claim={claim_label}; \
         adoption would now be held on this member forever",
        after_first_pass.status,
        outcome.status,
        outcome.summary
    );
    assert_ne!(
        outcome.status,
        MemberRunStatus::Blocked,
        "the drain fence refusing a resume is attempt-scoped, never a member verdict: {}",
        outcome.summary
    );
    for (name, status) in member_statuses(&fixture) {
        assert_ne!(
            status,
            MemberRunStatus::Blocked,
            "no member may be journalled blocked by an ordinary drain restart ({name})"
        );
    }

    // ── the lane resumed onto the successor generation, identity intact ──
    let resumed = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_ne!(
        resumed.lifecycle,
        AgentSessionStatus::Interrupted,
        "the lane must re-enter the ordinary lane on its own"
    );
    assert_eq!(resumed.node_daemon_generation, successor_generation);
    assert_eq!(
        resumed
            .native_session_ref
            .as_ref()
            .map(|native| native.native_session_id.as_str()),
        Some(format!("thread-drain-{MID_TURN_MEMBER}").as_str()),
        "resume keeps the provider-native session identity"
    );

    // ── the superseded Work returns through the ordinary dispatch path ──
    let superseded = fixture
        .store
        .fabric_work_deliveries(DRAIN_SPACE_ID)
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.id == claimed.delivery.id)
        .expect("the killed delivery");
    assert_eq!(superseded.status, WorkDeliveryStatus::Failed);
    assert_eq!(
        superseded.failure_code.as_deref(),
        Some(harness_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN)
    );
    let redelivered = claim_canonical_work_for_member(&second_pass, &after_first_pass)
        .expect("the ordinary dispatch pass runs")
        .expect("the superseded Work returns without a Host verb");
    assert_eq!(redelivered.work.id, drained_work.id);
    assert_ne!(redelivered.delivery.id, claimed.delivery.id);
    assert_ne!(redelivered.delivery.claim_id, claimed.delivery.claim_id);
    assert_eq!(
        redelivered.delivery.claimed_node_daemon_generation,
        Some(successor_generation)
    );

    // ── the killed cycle stays exactly one settled, non-replayed fact ──
    let after = fixture.start_cycles();
    assert_eq!(
        after.len(),
        1,
        "resume opens a new cycle and never replays the killed one"
    );
    assert_eq!(after[0].id, killed_cycle[0].id);
    assert_eq!(after[0].status, RuntimeCommandStatus::Applied);
    assert_eq!(after[0].effect_certainty, RuntimeEffectCertainty::Applied);
    assert_eq!(
        after[0].target_node_daemon_generation, fixture.daemon_generation,
        "the killed cycle stays bound to the dead daemon generation"
    );
}

/// #779 review P2-2: the runner's own refusal handling, on the exact lane state
/// the pre-fix daemon handed it. The adoption seam normally resumes the lane
/// first, so this reaches the runner through a reattach-only successor
/// generation — the shape the live r5b run hit.
#[test]
fn a_runner_that_meets_the_drain_fence_leaves_the_member_startable() {
    let fixture = drain_fixture("drain-runner-fence");
    let ledger = fixture.supervise("supervisor-fence-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:drain:fence:1");
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    drop(ledger);
    fixture.drain("supervisor-fence-1", 1);

    let successor_generation = reattach_without_resuming(&fixture);
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Interrupted,
        "this test only means something while the lane is still interrupted"
    );

    let ledger = fixture.supervise("supervisor-fence-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    // The runner claims the start before it attaches anything, which is what
    // leaves the row `Starting` — the non-startable state the failed attempt
    // must then correct rather than harden into `Blocked`.
    let claimed = match crate::claim_member_provider_start(&ledger, &mid_turn)
        .expect("the start claim is readable")
    {
        crate::MemberProviderStartClaim::Claimed(starting) => starting,
        crate::MemberProviderStartClaim::Superseded(latest) => panic!(
            "the drained member must be claimable, not superseded at status {:?}",
            latest.status
        ),
        crate::MemberProviderStartClaim::Retry => {
            panic!("the drained member must be claimable, not contended")
        }
    };
    assert_eq!(claimed.status, MemberRunStatus::Starting);
    let outcome = drive_first_resumed_cycle(&ledger, &claimed);

    assert_eq!(
        outcome.status,
        MemberRunStatus::Disconnected,
        "the drain fence refusing a resume is attempt-scoped: {}",
        outcome.summary
    );
    assert!(
        outcome.summary.starts_with("DRAIN_LANE_RESUME_PENDING:"),
        "the member must say why it is waiting: {}",
        outcome.summary
    );
    let after = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    assert_eq!(after.status, MemberRunStatus::Disconnected);
    for (name, status) in member_statuses(&fixture) {
        assert_ne!(
            status,
            MemberRunStatus::Blocked,
            "a transient fence must not journal {name} blocked"
        );
    }
    // The handle release the runner published left the lane resumable, and the
    // member is startable again with no Host verb.
    assert!(
        crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &after),
        "the released lane proves the runtime is gone"
    );
    assert!(
        matches!(
            crate::claim_member_provider_start(&ledger, &after).expect("start claim is readable"),
            crate::MemberProviderStartClaim::Claimed(_)
        ),
        "the next Supervisor pass must be able to retry"
    );
    assert_eq!(
        fixture.start_cycles().len(),
        1,
        "the refused attempt never replays the killed cycle"
    );
}

/// The retry note is not a lifecycle decision. A durable Close latched over the
/// member outranks it, and the ordinary control path — not the journal — is
/// what applies that Close.
#[test]
fn a_latched_close_outranks_the_drain_retry_note() {
    let fixture = drain_fixture("drain-retry-close");
    let ledger = fixture.supervise("supervisor-retry-close-1", fixture.daemon_generation);
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);

    // The status the start claim leaves behind, which the journal would
    // otherwise correct to Disconnected.
    let expected = member.clone();
    let mut starting = expected.clone();
    starting.status = MemberRunStatus::Starting;
    starting.last_event_at = Some("unix-ms:retry-close".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &starting)
        .expect("claim the member for a provider start");

    let close = harness_core::TeamMemberCloseRequest {
        id: format!("team-member-close:{}", starting.id),
        team_run_id: fixture.run_id.clone(),
        member_run_id: starting.id.clone(),
        requested_by: "host".into(),
        reason: "the Host closed this member while the attempt was in flight".into(),
        status: harness_core::TeamMemberCloseStatus::Pending,
        requested_at: crate::now_string(),
        applied_at: None,
        detached_recovery_fence: None,
    };
    fixture
        .store
        .latch_team_member_close_for_supervisor(&close, "supervisor-retry-close-1", 1)
        .expect("latch the Host Close under the live Supervisor");

    let outcome = crate::journal_member_awaiting_drain_lane_resume(
        &ledger,
        &starting,
        1,
        "store error: conflict: drain fence",
    );

    assert_eq!(
        outcome.status,
        MemberRunStatus::Starting,
        "a latched Close is authoritative; the retry note never rewrites the status"
    );
    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Starting
    );
    assert!(
        crate::pending_member_close(&fixture.store, &starting.id)
            .expect("close read")
            .is_some(),
        "the Close latch survives for the ordinary control path to apply"
    );
}

/// #779 review P1-1: `Blocked` alone is not provenance. A member blocked by the
/// provider-compatibility gate sits on a Cold, detached, disarmed lane, so a
/// lane-only test would restart it — and the typed cause is bound to `Blocked`
/// by validation, so the resulting row is unappendable and used to abort the
/// whole recovery run before later members were repaired.
#[test]
fn recover_reports_a_typed_block_and_still_repairs_the_drain_blocked_member() {
    let fixture = drain_fixture("drain-recover-provenance");
    let ledger = fixture.supervise("supervisor-provenance-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:drain:provenance:1");
    fixture.idle_one_member(&ledger);
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    drop(ledger);
    fixture.drain("supervisor-provenance-1", 1);
    fixture.readopt();

    // One member wedged by the drain fence, with no typed provenance.
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:drain-blocked".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the drain-blocked member");

    // One member blocked by the compatibility gate, through its own writer, so
    // the row carries the exact typed cause the Store binds to Blocked.
    let compatible = member_named(&fixture.store, &fixture.run_id, IDLE_MEMBER);
    let profile = compatible
        .provider_profile
        .clone()
        .expect("the fixture member has an observed provider profile");
    let cause = harness_core::ProviderCompatibilityBlockCause {
        schema_version: 1,
        id: format!("provider-compatibility-block:{}:1", compatible.id),
        member_run_id: compatible.id.clone(),
        provider: profile.provider.clone(),
        execution_mode: profile.execution_mode.clone(),
        provider_version: profile
            .provider_version
            .clone()
            .unwrap_or_else(|| "unavailable".into()),
        adapter_contract_version: profile
            .adapter_contract_version
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        boundary: harness_core::ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: profile.compatibility_status,
        source: harness_core::ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: crate::now_string(),
    };
    fixture
        .store
        .block_member_run_for_provider_compatibility(
            &compatible,
            &profile,
            cause,
            "unix-ms:compatibility-blocked",
        )
        .expect("block the second member through the compatibility gate");

    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, true)
        .expect("a typed block is reported, never an aborted recovery run");

    assert_eq!(report["restarted_blocked_members"], serde_json::json!(1));
    let reported = report["blocked_members_not_restarted"]
        .as_array()
        .expect("the report names every block it did not clear");
    assert_eq!(reported.len(), 1);
    assert_eq!(
        reported[0]["member_run_id"],
        serde_json::json!(compatible.id)
    );
    assert_eq!(
        reported[0]["provenance"],
        serde_json::json!("provider_compatibility")
    );
    assert!(
        reported[0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("provider review gate")),
        "the report must name the gate that owns the block: {:?}",
        reported[0]["reason"]
    );

    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Idle,
        "the drain-blocked member is repaired in the same run"
    );
    let untouched = member_named(&fixture.store, &fixture.run_id, IDLE_MEMBER);
    assert_eq!(untouched.status, MemberRunStatus::Blocked);
    assert!(
        untouched.provider_compatibility_block_cause.is_some(),
        "the typed cause survives the recovery run intact"
    );
}

/// #779: a held adoption is keyed to the canonical state it observed, so it can
/// only lift on its own if that key sees the execution lane. A lane leaving
/// `Interrupted` changes no MemberRun, Work, Message or RuntimeCommand row —
/// which is exactly why the wedged run needed a Host poke to be re-scanned.
#[test]
fn the_adoption_hold_fingerprint_sees_the_execution_lane() {
    let fixture = drain_fixture("drain-lane-fingerprint");
    let ledger = fixture.supervise("supervisor-lane-1", fixture.daemon_generation);
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);

    let fingerprint = || {
        crate::team_run_canonical_state_fingerprint(
            &fixture.store,
            Some(DRAIN_SPACE_ID),
            &fixture.run_id,
        )
        .expect("canonical state fingerprint")
    };
    let other_planes = || {
        (
            member_statuses(&fixture),
            fixture
                .store
                .work_operations()
                .expect("work operations")
                .len(),
            fixture
                .store
                .fabric_messages(DRAIN_SPACE_ID)
                .expect("messages")
                .len(),
            fixture
                .store
                .runtime_commands(DRAIN_SPACE_ID)
                .expect("runtime commands")
                .len(),
        )
    };
    let before = fingerprint();
    let before_planes = other_planes();

    crate::transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Idle)
        .expect("the lane becomes resumable");

    assert_eq!(
        other_planes(),
        before_planes,
        "this test is only meaningful while nothing but the lane changed"
    );
    assert_ne!(
        fingerprint(),
        before,
        "a lane becoming resumable must change the canonical state a hold is keyed to, \
         otherwise automatic adoption stays held until a Host pokes the run"
    );
}

#[test]
fn recover_returns_a_blocked_member_on_a_dead_lane_to_a_startable_status() {
    let fixture = drain_fixture("drain-blocked-recover");
    let ledger = fixture.supervise("supervisor-recover-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:drain:recover:1");
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    drop(ledger);
    fixture.drain("supervisor-recover-1", 1);
    let successor_generation = fixture.readopt();

    // Wedge the member exactly the way the pre-fix runner did: a Blocked row
    // over a lane that is already back in the ordinary lane and detached.
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:drain-blocked".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    assert!(
        crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &blocked),
        "the adopted lane proves the drained runtime is gone"
    );
    assert!(
        fixture
            .store
            .latest_team_supervisor_lease(&fixture.run_id)
            .expect("supervisor lease read")
            .is_none_or(|lease| lease.status == harness_core::TeamSupervisorLeaseStatus::Released),
        "the repair must not need a current Supervisor"
    );

    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, true)
        .expect("recover reads and repairs without a Supervisor");
    assert_eq!(report["restarted_blocked_members"], serde_json::json!(1));

    let repaired = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    assert_eq!(repaired.status, MemberRunStatus::Idle);
    assert!(
        repaired.coordination_is_active(),
        "the repair is a status correction, never a coordination change"
    );
    assert_eq!(
        repaired.runtime_generation, expected.runtime_generation,
        "the repair never mints a new runtime generation"
    );
    assert_eq!(
        repaired.native_session, expected.native_session,
        "the repair never touches provider-native session truth"
    );

    let lane = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(lane.node_daemon_generation, successor_generation);
    assert_eq!(
        lane.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert_eq!(
        fixture.start_cycles().len(),
        1,
        "the repair never replays the killed cycle"
    );

    // The repaired member is startable again by the ordinary claim.
    let ledger = fixture.supervise("supervisor-recover-2", successor_generation);
    assert!(
        matches!(
            crate::claim_member_provider_start(&ledger, &repaired)
                .expect("the start claim is readable"),
            crate::MemberProviderStartClaim::Claimed(_)
        ),
        "recover must leave a status the Supervisor will actually start"
    );
}
