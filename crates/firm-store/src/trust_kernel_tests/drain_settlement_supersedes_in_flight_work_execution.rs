use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;

const NODE_ID: &str = "11111111-1111-4111-8111-111111111111";

fn daemon_context(
    daemon_id: &str,
    command: &str,
    key: &str,
    expected_version: u64,
) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon_id.into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version,
        request_fingerprint: None,
    }
}

fn runtime_binding_for(
    session: &AgentSession,
    member_run_id: &str,
    member_run_generation: u64,
) -> firm_core::agentfirm_api::RuntimeCommandBinding {
    let mut binding = runtime_command_fixture(
        &format!("runtime-{}", session.id),
        RuntimeCommandKind::StartCycle,
        session,
        "start_cycle",
    )
    .0
    .binding;
    binding.target_member_run_id = Some(member_run_id.into());
    binding.target_member_run_generation = Some(member_run_generation);
    binding
}

fn current_session(store: &HarnessStore, session_id: &str) -> AgentSession {
    store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .expect("AgentSession")
}

fn current_binding(store: &HarnessStore, binding_id: &str) -> WorkExecutionBinding {
    store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .into_iter()
        .find(|binding| binding.id == binding_id)
        .expect("WorkExecutionBinding")
}

fn current_delivery(store: &HarnessStore, delivery_id: &str) -> CanonicalWorkDelivery {
    store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == delivery_id)
        .expect("CanonicalWorkDelivery")
}

/// Every canonical event written against one WorkExecutionBinding, oldest first.
fn binding_events(
    store: &HarnessStore,
    binding_id: &str,
) -> Vec<firm_core::agentfirm_api::CanonicalMutationEvent> {
    store
        .canonical_operations()
        .unwrap()
        .into_iter()
        .map(|operation| operation.event)
        .filter(|event| {
            event.aggregate_kind == "work_execution_binding" && event.aggregate_id == binding_id
        })
        .collect()
}

/// One member lane: an AgentSession with a Work bound for execution on it.
struct Lane {
    session: AgentSession,
    member_run_id: String,
    work: firm_core::Work,
    binding: WorkExecutionBinding,
}

fn open_lane(store: &HarnessStore, name: &str, membership: &TeamMembership) -> Lane {
    let session = session(&format!("session-{name}"), name);
    store
        .create_agent_session(
            &daemon_context("daemon-1", "session.create", &format!("session-{name}"), 0),
            session.clone(),
        )
        .unwrap();
    let member_run_id = format!("member-run-{name}");
    admit_member_run(
        store,
        canonical_member_run(&member_run_id, name, "run-admission"),
    );
    let work = assign_responsibility(store, &format!("work-{name}"), &membership.id);
    let binding = execution_binding(&work, membership, &session, &format!("binding-{name}"));
    store
        .bind_responsible_work_execution(
            &daemon_context("daemon-1", "work.bind", &format!("binding-{name}"), 0),
            &runtime_binding_for(&session, &member_run_id, 1),
            binding.clone(),
        )
        .unwrap();
    Lane {
        session,
        member_run_id,
        work,
        binding,
    }
}

/// Drive one lane's delivery to `ProviderReceived`: the provider has the work,
/// and only the member's own report could settle it.
fn hand_lane_to_provider(store: &HarnessStore, lane: &Lane, daemon_id: &str, generation: u64) {
    let name = &lane.binding.id;
    store
        .claim_work_for_provider(
            &daemon_context(daemon_id, "work.claim", &format!("claim-{name}"), 0),
            &lane.binding.delivery_id,
            NODE_ID,
            daemon_id,
            generation,
            &format!("claim-{name}"),
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim",
        )
        .unwrap();
    store
        .record_work_provider_receipt(
            &daemon_context(daemon_id, "work.receipt", &format!("receipt-{name}"), 0),
            &lane.binding.delivery_id,
            NODE_ID,
            daemon_id,
            generation,
            &format!("claim-{name}"),
            &format!("provider-receipt-{name}"),
            "t-receipt",
        )
        .unwrap();
}

