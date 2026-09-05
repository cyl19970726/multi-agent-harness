//! #756: what happens to a member's in-flight Work across a NodeDaemon drain.
//!
//! DEV-171 (#748) proved the drained Session can resume. This proves the Work
//! it was mid-turn on comes back with it: the killed generation's binding is
//! invalidated with a recorded cause, its provider-received delivery is
//! superseded with an explicit failure code, and the ordinary dispatch path
//! re-delivers the same Work under the successor generation as a brand new
//! delivery and claim — while the killed StartCycle stays exactly one settled
//! row on the dead generation.

use super::drain_recovery_tests::{
    agent_session, drain_fixture, member_named, DrainFixture, DRAIN_SPACE_ID, IDLE_MEMBER,
    MID_TURN_MEMBER,
};
use crate::claim_canonical_work_for_member;
use harness_core::agentfirm_api::{
    AgentSessionStatus, CanonicalWorkDelivery, RuntimeCommandStatus, RuntimeEffectCertainty,
    WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};
use harness_core::{CurrentWorkDraft, Work, WorkClaimMode, WorkCommandContext, WorkPriority};

pub(super) fn assign_work(
    fixture: &DrainFixture,
    agent_member_id: &str,
    suffix: &str,
    title: &str,
    created_at: &str,
) -> Work {
    let run = crate::latest_team_run(&fixture.store, &fixture.run_id).expect("TeamRun");
    let membership = fixture
        .store
        .fabric_team_memberships(DRAIN_SPACE_ID)
        .expect("Team memberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == run.agent_team_id && membership.agent_member_id == agent_member_id
        })
        .expect("exact member TeamMembership");
    let host = crate::compatibility_team_actor("host", "test");
    let work = fixture
        .store
        .insert_work(
            {
                let mut draft = CurrentWorkDraft::new(
                    format!("drain-work-{suffix}"),
                    run.id.clone(),
                    run.agent_team_id.clone(),
                    title.into(),
                    "A real assigned Work is in flight when the drain arrives".into(),
                    "The provider receipt is canonical".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    host.clone(),
                    created_at.into(),
                );
                draft.eligible_member_ids = vec![agent_member_id.to_string()];
                draft.into_work()
            },
            WorkCommandContext {
                event_id: format!("drain-work-{suffix}-created"),
                performed_by_actor: host.clone(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("drain-work-{suffix}-create"),
                created_at: created_at.into(),
                duplicate_ok: false,
            },
        )
        .expect("create the Work");
    fixture
        .store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &membership.id,
            DRAIN_SPACE_ID,
            WorkCommandContext {
                event_id: format!("drain-work-{suffix}-assigned"),
                performed_by_actor: host,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("drain-work-{suffix}-assign"),
                created_at: created_at.into(),
                duplicate_ok: false,
            },
        )
        .expect("assign stable TeamMembership responsibility")
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

fn delivery(fixture: &DrainFixture, delivery_id: &str) -> CanonicalWorkDelivery {
    fixture
        .store
        .fabric_work_deliveries(DRAIN_SPACE_ID)
        .expect("deliveries")
        .into_iter()
        .find(|delivery| delivery.id == delivery_id)
        .expect("canonical WorkDelivery")
}

