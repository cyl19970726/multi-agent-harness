use super::*;

#[test]
fn agent_session_reattach_rejects_expiry_without_provider_drain_receipt() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "expired-agent", 0),
            identity("expired-agent"),
        )
        .unwrap();
    let mut target = session("session-expired-reattach", "expired-agent");
    target.control_state.runtime_residency = RuntimeResidency::Attached;
    target.control_state.activity = RuntimeActivity::Idle;
    target.native_session_ref = Some(NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "thread-native-expired".into(),
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
            &service_context("session.create", "session-expired-reattach", 0),
            target.clone(),
        )
        .unwrap();
    let successor = store
        .acquire_node_daemon_lease(
            &target.node_id,
            "daemon-2",
            "instance-2",
            current_unix_ms() + 61_000,
            60_000,
        )
        .unwrap();
    let before = store.canonical_operations().unwrap();
    let error = store
        .reattach_agent_session_to_node_daemon(
            &MutationContext {
                execution_space_id: "space-test".into(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: successor.daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "node_daemon.session.reattach".into(),
                idempotency_key: "reattach-expired-session".into(),
                expected_version: target.version,
                request_fingerprint: None,
            },
            &target.id,
            target.runtime_generation,
            1,
            &successor.daemon_id,
            successor.generation,
            "t2",
        )
        .expect_err("lease expiry is not a provider drain receipt");
    assert!(error
        .to_string()
        .contains("explicit predecessor NodeDaemon release"));
    assert_eq!(store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
