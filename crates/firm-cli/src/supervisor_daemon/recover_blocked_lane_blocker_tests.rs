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
