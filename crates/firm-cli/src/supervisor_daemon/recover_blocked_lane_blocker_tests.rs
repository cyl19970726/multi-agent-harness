//! GitHub #841 (DEV-232), the lane-closability half: `team-run recover` and
//! the detached-recovery Close must judge a lane with one predicate, and
//! recover must name the exact clause that blocks a Blocked member instead of
//! reporting success (or a bare "skipped") for a lane Close will refuse.

use super::drain_recovery_tests::{
    agent_session, drain_fixture, member_named, DRAIN_SPACE_ID, MID_TURN_MEMBER,
};
use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity};
use harness_core::MemberRunStatus;

#[test]
fn recover_names_the_blocker_of_a_blocked_member_whose_lane_is_still_live() {
    let fixture = drain_fixture("recover-lane-blocker");
    let ledger = fixture.supervise("supervisor-blocker-1", fixture.daemon_generation);
    // The member is mid-cycle with an attached provider handle.
    fixture.start_cycle_for(&ledger, "work-delivery:blocker:1");

    // A runner journals Blocked over that live lane (the #779 shape, but the
    // lane is NOT dead this time).
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:blocked-live".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    assert!(
        !crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &blocked),
        "an attached, mid-cycle lane never proves its runtime gone"
    );
    let blocker = crate::member_lane_blocker(&fixture.store, DRAIN_SPACE_ID, &blocked)
        .expect("the failing clause is named");
    assert!(
        blocker.contains("not at a terminal turn boundary"),
        "the first failing clause is the open cycle: {blocker}"
    );
    // The report and the repair decision derive from one proof.
    assert_eq!(
        crate::member_lane_blocker(&fixture.store, DRAIN_SPACE_ID, &blocked).is_none(),
        crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &blocked)
    );

    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, true)
        .expect("recover reports without repairing");
    assert_eq!(report["restarted_blocked_members"], serde_json::json!(0));
    let lanes = report["blocked_lanes_not_proven"]
        .as_array()
        .expect("blocked_lanes_not_proven is reported");
    assert_eq!(lanes.len(), 1, "{report}");
    assert_eq!(lanes[0]["member_run_id"], serde_json::json!(blocked.id));
    assert_eq!(lanes[0]["blocker"], serde_json::json!(blocker));
    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Blocked,
        "a lane that is not proven dead is never restarted"
    );
}

#[test]
fn recover_and_close_share_one_terminal_turn_boundary_predicate() {
    let fixture = drain_fixture("shared-boundary-predicate");
    let base = agent_session(&fixture.store, MID_TURN_MEMBER);
    let lifecycles = [
        AgentSessionStatus::Cold,
        AgentSessionStatus::Idle,
        AgentSessionStatus::Active,
        AgentSessionStatus::Waiting,
        AgentSessionStatus::Interrupted,
        AgentSessionStatus::RecoveryRequired,
        AgentSessionStatus::Closed,
    ];
    for lifecycle in lifecycles {
        for activity in [RuntimeActivity::Idle, RuntimeActivity::Running] {
            for turn in [None, Some("provider-turn:x".to_string())] {
                let mut session = base.clone();
                session.lifecycle = lifecycle;
                session.control_state.activity = activity;
                session.current_turn_id = turn;
                assert_eq!(
                    crate::lane_is_at_terminal_turn_boundary(&session),
                    crate::session_is_at_terminal_turn_boundary(&session),
                    "recover and close disagree on {lifecycle:?}/{activity:?}"
                );
            }
        }
    }
    // The boundary itself: quiet lanes in every non-executing lifecycle,
    // including a reconciled RecoveryRequired one (#755), and nothing else.
    let mut quiet = base.clone();
    quiet.control_state.activity = RuntimeActivity::Idle;
    quiet.current_turn_id = None;
    for lifecycle in [
        AgentSessionStatus::Cold,
        AgentSessionStatus::Idle,
        AgentSessionStatus::Interrupted,
        AgentSessionStatus::RecoveryRequired,
    ] {
        quiet.lifecycle = lifecycle;
        assert!(
            crate::lane_is_at_terminal_turn_boundary(&quiet),
            "{lifecycle:?}"
        );
    }
    for lifecycle in [
        AgentSessionStatus::Active,
        AgentSessionStatus::Waiting,
        AgentSessionStatus::Closed,
    ] {
        quiet.lifecycle = lifecycle;
        assert!(
            !crate::lane_is_at_terminal_turn_boundary(&quiet),
            "{lifecycle:?}"
        );
    }
}

