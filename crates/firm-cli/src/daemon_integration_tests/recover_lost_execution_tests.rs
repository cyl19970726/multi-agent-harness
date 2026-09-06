//! #799 (DEV-230): a Work the member had *started* before a NodeDaemon drain.
//!
//! The drain invalidates the killed generation's binding and supersedes its
//! provider-received delivery (#756), but a started Work is not `Open`, so
//! the ordinary dispatch path never re-delivers it, the member cannot submit
//! without an Active binding, and `redeliver` / `release` refuse it. This
//! proves the explicit Host exit: `team-run recover` reports the Work as a
//! lost execution, `recover_lost_work_execution` returns it to `Open` with
//! the same assignee and an advanced revision, and the ordinary dispatch path
//! then re-delivers it under the successor generation as a brand new binding,
//! delivery and claim — after which the member can start it again.

use super::drain_inflight_work_tests::assign_work;
use super::drain_recovery_tests::{
    drain_fixture, member_named, DrainFixture, DRAIN_SPACE_ID, MID_TURN_MEMBER,
};
use crate::claim_canonical_work_for_member;
use harness_core::agentfirm_api::{
    AgentSessionStatus, WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};
use harness_core::{
    MemberRunStatus, TeamActorKind, TeamActorRef, Work, WorkCommandContext, WorkPhase,
};

fn member_context(member_run_id: &str, event: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: event.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: event.into(),
        created_at: "unix-ms:5".into(),
        duplicate_ok: false,
    }
}

fn host_context(fixture: &DrainFixture, event: &str) -> WorkCommandContext {
    let host = fixture
        .store
        .exact_team_run_host_actor(&fixture.run_id)
        .expect("the TeamRun has one exact Host actor");
    WorkCommandContext {
        event_id: event.into(),
        performed_by_actor: host.clone(),
        authority_actor: Some(host),
        causation_ref: None,
        idempotency_key: event.into(),
        created_at: "unix-ms:9".into(),
        duplicate_ok: false,
    }
}

