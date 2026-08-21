use super::*;

#[test]
fn agent_session_reattach_preserves_native_identity_and_fences_daemon_driver() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "reattach-agent", 0),
            identity("reattach-agent"),
        )
        .unwrap();
    let mut target = session("session-reattach", "reattach-agent");
    target.control_state.runtime_residency = RuntimeResidency::Attached;
    target.control_state.activity = RuntimeActivity::Idle;
    target.native_session_ref = Some(NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "thread-native-1".into(),
        native_locator_kind: "codex_thread".into(),
        provider_version: Some("0.148.0-alpha.9".into()),
        adapter_contract_version: "codex-app-server-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t1".into()),
        parent_native_session_id: None,
    });
    store
        .create_agent_session(
            &service_context("session.create", "session-reattach", 0),
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
        idempotency_key: "reattach-session-1".into(),
        expected_version: target.version,
        request_fingerprint: None,
    };
    let moved = store
        .reattach_agent_session_to_node_daemon(
            &reattach_context,
            &target.id,
            target.runtime_generation,
            1,
            &successor.daemon_id,
            successor.generation,
            "t2",
        )
        .unwrap();
    assert_eq!(
        moved.projection.runtime_generation,
        target.runtime_generation
    );
    assert_eq!(
        moved.projection.native_session_ref,
        target.native_session_ref
    );
    assert_eq!(
        moved.projection.node_daemon_generation,
        successor.generation
    );
    assert_eq!(moved.projection.control_state.driver_generation, 2);
    assert_eq!(
        moved.projection.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert_eq!(
        moved.projection.control_state.driver_ref,
        RuntimeDriverRef::NodeDaemon {
            node_daemon_id: successor.daemon_id,
            node_daemon_generation: successor.generation,
        }
    );
    let replay = store
        .reattach_agent_session_to_node_daemon(
            &reattach_context,
            &target.id,
            target.runtime_generation,
            1,
            "daemon-2",
            2,
            "t2",
        )
        .unwrap();
    assert!(replay.replayed);
    fs::remove_dir_all(root).unwrap();
}