/// GitHub #755, the writer: `team-run recover` returns a reconciled
/// `RecoveryRequired` lane to `Idle` before it returns the member to a
/// startable status, so the next Supervisor start is admitted instead of being
/// refused on a recovery-required lane (the churn the round-1 review named).
#[test]
fn recover_returns_a_recovery_required_lane_to_idle_before_restarting_the_member() {
    let fixture = drain_fixture("recover-recovery-required");
    let ledger = fixture.supervise("supervisor-rr-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);

    // The runner records the unrecoverable provider error, the process is
    // reaped (detached, idle), and the member is journaled Blocked.
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("the reaped process detaches the lane");
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:rr-blocked".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );
    assert!(
        crate::member_lane_proves_runtime_gone(&fixture.store, DRAIN_SPACE_ID, &blocked),
        "a detached, quiet RecoveryRequired lane proves its runtime gone"
    );

    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, true)
        .expect("recover repairs the lane and the member");
    assert_eq!(
        report["restarted_blocked_members"],
        serde_json::json!(1),
        "{report}"
    );
    assert_eq!(report["blocked_lanes_not_proven"], serde_json::json!([]));
    let lane = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(
        lane.lifecycle,
        AgentSessionStatus::Idle,
        "the lane re-entered the ordinary lane before the member moved"
    );
    assert_eq!(lane.runtime_generation, expected.runtime_generation);
    let repaired = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    assert_eq!(repaired.status, MemberRunStatus::Idle);
    assert!(repaired.coordination_is_active());
    assert_eq!(repaired.native_session, expected.native_session);

    // The repaired member is startable again by the ordinary claim.
    assert!(
        matches!(
            crate::claim_member_provider_start(&ledger, &repaired)
                .expect("the start claim is readable"),
            crate::MemberProviderStartClaim::Claimed(_)
        ),
        "recover must leave a lane and a status the Supervisor will actually start"
    );
}

/// The same lane while its handle is still attached: recover names the clause
/// and leaves the member Blocked; nothing is written to the lane.
#[test]
fn recover_names_the_clause_that_keeps_a_recovery_required_lane_shut() {
    let fixture = drain_fixture("recover-recovery-required-shut");
    let ledger = fixture.supervise("supervisor-rr-shut-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr-shut:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    let blocker = crate::member_lane_blocker(&fixture.store, DRAIN_SPACE_ID, &blocked)
        .expect("the attached handle is named");
    assert!(
        blocker.contains("not at a terminal turn boundary") || blocker.contains("attached"),
        "{blocker}"
    );

    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, true)
        .expect("recover reports without repairing");
    assert_eq!(report["restarted_blocked_members"], serde_json::json!(0));
    let lanes = report["blocked_lanes_not_proven"]
        .as_array()
        .expect("reported");
    assert_eq!(lanes.len(), 1, "{report}");
    assert_eq!(lanes[0]["blocker"], serde_json::json!(blocker));
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );
    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Blocked
    );
}