#[test]
fn drained_in_flight_work_is_redelivered_under_the_successor_generation() {
    let fixture = drain_fixture("drain-inflight-work");
    let ledger = fixture.supervise("supervisor-work-1", fixture.daemon_generation);
    let drained_work = assign_work(
        &fixture,
        MID_TURN_MEMBER,
        "in-flight",
        "Work the member is mid-turn on",
        "unix-ms:3",
    );
    // A second ready Work for the same member proves the drain touches only the
    // lane it killed, not the member's whole backlog.
    let other_work = assign_work(
        &fixture,
        MID_TURN_MEMBER,
        "backlog",
        "Work that was never dispatched",
        "unix-ms:4",
    );

    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("dispatch one canonical Work")
        .expect("the oldest ready Work is dispatched first");
    assert_eq!(claimed.work.id, drained_work.id);
    ledger
        .complete_work_delivery(&claimed, "provider-receipt:drain-in-flight")
        .expect("the provider receives the Work before the drain");
    // The member is mid-turn on exactly that delivery when the drain arrives.
    fixture.start_cycle_for(&ledger, &claimed.delivery.id);
    fixture.idle_one_member(&ledger);
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    let killed_binding = bindings_for(&fixture, &drained_work.id)
        .pop()
        .expect("one binding for the in-flight Work");
    assert_eq!(killed_binding.status, WorkExecutionBindingStatus::Active);
    assert_eq!(
        delivery(&fixture, &claimed.delivery.id).status,
        WorkDeliveryStatus::ProviderReceived
    );
    assert!(bindings_for(&fixture, &other_work.id).is_empty());
    drop(ledger);

    fixture.drain("supervisor-work-1", 1);

    // The killed generation's Work authority is ended with a recorded cause,
    // and its delivery says the attempt was superseded — never that the turn
    // completed.
    let settled_binding = bindings_for(&fixture, &drained_work.id)
        .pop()
        .expect("the killed binding");
    assert_eq!(
        settled_binding.status,
        WorkExecutionBindingStatus::Released,
        "an Active, judged-current binding would never be re-dispatched"
    );
    let superseded = delivery(&fixture, &claimed.delivery.id);
    assert_eq!(superseded.status, WorkDeliveryStatus::Failed);
    assert_eq!(
        superseded.failure_code.as_deref(),
        Some(harness_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN)
    );
    assert_eq!(
        superseded.provider_receipt_id.as_deref(),
        Some("provider-receipt:drain-in-flight"),
        "the provider receipt stays immutable evidence of what did cross the boundary"
    );
    let invalidation = fixture
        .store
        .canonical_operations()
        .expect("canonical operations")
        .into_iter()
        .map(|operation| operation.event)
        .find(|event| {
            event.aggregate_kind == "work_execution_binding"
                && event.aggregate_id == killed_binding.id
                && event.transition == "invalidated_by_lost_runtime_generation"
        })
        .expect("the drain records why the binding ended");
    assert_eq!(
        invalidation.payload["lost_runtime_generation"]["cause"],
        serde_json::json!("node_daemon_drain")
    );

    // The successor generation re-adopts and the member resumes, exactly as
    // DEV-171 proved.
    let successor_generation = fixture.readopt();
    let ledger = fixture.supervise("supervisor-work-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(&ledger, &mid_turn, AgentSessionStatus::Idle)
        .expect("a drained member resumes without INVALID_STATE_TRANSITION");
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Idle
    );

    // The ordinary dispatch path re-delivers the same Work under the successor
    // generation: a new binding generation, a new delivery, a new claim.
    let redelivered = claim_canonical_work_for_member(&ledger, &mid_turn)
        .expect("the ordinary dispatch pass runs")
        .expect("the drained Work returns to the ordinary dispatch path");
    assert_eq!(redelivered.work.id, drained_work.id);
    assert_ne!(redelivered.delivery.id, claimed.delivery.id);
    assert_ne!(redelivered.delivery.claim_id, claimed.delivery.claim_id);
    assert_eq!(
        redelivered.delivery.claimed_node_daemon_generation,
        Some(successor_generation)
    );
    let generations = bindings_for(&fixture, &drained_work.id)
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

    // Settlement plus ordinary re-delivery minted no extra StartCycle row,
    // and the killed cycle stays the one settled terminal fact on the dead
    // daemon generation. This does NOT prove a later driven cycle would not
    // replay — no cycle is driven after readopt, so that question is simply
    // not exercised here.
    let after = fixture.start_cycles();
    assert_eq!(
        after.len(),
        1,
        "settlement plus re-delivery must not mint an extra StartCycle row"
    );
    assert_eq!(after[0].id, killed_cycle[0].id);
    assert_eq!(after[0].status, RuntimeCommandStatus::Applied);
    assert_eq!(after[0].effect_certainty, RuntimeEffectCertainty::Applied);
    assert_eq!(
        after[0].target_node_daemon_generation, fixture.daemon_generation,
        "the killed cycle stays bound to the dead daemon generation"
    );

    // The member's other Work is untouched by the drain and still dispatchable.
    let backlog = fixture
        .store
        .latest_works()
        .expect("works")
        .into_iter()
        .find(|work| work.id == other_work.id)
        .expect("the never-dispatched Work");
    assert_eq!(backlog.version, other_work.version);
    assert_eq!(backlog.phase, other_work.phase);
    assert_eq!(
        agent_session(&fixture.store, IDLE_MEMBER).lifecycle,
        AgentSessionStatus::Idle,
        "a member idle at drain time is unaffected"
    );
}
