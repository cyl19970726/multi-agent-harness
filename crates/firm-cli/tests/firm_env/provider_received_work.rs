use super::TempHome;
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, MutationContext, RuntimeDispatchMode, WorkDeliveryStatus,
};
use std::time::{Duration, Instant};

pub fn record_provider_received_work(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    key: &str,
) {
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let bindings = store
        .fabric_work_execution_bindings(execution_space_id)
        .expect("read fixture WorkExecutionBindings")
        .into_iter()
        .filter(|binding| {
            binding.work_id == work_id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .collect::<Vec<_>>();
    let [binding] = bindings.as_slice() else {
        panic!("fixture Work must have exactly one active execution binding")
    };
    let delivery_id = binding.delivery_id.clone();
    let delivery = exact_delivery(&store, execution_space_id, binding);
    let daemon = store
        .latest_node_daemon_lease(&delivery.target_node_id)
        .expect("read fixture NodeDaemon")
        .expect("fixture NodeDaemon is current");
    let context = |command_name: &str, suffix: &str| MutationContext {
        execution_space_id: execution_space_id.into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: command_name.into(),
        idempotency_key: format!("{key}:{suffix}"),
        expected_version: 0,
        request_fingerprint: None,
    };
    let claim_id = format!("{key}:claim");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let delivery = store
            .fabric_work_deliveries(execution_space_id)
            .expect("read fixture Work deliveries")
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .expect("fixture binding has its exact Work delivery");
        match delivery.status {
            WorkDeliveryStatus::ProviderReceived => return,
            WorkDeliveryStatus::Queued => {
                if let Err(error) = store.claim_work_for_provider(
                    &context("test.work.claim", "claim"),
                    &delivery.id,
                    &daemon.node_id,
                    &daemon.daemon_id,
                    daemon.generation,
                    &claim_id,
                    RuntimeDispatchMode::QueueOnly,
                    "unix-ms:test-provider-claim",
                ) {
                    let current = store
                        .fabric_work_deliveries(execution_space_id)
                        .expect("re-read fixture Work delivery after claim race")
                        .into_iter()
                        .find(|current| current.id == delivery.id)
                        .expect("fixture Work delivery after claim race");
                    assert_ne!(
                        current.status,
                        WorkDeliveryStatus::Queued,
                        "fixture Work claim failed without a competing canonical claim: {error}"
                    );
                    continue;
                }
            }
            WorkDeliveryStatus::Claimed => {
                if delivery.claim_id.as_deref() != Some(claim_id.as_str()) {
                    assert!(
                        Instant::now() < deadline,
                        "Supervisor claim did not reach ProviderReceived"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                store
                    .record_work_provider_receipt(
                        &context("test.work.receipt", "receipt"),
                        &delivery.id,
                        &daemon.node_id,
                        &daemon.daemon_id,
                        daemon.generation,
                        &claim_id,
                        &format!("provider-receipt:{key}"),
                        "unix-ms:test-provider-receipt",
                    )
                    .expect("record fixture provider receipt");
            }
            WorkDeliveryStatus::Failed => {
                panic!("fixture Work delivery failed before provider receipt")
            }
        }
    }
}

fn exact_delivery(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    binding: &harness_core::agentfirm_api::WorkExecutionBinding,
) -> harness_core::agentfirm_api::CanonicalWorkDelivery {
    let deliveries = store
        .fabric_work_deliveries(execution_space_id)
        .expect("read fixture Work deliveries")
        .into_iter()
        .filter(|delivery| {
            delivery.id == binding.delivery_id
                && delivery.work_execution_binding_id == binding.id
                && delivery.work_id == binding.work_id
                && delivery.work_revision == binding.work_revision
                && delivery.recipient_agent_member_id == binding.agent_member_id
                && delivery.recipient_session_id == binding.agent_session_id
                && delivery.recipient_session_generation == binding.agent_session_generation
        })
        .collect::<Vec<_>>();
    let [delivery] = deliveries.as_slice() else {
        panic!("fixture binding must have exactly one canonical delivery")
    };
    delivery.clone()
}