fn drain_scope(store: &HarnessStore, name: &str) -> TeamMembership {
    append_runtime_team(store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                &format!("identity-{name}"),
                0,
            ),
            identity(name),
        )
        .unwrap();
    join_runtime_membership(
        store,
        &format!("membership-{name}"),
        "team-admission",
        name,
        TeamMembershipRole::Member,
    )
}

/// #756: a NodeDaemon drain kills this generation's owned provider process
/// groups, so the Work its members were mid-turn on can never be settled by
/// that generation. The settlement writer records that as an invalidated
/// binding plus a superseded delivery — never as a completed turn — and touches
/// nothing else.
#[test]
fn drain_settlement_supersedes_only_the_in_flight_work_it_killed() {
    let (store, _root) = fabric_store();
    let membership = drain_scope(&store, "drain-in-flight");
    let in_flight = open_lane(&store, "drain-in-flight", &membership);
    hand_lane_to_provider(&store, &in_flight, "daemon-1", 1);

    // A second Work for the same membership is bound but never dispatched: the
    // drain did not hand it to any provider, so it stays claimable.
    let queued_work = assign_responsibility(&store, "work-drain-queued", &membership.id);
    let queued_binding = {
        let mut binding = execution_binding(
            &queued_work,
            &membership,
            &in_flight.session,
            "binding-drain-queued",
        );
        binding.binding_generation = 1;
        binding.delivery_id = format!("work-delivery:{}:1", queued_work.id);
        binding
    };
    store
        .bind_responsible_work_execution(
            &daemon_context("daemon-1", "work.bind", "binding-drain-queued", 0),
            &runtime_binding_for(&in_flight.session, &in_flight.member_run_id, 1),
            queued_binding.clone(),
        )
        .unwrap();

    store
        .settle_node_daemon_shutdown_sessions(
            &daemon_context(
                "daemon-1",
                "node_daemon.shutdown.settle_sessions",
                "drain",
                1,
            ),
            NODE_ID,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain",
        )
        .expect("the exact daemon settles its own generation");

    let invalidated = current_binding(&store, &in_flight.binding.id);
    assert_eq!(
        invalidated.status,
        WorkExecutionBindingStatus::Released,
        "the killed generation's binding must not stay Active and current"
    );
    assert_eq!(invalidated.ended_at.as_deref(), Some("t-drain"));
    let events = binding_events(&store, &in_flight.binding.id);
    let [bound, ended] = events.as_slice() else {
        panic!("expected exactly one bind and one end event: {events:?}");
    };
    assert_eq!(bound.transition, "bound");
    assert_eq!(
        ended.transition, "invalidated_by_lost_runtime_generation",
        "the recorded reason must say the binding was invalidated, never that the turn completed"
    );
    assert_eq!(
        ended.payload["lost_runtime_generation"]["cause"],
        serde_json::json!("node_daemon_drain")
    );
    assert_eq!(
        ended.payload["lost_runtime_generation"]["evidence"]["provider_process_groups_terminated"],
        serde_json::json!(true)
    );
    assert_eq!(
        ended.payload["lost_runtime_generation"]["evidence"]["node_daemon_generation"],
        serde_json::json!(1)
    );
    assert_eq!(
        ended.payload["superseded_delivery"]["status_before_supersession"],
        serde_json::json!("provider_received"),
        "the event names what the delivery actually was when the drain cut it"
    );

    let superseded = current_delivery(&store, &in_flight.binding.delivery_id);
    assert_eq!(superseded.status, WorkDeliveryStatus::Failed);
    assert_eq!(
        superseded.failure_code.as_deref(),
        Some(firm_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN)
    );
    assert_eq!(
        superseded.provider_receipt_id.as_deref(),
        Some("provider-receipt-binding-drain-in-flight"),
        "the provider receipt stays immutable evidence of what crossed the boundary"
    );

    // The Work itself keeps its responsibility and revision, so the ordinary
    // dispatch path can mint a fresh binding generation for it.
    let work = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == in_flight.work.id)
        .unwrap();
    assert_eq!(work.phase, firm_core::WorkPhase::Open);
    assert_eq!(work.version, in_flight.work.version);
    assert_eq!(
        work.assignee_membership_id.as_deref(),
        Some(membership.id.as_str())
    );

    // The never-dispatched lane is untouched: no provider ever saw it, so the
    // drain has nothing to supersede and the successor generation can claim it.
    let queued = current_binding(&store, &queued_binding.id);
    assert_eq!(queued.status, WorkExecutionBindingStatus::Active);
    assert_eq!(
        current_delivery(&store, &queued_binding.delivery_id).status,
        WorkDeliveryStatus::Queued
    );
    assert_eq!(binding_events(&store, &queued_binding.id).len(), 1);
}