/// The detached-recovery Close accepts the same reconciled `RecoveryRequired`
/// lane `recover` accepts, through the real Close path (BlockedMemberExactGeneration).
#[test]
fn close_member_for_recovery_accepts_a_reconciled_recovery_required_lane() {
    let fixture = drain_fixture("close-recovery-required");
    let ledger = fixture.supervise("supervisor-rr-close-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr-close:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("the reaped process detaches the lane");
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    let lease = fixture
        .store
        .latest_team_supervisor_lease(&fixture.run_id)
        .expect("lease read")
        .expect("the fixture Supervisor holds the lease");

    let closed = crate::close_detached_blocked_member_for_recovery(
        &fixture.store,
        &fixture.run_id,
        &blocked,
        &lease,
        "host",
        "the member should not come back at all",
    )
    .expect("a reconciled RecoveryRequired lane is closable")
    .expect("the Close was applied, not skipped");
    assert_eq!(closed["coordination_status"], serde_json::json!("closed"));
    assert_eq!(
        closed["runtime_effect"],
        serde_json::json!("already_detached")
    );
    let after = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    assert_eq!(after.status, MemberRunStatus::Stopped);
    assert!(!after.coordination_is_active());
    assert_eq!(closed["dormant_residue"], serde_json::json!([]));
    // The Close performed the same Idle hop recover does, so a later Reopen
    // finds an ordinary lane with ordinary exits instead of a lifecycle no
    // writer can touch (round-2 review P2-A).
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Idle
    );
}

/// Dormant input is refused for a running run's Blocked member and tolerated
/// (recorded) only where the Close explicitly asks for it: the proof is one
/// function with one switch.
#[test]
fn dormant_continuation_is_refused_unless_the_close_tolerates_it() {
    use harness_core::agentfirm_api::{
        DriverHandoffState, NativeContinuationActivation, RuntimeResidency,
    };
    let fixture = drain_fixture("dormant-input-proof");
    let mut armed = agent_session(&fixture.store, MID_TURN_MEMBER);
    armed.lifecycle = AgentSessionStatus::Idle;
    armed.control_state.runtime_residency = RuntimeResidency::Detached;
    armed.control_state.activity = RuntimeActivity::Idle;
    armed.current_turn_id = None;
    armed.control_state.continuation.activation = NativeContinuationActivation::Armed {
        runtime_generation: armed.runtime_generation,
        driver_generation: armed.control_state.driver_generation,
    };
    let refused = crate::lane_termination_proof(&fixture.store, DRAIN_SPACE_ID, &armed, false)
        .expect("proof reads");
    assert!(
        refused
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("armed native continuation")),
        "{:?}",
        refused.blocker
    );
    assert!(refused.dormant_residue.is_empty());
    let tolerated = crate::lane_termination_proof(&fixture.store, DRAIN_SPACE_ID, &armed, true)
        .expect("proof reads");
    assert!(tolerated.blocker.is_none(), "{:?}", tolerated.blocker);
    assert_eq!(tolerated.dormant_residue.len(), 1);
    assert!(tolerated.dormant_residue[0].contains("armed native continuation"));
    // A handoff in progress is never tolerated.
    let mut handing_off = armed.clone();
    handing_off.control_state.handoff_state = DriverHandoffState::PreparingHostToProvider;
    let refused = crate::lane_termination_proof(&fixture.store, DRAIN_SPACE_ID, &handing_off, true)
        .expect("proof reads");
    assert!(
        refused
            .blocker
            .as_deref()
            .is_some_and(|blocker| blocker.contains("mid driver handoff")),
        "{:?}",
        refused.blocker
    );
}

/// The projection planner reaches `Active` from `RecoveryRequired` only through
/// the Store-proved `Idle` hop. The production path is the adoption seam, which
/// performs that hop before any process-open command is admitted; this pins
/// the planner arm and the fence behind it: while the lane is not proven gone
/// the projection is refused with the named constant and nothing is written.
#[test]
fn recovery_required_lane_reaches_active_only_through_the_proved_idle_hop() {
    let fixture = drain_fixture("rr-first-cycle");
    let ledger = fixture.supervise("supervisor-rr-first-cycle-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr-first-cycle:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");

    // Still attached: the hop is refused, so the cycle never resumes on top of
    // a runtime that is not provably gone.
    let refused =
        crate::transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Active)
            .expect_err("an attached RecoveryRequired lane cannot start a cycle");
    assert!(
        refused.to_string().contains(
            harness_core::agentfirm_api::AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE
        ),
        "{refused}"
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );

    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("the reaped process detaches the lane");
    crate::transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Active)
        .expect("a reconciled lane reaches Active through the proved Idle hop");
    let lane = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(lane.lifecycle, AgentSessionStatus::Active);
    assert!(lane.current_turn_id.is_some());
}