fn current_work(fixture: &DrainFixture, work_id: &str) -> Work {
    fixture
        .store
        .latest_works()
        .expect("works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("Work")
}

fn bindings_for(fixture: &DrainFixture, work_id: &str) -> Vec<WorkExecutionBinding> {
    let mut bindings = fixture
        .store
        .fabric_work_execution_bindings(DRAIN_SPACE_ID)
        .expect("bindings")
        .into_iter()
        .filter(|binding| binding.work_id == work_id)
        .collect::<Vec<_>>();
    bindings.sort_by_key(|binding| binding.binding_generation);
    bindings
}

/// The member-side Work-plane start, as `member work start` does it.
fn member_starts(fixture: &DrainFixture, work: &Work, event: &str) -> Work {
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    if !matches!(
        member.status,
        MemberRunStatus::Idle | MemberRunStatus::Running
    ) {
        let mut running = member.clone();
        running.status = MemberRunStatus::Running;
        fixture
            .store
            .compare_and_append_member_run(&member, &running)
            .expect("the member is running its provider turn");
    }
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    fixture
        .store
        .start_work(
            &work.id,
            work.version,
            &member.id,
            member_context(&member.id, event),
        )
        .expect("the member starts its provider-received Work")
}

#[test]
fn started_work_lost_to_a_drain_is_recovered_by_the_host_and_redelivered() {
    let fixture = drain_fixture("recover-lost-execution");
    let ledger = fixture.supervise("supervisor-recover-1", fixture.daemon_generation);
    let work = assign_work(
        &fixture,
        MID_TURN_MEMBER,
        "recover",
        "Work the member started before the drain",
        "unix-ms:3",
    );
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("dispatch one canonical Work")
        .expect("the assigned Work is dispatched");
    assert_eq!(claimed.work.id, work.id);
    ledger
        .complete_work_delivery(&claimed, "provider-receipt:recover-1")
        .expect("the provider receives the Work before the drain");
    fixture.start_cycle_for(&ledger, &claimed.delivery.id);
    let started = member_starts(&fixture, &claimed.work, "start-before-drain");
    assert_eq!(started.phase, WorkPhase::Active);
    fixture.idle_one_member(&ledger);
    drop(ledger);

    fixture.drain("supervisor-recover-1", 1);

    // The drain ended the binding and superseded the delivery (#756), but the
    // started Work itself is stranded.
    let killed = bindings_for(&fixture, &work.id);
    assert_eq!(killed.len(), 1);
    assert_eq!(killed[0].status, WorkExecutionBindingStatus::Released);
    assert_eq!(current_work(&fixture, &work.id).phase, WorkPhase::Active);

    let successor_generation = fixture.readopt();
    let ledger = fixture.supervise("supervisor-recover-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(&ledger, &mid_turn, AgentSessionStatus::Idle)
        .expect("the drained member resumes under the successor generation");

    // The exact #799 dead end: the ordinary dispatch path never re-delivers a
    // started Work, so the resumed member sits idle with active Work assigned.
    assert!(
        claim_canonical_work_for_member(&ledger, &mid_turn)
            .expect("the ordinary dispatch pass runs")
            .is_none(),
        "a started Work is not Open, so ordinary dispatch cannot re-deliver it"
    );
    let stranded = current_work(&fixture, &work.id);
    let redeliver = fixture
        .store
        .redeliver_work_to_current_session(
            &work.id,
            stranded.version,
            DRAIN_SPACE_ID,
            None,
            host_context(&fixture, "redeliver-stranded"),
        )
        .expect_err("redeliver refuses a started Work")
        .to_string();
    assert!(redeliver.contains("WORK_ALREADY_STARTED"), "{redeliver}");

    // `team-run recover` reports the stranded Work instead of leaving the
    // Host to discover it from member complaints.
    let report =
        crate::team_run_recover(&fixture.store, &fixture.run_id, true).expect("team-run recover");
    let lost = report["lost_execution_works"]
        .as_array()
        .expect("lost_execution_works is reported");
    assert_eq!(lost.len(), 1, "{report}");
    assert_eq!(lost[0]["work_id"], serde_json::json!(work.id));
    assert_eq!(lost[0]["phase"], serde_json::json!("active"));
    assert_eq!(
        lost[0]["latest_binding_end_transition"],
        serde_json::json!("invalidated_by_lost_runtime_generation")
    );
    assert_eq!(
        lost[0]["causes"],
        serde_json::json!(["started_work_binding_released_by_lost_runtime_generation"])
    );
    assert!(lost[0]["executable_binding_id"].is_null());
    assert_eq!(report["lost_execution_scan_errors"], serde_json::json!([]));

    // The Host recovers: the Work returns to Open with the same assignee and
    // an advanced revision; nothing is replayed.
    let stranded = current_work(&fixture, &work.id);
    let recovered = fixture
        .store
        .recover_lost_work_execution(
            &work.id,
            stranded.version,
            DRAIN_SPACE_ID,
            Some("the drain killed the generation that was executing it"),
            host_context(&fixture, "recover-lost-execution"),
        )
        .expect("a started Work whose binding a settlement invalidated is recoverable");
    assert_eq!(recovered.phase, WorkPhase::Open);
    assert_eq!(recovered.version, stranded.version + 1);
    assert_eq!(recovered.owner_member_id, stranded.owner_member_id);
    assert_eq!(
        recovered.assignee_membership_id,
        stranded.assignee_membership_id
    );
    assert_eq!(
        bindings_for(&fixture, &work.id).len(),
        1,
        "recovery mints no binding"
    );

    // The ordinary dispatch path now re-delivers it under the successor
    // generation: a new binding generation, a new delivery, a new claim.
    let redelivered = claim_canonical_work_for_member(&ledger, &mid_turn)
        .expect("the ordinary dispatch pass runs")
        .expect("the recovered Work returns to the ordinary dispatch path");
    assert_eq!(redelivered.work.id, work.id);
    assert_eq!(redelivered.work.version, recovered.version);
    assert_ne!(redelivered.delivery.id, claimed.delivery.id);
    assert_ne!(redelivered.delivery.claim_id, claimed.delivery.claim_id);
    assert_eq!(
        redelivered.delivery.claimed_node_daemon_generation,
        Some(successor_generation)
    );
    let generations = bindings_for(&fixture, &work.id)
        .iter()
        .map(|binding| (binding.binding_generation, binding.status))
        .collect::<Vec<_>>();
    assert_eq!(
        generations,
        vec![
            (1, WorkExecutionBindingStatus::Released),
            (2, WorkExecutionBindingStatus::Active),
        ],
        "the fresh delivery generation is a new binding, never a revived one"
    );
    let superseded = fixture
        .store
        .fabric_work_deliveries(DRAIN_SPACE_ID)
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.id == claimed.delivery.id)
        .expect("the superseded delivery is preserved");
    assert_eq!(superseded.status, WorkDeliveryStatus::Failed);
    assert_eq!(
        superseded.provider_receipt_id.as_deref(),
        Some("provider-receipt:recover-1")
    );

    // And the member can start the Work again on the new delivery, so a
    // finished implementation is submitted through the ordinary path instead
    // of `cancel` plus a re-issued Work.
    ledger
        .complete_work_delivery(&redelivered, "provider-receipt:recover-2")
        .expect("the provider receives the recovered Work");
    let restarted = member_starts(&fixture, &redelivered.work, "start-after-recovery");
    assert_eq!(restarted.phase, WorkPhase::Active);
    assert_eq!(restarted.version, recovered.version + 1);

    // Recovery is not repeatable once the Work is executing again.
    let live = fixture
        .store
        .recover_lost_work_execution(
            &work.id,
            restarted.version,
            DRAIN_SPACE_ID,
            None,
            host_context(&fixture, "recover-again"),
        )
        .expect_err("a re-delivered, executing Work is live")
        .to_string();
    assert!(live.contains("WORK_EXECUTION_AUTHORITY_LIVE"), "{live}");
    let report =
        crate::team_run_recover(&fixture.store, &fixture.run_id, true).expect("team-run recover");
    assert_eq!(
        report["lost_execution_works"],
        serde_json::json!([]),
        "nothing is lost once the Work executes again"
    );
}
