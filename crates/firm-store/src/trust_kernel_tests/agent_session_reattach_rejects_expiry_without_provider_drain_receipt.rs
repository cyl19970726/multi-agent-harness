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
    target.control_state.runtime_residency = RuntimeResidency::Detached;
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
    let after_expiry = current_unix_ms() + 61_000;
    let operations_before = store.canonical_operations().unwrap();
    let foreign_error = store
        .acquire_node_daemon_lease(
            &target.node_id,
            "daemon-2",
            "instance-2",
            after_expiry,
            60_000,
        )
        .expect_err("expiry cannot create a successor generation");
    assert!(foreign_error
        .to_string()
        .contains("NODE_DAEMON_PREDECESSOR_RECOVERY_REQUIRED"));
    let same_instance_error = store
        .acquire_node_daemon_lease(
            &target.node_id,
            "daemon-1",
            "instance-1",
            after_expiry,
            60_000,
        )
        .expect_err("the expired same instance must settle, not reacquire");
    assert!(same_instance_error
        .to_string()
        .contains("NODE_DAEMON_PREDECESSOR_SETTLEMENT_REQUIRED"));
    assert_eq!(store.canonical_operations().unwrap(), operations_before);

    store
        .drain_node_daemon_lease(
            &target.node_id,
            "daemon-1",
            1,
            "instance-1",
            after_expiry,
            60_000,
        )
        .expect("same instance may drain its expired predecessor");
    store
        .release_node_daemon_lease(
            &target.node_id,
            "daemon-1",
            1,
            "instance-1",
            after_expiry + 1,
        )
        .expect("explicit predecessor settlement publishes Released");
    let successor = store
        .acquire_node_daemon_lease(
            &target.node_id,
            "daemon-2",
            "instance-2",
            after_expiry + 2,
            60_000,
        )
        .expect("successor is allowed only after explicit Released");
    store
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
        .expect("explicit predecessor release permits exact successor reattach");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hard_crash_recovery_requires_exact_operator_evidence_and_detaches_the_predecessor() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "crash-agent", 0),
            identity("crash-agent"),
        )
        .unwrap();
    let mut target = session("session-crash-recovery", "crash-agent");
    target.control_state.runtime_residency = RuntimeResidency::Attached;
    target.control_state.activity = RuntimeActivity::Idle;
    target.native_session_ref = Some(NativeSessionRef {
        provider: "claude".into(),
        execution_mode: "claude_agent_sdk".into(),
        native_session_id: "claude-native-crash".into(),
        native_locator_kind: "claude_session".into(),
        provider_version: None,
        adapter_contract_version: "claude-agent-sdk-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t1".into()),
        parent_native_session_id: None,
    });
    store
        .create_agent_session(
            &service_context("session.create", "session-crash-recovery", 0),
            target,
        )
        .unwrap();

    let recovery_time = current_unix_ms() + 61_000;
    let before = store.canonical_operations().unwrap();
    let mut foreign = context("host", "node_daemon.predecessor_recover", "foreign", 0);
    foreign.authenticated_actor = ActorRef {
        kind: ActorKind::Service,
        id: "foreign-node".into(),
    };
    let error = store
        .recover_node_daemon_predecessor(
            &foreign,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "instance-1",
            true,
            true,
            "operator-check:foreign",
            recovery_time,
            "t2",
        )
        .expect_err("foreign Service cannot recover this Node");
    assert!(error.to_string().contains("RECOVERY_UNAUTHORIZED"));
    assert_eq!(store.canonical_operations().unwrap(), before);

    let mut operator = context("host", "node_daemon.predecessor_recover", "exact", 0);
    operator.authenticated_actor = ActorRef {
        kind: ActorKind::Service,
        id: "11111111-1111-4111-8111-111111111111".into(),
    };
    let missing_evidence = store
        .recover_node_daemon_predecessor(
            &operator,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "instance-1",
            true,
            false,
            "",
            recovery_time,
            "t2",
        )
        .expect_err("partial external evidence remains fail closed");
    assert!(missing_evidence
        .to_string()
        .contains("RECOVERY_EVIDENCE_REQUIRED"));

    let released = store
        .recover_node_daemon_predecessor(
            &operator,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "instance-1",
            true,
            true,
            "operator-check:pid-absent+process-groups-esrch",
            recovery_time,
            "t2",
        )
        .expect("exact Operator evidence settles the hard-crash predecessor");
    assert_eq!(
        released.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    assert_eq!(released.sessions_detached, vec!["session-crash-recovery"]);
    assert!(released.sessions_already_settled.is_empty());
    let recovered = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == "session-crash-recovery")
        .unwrap();
    assert_eq!(
        recovered.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert!(recovered.current_turn_id.is_none());
    store
        .acquire_node_daemon_lease(
            &released.lease.node_id,
            "daemon-2",
            "instance-2",
            recovery_time + 1,
            60_000,
        )
        .expect("successor is admitted after explicit hard-crash settlement");
    fs::remove_dir_all(root).unwrap();
}
