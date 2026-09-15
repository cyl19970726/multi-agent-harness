use super::*;

/// The driver generation is the fence that tells a provider effect which
/// driver is current. `saturating_add(1)` at `u64::MAX` would hand the
/// successor the predecessor's own generation, so every downstream driver
/// fence would then accept the stale driver as live. Reattach must refuse
/// instead, and refuse without committing anything.
#[test]
fn agent_session_driver_generation_fails_closed_at_the_ceiling() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "ceiling-agent", 0),
        "ceiling-agent",
    );
    let mut target = session("session-driver-ceiling", "ceiling-agent");
    // Detached with no native ref, so the reattach reaches the generation
    // arithmetic instead of stopping at the provider-drain receipt check.
    target.control_state.runtime_residency = RuntimeResidency::Detached;
    target.control_state.activity = RuntimeActivity::Idle;
    target.control_state.driver_generation = u64::MAX;
    target.native_session_ref = None;
    store
        .create_agent_session(
            &service_context("session.create", "session-driver-ceiling", 0),
            target.clone(),
        )
        .unwrap();

    let now = current_unix_ms();
    store
        .drain_node_daemon_lease(&target.node_id, "daemon-1", 1, "instance-1", now, 60_000)
        .unwrap();
    store
        .release_node_daemon_lease(&target.node_id, "daemon-1", 1, "instance-1", now + 1)
        .unwrap();
    let successor = store
        .acquire_node_daemon_lease(&target.node_id, "daemon-2", "instance-2", now + 2, 60_000)
        .unwrap();

    let reattach_context = MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: successor.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: "node_daemon.session.reattach".into(),
        idempotency_key: "reattach-driver-ceiling".into(),
        expected_version: target.version,
        request_fingerprint: None,
    };
    let before = store.canonical_operations().unwrap();
    let refusal = store
        .reattach_agent_session_to_node_daemon(
            &reattach_context,
            &target.id,
            target.runtime_generation,
            1,
            &successor.daemon_id,
            successor.generation,
            "t2",
        )
        .expect_err("a successor driver generation must not repeat the predecessor's");
    assert!(
        refusal
            .to_string()
            .contains("AGENT_SESSION_DRIVER_GENERATION_EXHAUSTED"),
        "unexpected refusal: {refusal}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        before,
        "a refused reattach must not commit a projection"
    );
    drop(root);
}