/// The Close hop sits behind the authority and generation gates: a Close that
/// this Supervisor generation may not perform is refused before the lane is
/// touched, so it leaves no durable trace (round-3 review P2).
#[test]
fn close_member_for_recovery_leaves_the_lane_untouched_when_its_authority_is_fenced() {
    let fixture = drain_fixture("rr-close-fenced");
    let ledger = fixture.supervise("supervisor-rr-close-fenced-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr-close-fenced:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("the reaped process detaches the lane");
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the blocked member");
    let mut stale_lease = fixture
        .store
        .latest_team_supervisor_lease(&fixture.run_id)
        .expect("lease read")
        .expect("the fixture Supervisor holds the lease");
    stale_lease.generation += 1;

    let error = crate::close_detached_blocked_member_for_recovery(
        &fixture.store,
        &fixture.run_id,
        &blocked,
        &stale_lease,
        "host",
        "a Close from the wrong Supervisor generation",
    )
    .expect_err("a Close outside the exact Supervisor generation is refused");
    assert!(
        error
            .to_string()
            .contains("DETACHED_MEMBER_RECOVERY_FENCED"),
        "{error}"
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "the refused Close performed no hop"
    );
    let after = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    assert_eq!(after.status, MemberRunStatus::Blocked);
    assert!(after.coordination_is_active());
}

/// The adoption seam performs the same hop for a reconciled `RecoveryRequired`
/// lane that it performs for a drained `Interrupted` one (round-4 review
/// P2-R4): the drain skips the lane as already settled, the successor
/// generation reattaches it, and it is `Idle` on that generation before any
/// process-open command is prepared for its member.
#[test]
fn readoption_hops_a_reconciled_recovery_required_lane_to_idle() {
    let fixture = drain_fixture("rr-readopt");
    let ledger = fixture.supervise("supervisor-rr-readopt-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:rr-readopt:1");
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(
        &ledger,
        &member,
        AgentSessionStatus::RecoveryRequired,
    )
    .expect("the runner's RecoveryRequired write lands");
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        harness_core::agentfirm_api::RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("the reaped process detaches the lane");
    let reconciled = agent_session(&fixture.store, MID_TURN_MEMBER);
    drop(ledger);

    // The drain has nothing left to settle on this lane and leaves it as it
    // stands, so it outlives the daemon generation that owned it.
    fixture.drain("supervisor-rr-readopt-1", 1);
    let survived = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(survived.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert_eq!(
        survived.version, reconciled.version,
        "the drain skips a settled lane"
    );

    let successor_generation = fixture.readopt();
    let adopted = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(
        adopted.lifecycle,
        AgentSessionStatus::Idle,
        "the adoption seam hops the reattached lane before any provider effect"
    );
    assert_eq!(adopted.node_daemon_generation, successor_generation);
    assert_eq!(adopted.runtime_generation, reconciled.runtime_generation);
    assert!(adopted.current_turn_id.is_none());
    assert_eq!(
        adopted
            .native_session_ref
            .as_ref()
            .map(|native| native.native_session_id.as_str()),
        reconciled
            .native_session_ref
            .as_ref()
            .map(|native| native.native_session_id.as_str()),
        "the hop keeps the provider-native session identity"
    );
    // The ledger tells a reconciliation after a runner failure apart from a
    // drain recovery (round-5 review P3-R5-a).
    let operations = fixture
        .store
        .canonical_operations()
        .expect("canonical operations");
    assert!(
        operations.iter().any(|operation| {
            operation.event.aggregate_id == adopted.id
                && operation
                    .event
                    .idempotency_key
                    .starts_with("session-recovery-resume:")
        }),
        "the adoption hop of a reconciled lane is recorded as a recovery resume"
    );
    assert!(
        !operations.iter().any(|operation| {
            operation.event.aggregate_id == adopted.id
                && operation
                    .event
                    .idempotency_key
                    .starts_with("session-drain-resume:")
        }),
        "no drain recovery is recorded for a lane the drain skipped"
    );
}
