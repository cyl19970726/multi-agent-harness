use super::*;

#[test]
fn runtime_control_rejects_missing_turn_and_requires_explicit_binding_release_before_stop() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-runtime-control", 0),
            identity("runtime-control"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "runtime-control-session", 0),
            session("session-runtime-control", "runtime-control"),
        )
        .unwrap();

    let daemon = ActorRef {
        kind: ActorKind::Service,
        id: "daemon-1".into(),
    };
    let cancel_payload = serde_json::json!({
        "session_id": "session-runtime-control",
        "session_generation": 1,
        "delivery_id": "control-cancel",
    });
    let cancel = ControlCommandEnvelope {
        id: "runtime-control-cancel".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: daemon.clone(),
        command: RuntimeCommandKind::CancelProviderTurn,
        required_capability: "provider.cancel".into(),
        idempotency_key: "runtime-control-cancel".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-runtime-control"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&cancel_payload),
        payload: cancel_payload,
        issued_at: "t2".into(),
    };
    let mut cancel_context = service_context(
        "node_daemon.provider_effect.prepare",
        "runtime-control-cancel",
        0,
    );
    cancel_context.authority_actor = Some(daemon.clone());
    cancel_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&cancel).unwrap());
    let operations_before_cancel = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(&cancel_context, &cancel, current_unix_ms(), "t2")
        .expect_err("an idle session has no provider turn to cancel");
    assert!(error.to_string().contains("exact active provider turn"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_cancel
    );

    let binding = WorkExecutionBinding {
        id: "binding-runtime-control".into(),
        work_id: "work-runtime-control".into(),
        work_revision: 1,
        team_id: "team-runtime-control".into(),
        team_membership_id: "membership-runtime-control".into(),
        agent_member_id: "runtime-control".into(),
        agent_session_id: "session-runtime-control".into(),
        agent_session_generation: 1,
        delivery_id: "work-delivery-runtime-control".into(),
        binding_generation: 1,
        status: WorkExecutionBindingStatus::Active,
        version: 1,
        created_by: actor("host"),
        bound_at: "t2".into(),
        ended_at: None,
    };
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &context("host", "binding.test_fixture", "binding-runtime-control", 0),
                "work_execution_binding",
                &binding.id,
                "bound",
                serde_json::to_value(&binding).unwrap(),
                &binding,
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
    }
    let stop_payload = serde_json::json!({
        "session_id": "session-runtime-control",
        "session_generation": 1,
        "delivery_id": "control-stop",
    });
    let stop = ControlCommandEnvelope {
        id: "runtime-control-stop".into(),
        execution_space_id: "space-test".into(),
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        target_node_daemon_id: "daemon-1".into(),
        target_node_daemon_generation: 1,
        authenticated_actor: daemon.clone(),
        command: RuntimeCommandKind::StopSession,
        required_capability: "agent_session.stop".into(),
        idempotency_key: "runtime-control-stop".into(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms() + 60_000,
        binding: test_runtime_binding("session-runtime-control"),
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: canonical_json_fingerprint(&stop_payload),
        payload: stop_payload,
        issued_at: "t3".into(),
    };
    let mut stop_context = service_context(
        "node_daemon.provider_effect.prepare",
        "runtime-control-stop",
        0,
    );
    stop_context.authority_actor = Some(daemon);
    stop_context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&stop).unwrap());
    let operations_before_stop = store.canonical_operations().unwrap();
    let stop_error = store
        .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t3")
        .expect_err("StopSession cannot silently rewrite an active Work binding");
    assert!(stop_error
        .to_string()
        .contains("WORK_EXECUTION_BINDING_ACTIVE"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_stop
    );
    let active = store.fabric_work_execution_bindings("space-test").unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].status, WorkExecutionBindingStatus::Active);

    store
        .release_work_execution_binding(
            &context(
                "runtime-control",
                "work_binding.release",
                "binding-runtime-control-release",
                1,
            ),
            &binding.id,
            "t-release",
        )
        .expect("exact owner explicitly releases the binding");
    let stopped = store
        .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t4")
        .expect("StopSession is admitted after explicit release");
    assert_eq!(stopped.projection.status, RuntimeCommandStatus::Accepted);
    let released = store.fabric_work_execution_bindings("space-test").unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].status, WorkExecutionBindingStatus::Released);
    assert_eq!(released[0].ended_at.as_deref(), Some("t-release"));
    fs::remove_dir_all(root).unwrap();
}
