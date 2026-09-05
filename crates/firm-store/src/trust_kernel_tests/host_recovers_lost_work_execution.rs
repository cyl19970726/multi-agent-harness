use super::host_redelivers_open_work_after_member_close_and_reopen::{
    deliver_to_provider, host_context, member_context, reopened_member_fixture,
};
use super::work_responsibility_execution_admission_is_exact_and_idempotent::assign_responsibility;
use super::*;

/// GitHub #734 / #799 (DEV-230): a Work whose WorkExecutionBinding is frozen
/// on a MemberRun generation that advanced without a clean Close has no
/// honest exit — the daemon's stale reconciliation refuses a provider-received
/// delivery, `redeliver` refuses a started Work, and `release` refuses a
/// non-open one. `recover_lost_work_execution` is the explicit Host authority
/// for exactly that state, and it fails closed while the binding's runtime
/// authority is still current.
fn current_work(store: &HarnessStore, work_id: &str) -> firm_core::Work {
    store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("Work")
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

fn recovery_operations(store: &HarnessStore, work_id: &str) -> Vec<firm_core::WorkOperation> {
    store
        .work_operations()
        .unwrap()
        .into_iter()
        .filter(|operation| {
            operation.work.id == work_id
                && operation.event.kind == firm_core::WorkEventKind::ExecutionRecovered
        })
        .collect()
}

/// The non-clean generation advance #734 describes: the MemberRun epoch moves
/// 1 -> 2 through the generic Supervisor recovery path, with no Close request,
/// no CloseMember provider effect, and no binding release.
fn advance_member_generation_without_close(store: &HarnessStore, member_run_id: &str) {
    let predecessor = store
        .member_runs()
        .unwrap()
        .into_iter()
        .find(|member| member.id == member_run_id)
        .expect("ProviderRuntimeProjection");
    let mut advanced = predecessor.clone();
    advanced.runtime_generation += 1;
    advanced.status = MemberRunStatus::Idle;
    advanced.started_at = "t-advance".into();
    store
        .compare_and_advance_member_run_generation(&predecessor, &advanced)
        .expect("generic recovery advances the generation without a Close");
}

#[test]
fn host_recovers_started_work_whose_binding_is_frozen_on_a_superseded_generation() {
    let (fixture, root) = reopened_member_fixture("recover-lost");
    let store = &fixture.store;
    let work = assign_responsibility(store, "work-recover-lost", &fixture.membership.id);
    let binding = deliver_to_provider(&fixture, &work, "recover-lost");
    let started = store
        .start_work(
            &work.id,
            work.version,
            &fixture.member_run_id,
            member_context(
                &fixture.member_run_id,
                "start-recover-lost",
                "start-recover-lost",
            ),
        )
        .expect("the member starts its provider-received Work");
    assert_eq!(started.phase, firm_core::WorkPhase::Active);

    // While the binding's exact generations are still the member's current
    // runtime authority, recovery is refused with zero writes.
    let live = store
        .recover_lost_work_execution(
            &work.id,
            started.version,
            "space-test",
            Some("too early"),
            host_context(store, "recover-live", "recover-live"),
        )
        .expect_err("a live execution binding is not a lost execution");
    let live = live.to_string();
    assert!(live.contains("WORK_EXECUTION_AUTHORITY_LIVE"), "{live}");
    assert!(live.contains(&binding.id), "{live}");
    assert!(live.contains("generation 1"), "{live}");
    assert_eq!(current_work(store, &work.id).version, started.version);
    assert_eq!(
        current_binding(store, &binding.id).status,
        WorkExecutionBindingStatus::Active
    );
    assert!(recovery_operations(store, &work.id).is_empty());

    advance_member_generation_without_close(store, &fixture.member_run_id);

    // The exact #734 dead end. The daemon's stale reconciliation sees the
    // binding is no longer current but refuses to release a provider-received
    // delivery; every Work verb refuses a started Work.
    let stale = store
        .release_work_execution_binding_if_stale(
            &service_context(
                "node_daemon.work_execution_binding.release_if_stale",
                "stale-recover-lost",
                binding.version,
            ),
            &binding.id,
            &fixture.session.node_id,
            &fixture.session.node_daemon_id,
            fixture.session.node_daemon_generation,
            "t-stale",
        )
        .expect_err("a provider-received delivery is a recovery fence for the daemon");
    let stale = stale.to_string();
    assert!(
        stale.contains(
            "provider-received WorkDelivery must reach an explicit terminal provider outcome"
        ),
        "{stale}"
    );
    assert_eq!(
        current_binding(store, &binding.id).status,
        WorkExecutionBindingStatus::Active
    );
    let redeliver = store
        .redeliver_work_to_current_session(
            &work.id,
            started.version,
            "space-test",
            None,
            host_context(store, "redeliver-recover-lost", "redeliver-recover-lost"),
        )
        .expect_err("redeliver refuses a started Work")
        .to_string();
    assert!(redeliver.contains("WORK_ALREADY_STARTED"), "{redeliver}");
    let release = store
        .release_work_as_host(
            &work.id,
            started.version,
            host_context(store, "release-recover-lost", "release-recover-lost"),
        )
        .expect_err("release refuses a started Work")
        .to_string();
    assert!(release.contains("must be open to release"), "{release}");

    // The Host recovery: the binding is released through the lost-generation
    // writer with the receipt as evidence, and the Work returns to Open with
    // its responsibility intact and its revision advanced.
    let recovered = store
        .recover_lost_work_execution(
            &work.id,
            started.version,
            "space-test",
            Some("member generation advanced without a clean Close"),
            host_context(store, "recover-lost", "recover-lost"),
        )
        .expect("a provably superseded generation is a lost execution");
    assert_eq!(recovered.phase, firm_core::WorkPhase::Open);
    assert_eq!(recovered.condition, firm_core::WorkCondition::Normal);
    assert_eq!(recovered.resolution, None);
    assert_eq!(recovered.version, started.version + 1);
    assert_eq!(recovered.owner_member_id, started.owner_member_id);
    assert_eq!(
        recovered.assignee_membership_id,
        started.assignee_membership_id
    );

    let released = current_binding(store, &binding.id);
    assert_eq!(released.status, WorkExecutionBindingStatus::Released);
    assert_eq!(released.ended_at.as_deref(), Some("t-redeliver"));
    let superseded = current_delivery(store, &binding.delivery_id);
    assert_eq!(superseded.status, WorkDeliveryStatus::Failed);
    assert_eq!(
        superseded.failure_code.as_deref(),
        Some(firm_core::agentfirm_api::WORK_DELIVERY_SUPERSEDED_BY_HOST_LOST_EXECUTION_RECOVERY)
    );
    assert_eq!(
        superseded.provider_receipt_id.as_deref(),
        Some("provider-receipt-recover-lost"),
        "the provider receipt stays immutable evidence of what crossed the boundary"
    );
    let events = binding_events(store, &binding.id);
    let ended = events.last().expect("the binding end event");
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(ended.transition, "invalidated_by_lost_runtime_generation");
    assert_eq!(
        ended.payload["lost_runtime_generation"]["cause"],
        serde_json::json!("host_lost_execution_recovery")
    );
    assert_eq!(
        ended.payload["lost_runtime_generation"]["evidence"]["causes"],
        serde_json::json!(["member_run_generation_superseded"])
    );
    assert_eq!(
        ended.payload["lost_runtime_generation"]["evidence"]["member_run_generation_now"],
        serde_json::json!(2)
    );
    assert_eq!(
        ended.payload["superseded_delivery"]["status_before_supersession"],
        serde_json::json!("provider_received")
    );
    assert_eq!(
        ended.payload["superseded_delivery"]["provider_receipt_id"],
        serde_json::json!("provider-receipt-recover-lost")
    );

    let operations = recovery_operations(store, &work.id);
    let [operation] = operations.as_slice() else {
        panic!("expected exactly one ExecutionRecovered operation: {operations:?}");
    };
    let payload = &operation.event.payload;
    assert_eq!(payload["recovery"], serde_json::json!("lost_execution"));
    assert_eq!(payload["phase_before"], serde_json::json!("active"));
    assert_eq!(
        payload["lost_execution"]["causes"],
        serde_json::json!(["member_run_generation_superseded"])
    );
    assert_eq!(
        payload["lost_execution"]["member_run_generation_at_binding"],
        serde_json::json!(1)
    );
    assert_eq!(
        payload["released_binding"]["id"],
        serde_json::json!(binding.id)
    );
    assert_eq!(
        payload["released_binding"]["status"],
        serde_json::json!("released")
    );
    let superseded_deliveries = payload["superseded_deliveries"]
        .as_array()
        .expect("superseded deliveries");
    assert_eq!(superseded_deliveries.len(), 1);
    assert_eq!(
        superseded_deliveries[0]["delivery_id"],
        serde_json::json!(binding.delivery_id)
    );
    assert_eq!(
        superseded_deliveries[0]["status"],
        serde_json::json!("provider_received"),
        "the payload records the delivery state the recovery superseded"
    );
    assert_eq!(
        superseded_deliveries[0]["stale_because"],
        serde_json::json!("work_execution_binding_released")
    );

    // The exact retry is idempotent.
    let replay = store
        .recover_lost_work_execution(
            &work.id,
            started.version,
            "space-test",
            Some("member generation advanced without a clean Close"),
            host_context(store, "recover-lost", "recover-lost"),
        )
        .expect("the exact retry is idempotent");
    assert_eq!(replay, recovered);
    assert_eq!(recovery_operations(store, &work.id).len(), 1);

    // The ordinary delivery path binds the recovered revision to the member's
    // current generation and produces a new WorkDelivery, exactly as after
    // `work assign`; the superseded row stays readable next to it.
    assert!(!store
        .provider_received_work_requires_host_reauthorization(
            "space-test",
            &work.id,
            recovered.version
        )
        .unwrap());
    let mut successor_runtime = fixture.runtime_binding.clone();
    successor_runtime.target_member_run_generation = Some(2);
    let mut successor = binding.clone();
    successor.id = "binding-recover-lost-2".into();
    successor.binding_generation = 2;
    successor.delivery_id = format!("work-delivery:{}:2", work.id);
    successor.work_revision = recovered.version;
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-recover-lost-2", 0),
            &successor_runtime,
            successor.clone(),
        )
        .expect("the current generation binds the recovered revision");
    let fresh = current_delivery(store, &successor.delivery_id);
    assert_eq!(fresh.status, WorkDeliveryStatus::Queued);
    assert_eq!(fresh.work_revision, recovered.version);
    assert_eq!(
        current_delivery(store, &binding.delivery_id).status,
        WorkDeliveryStatus::Failed,
        "the superseded delivery is preserved, never deleted"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn host_recovers_open_work_whose_live_binding_lost_its_generation() {
    let (fixture, root) = reopened_member_fixture("recover-open");
    let store = &fixture.store;
    let work = assign_responsibility(store, "work-recover-open", &fixture.membership.id);
    let binding = deliver_to_provider(&fixture, &work, "recover-open");
    advance_member_generation_without_close(store, &fixture.member_run_id);

    // `redeliver` still sees an executable binding and refuses (#734).
    let redeliver = store
        .redeliver_work_to_current_session(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(store, "redeliver-recover-open", "redeliver-recover-open"),
        )
        .expect_err("redeliver refuses while a binding is still executable")
        .to_string();
    assert!(redeliver.contains("WORK_DELIVERY_LIVE"), "{redeliver}");

    let recovered = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(store, "recover-open", "recover-open"),
        )
        .expect("an open Work with a provably dead binding is recoverable");
    assert_eq!(recovered.phase, firm_core::WorkPhase::Open);
    assert_eq!(recovered.version, work.version + 1);
    assert_eq!(
        current_binding(store, &binding.id).status,
        WorkExecutionBindingStatus::Released
    );
    assert_eq!(
        current_delivery(store, &binding.delivery_id).status,
        WorkDeliveryStatus::Failed
    );
    let operations = recovery_operations(store, &work.id);
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].event.payload["phase_before"],
        serde_json::json!("open")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn recover_lost_execution_fails_closed_when_nothing_is_provably_lost() {
    let (fixture, root) = reopened_member_fixture("recover-refusals");
    let store = &fixture.store;
    let work = assign_responsibility(store, "work-recover-refusals", &fixture.membership.id);

    let never_dispatched = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(
                store,
                "recover-never-dispatched",
                "recover-never-dispatched",
            ),
        )
        .expect_err("an assigned, never-dispatched Work has nothing to recover")
        .to_string();
    assert!(
        never_dispatched.contains("WORK_EXECUTION_NOT_LOST"),
        "{never_dispatched}"
    );
    assert!(
        never_dispatched.contains("work_was_never_dispatched"),
        "{never_dispatched}"
    );

    let wrong_space = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "foreign-space",
            None,
            host_context(store, "recover-wrong-space", "recover-wrong-space"),
        )
        .expect_err("caller scope must match the Work's canonical TeamRun scope")
        .to_string();
    assert!(
        wrong_space.contains("EXECUTION_SPACE_SCOPE_MISMATCH"),
        "{wrong_space}"
    );

    let member_actor = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            member_context(
                &fixture.member_run_id,
                "recover-member-actor",
                "recover-member-actor",
            ),
        )
        .expect_err("only the exact Host may recover a lost execution")
        .to_string();
    assert!(
        member_actor.contains("Host authority is required"),
        "{member_actor}"
    );

    // A delivered Work whose binding is exactly current is live, not lost —
    // even though the member never started it.
    let binding = deliver_to_provider(&fixture, &work, "recover-refusals");
    let live = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(store, "recover-live-open", "recover-live-open"),
        )
        .expect_err("a current binding is live")
        .to_string();
    assert!(live.contains("WORK_EXECUTION_AUTHORITY_LIVE"), "{live}");
    assert_eq!(
        current_binding(store, &binding.id).status,
        WorkExecutionBindingStatus::Active
    );
    assert_eq!(current_work(store, &work.id).version, work.version);
    assert!(recovery_operations(store, &work.id).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
