use super::TempHome;
use harness_core::agentfirm_api::{ActorKind, ActorRef, MutationContext, RuntimeDispatchMode};

pub fn record_provider_received_work(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    key: &str,
) {
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let binding = store
        .fabric_work_execution_bindings(execution_space_id)
        .expect("read fixture WorkExecutionBindings")
        .into_iter()
        .find(|binding| {
            binding.work_id == work_id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .expect("fixture Work has one active execution binding");
    let delivery = store
        .fabric_work_deliveries(execution_space_id)
        .expect("read fixture Work deliveries")
        .into_iter()
        .find(|delivery| delivery.id == binding.delivery_id)
        .expect("fixture binding has its exact Work delivery");
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
    store
        .claim_work_for_provider(
            &context("test.work.claim", "claim"),
            &delivery.id,
            &daemon.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &claim_id,
            RuntimeDispatchMode::QueueOnly,
            "unix-ms:test-provider-claim",
        )
        .expect("claim fixture Work through exact NodeDaemon authority");
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