/// The settlement writer can only ever reach the exact generation it settles.
/// After the successor generation reattaches the Session and re-dispatches the
/// Work, the predecessor's settlement is fenced and the live generation's
/// in-flight binding is left alone.
#[test]
fn drain_settlement_cannot_reach_the_successor_generation_binding() {
    let (store, _root) = fabric_store();
    let membership = drain_scope(&store, "drain-in-flight");
    let lane = open_lane(&store, "drain-in-flight", &membership);
    hand_lane_to_provider(&store, &lane, "daemon-1", 1);
    store
        .settle_node_daemon_shutdown_sessions(
            &daemon_context(
                "daemon-1",
                "node_daemon.shutdown.settle_sessions",
                "drain",
                1,
            ),
            NODE_ID,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain",
        )
        .unwrap();
    store
        .release_node_daemon_lease(NODE_ID, "daemon-1", 1, "instance-1", current_unix_ms())
        .unwrap();
    let successor = store
        .acquire_node_daemon_lease(NODE_ID, "daemon-2", "instance-2", current_unix_ms(), 60_000)
        .unwrap();
    assert_eq!(successor.generation, 2);
    let drained_session = current_session(&store, &lane.session.id);
    store
        .reattach_agent_session_to_node_daemon(
            &daemon_context(
                "daemon-2",
                "session.reattach",
                "reattach-drain",
                drained_session.version,
            ),
            &lane.session.id,
            lane.session.runtime_generation,
            1,
            "daemon-2",
            successor.generation,
            "t-reattach",
        )
        .unwrap();

    // The ordinary dispatch path re-binds the same Work under the successor
    // generation: a new binding generation and a new delivery, never a replay
    // of the superseded one.
    let reattached = current_session(&store, &lane.session.id);
    let mut successor_binding = execution_binding(
        &lane.work,
        &membership,
        &reattached,
        "binding-drain-in-flight-2",
    );
    successor_binding.binding_generation = 2;
    successor_binding.delivery_id = format!("work-delivery:{}:2", lane.work.id);
    store
        .bind_responsible_work_execution(
            &daemon_context("daemon-2", "work.bind", "binding-drain-in-flight-2", 0),
            &runtime_binding_for(&reattached, &lane.member_run_id, 1),
            successor_binding.clone(),
        )
        .expect("the superseded attempt does not fence a fresh delivery generation");
    hand_lane_to_provider(
        &store,
        &Lane {
            session: reattached,
            member_run_id: lane.member_run_id.clone(),
            work: lane.work.clone(),
            binding: successor_binding.clone(),
        },
        "daemon-2",
        successor.generation,
    );

    let stale_settlement = store
        .settle_node_daemon_shutdown_sessions(
            &daemon_context(
                "daemon-1",
                "node_daemon.shutdown.settle_sessions",
                "drain-again",
                1,
            ),
            NODE_ID,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain-again",
        )
        .expect_err("a dead generation can never settle over the live one");
    assert!(
        stale_settlement
            .to_string()
            .contains("NODE_DAEMON_GENERATION_FENCED"),
        "{stale_settlement}"
    );
    let live = current_binding(&store, &successor_binding.id);
    assert_eq!(live.status, WorkExecutionBindingStatus::Active);
    assert_eq!(
        current_delivery(&store, &successor_binding.delivery_id).status,
        WorkDeliveryStatus::ProviderReceived
    );
}
