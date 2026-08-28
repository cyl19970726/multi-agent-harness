use super::*;

#[test]
fn runtime_cycle_correlation_is_exact_and_idempotent() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "cycle-correlation", 0),
            identity("cycle-correlation"),
        )
        .unwrap();
    let mut target = session("session-cycle-correlation", "cycle-correlation");
    target.lifecycle = AgentSessionStatus::Active;
    target.native_session_ref = None;
    store
        .create_agent_session(
            &service_context("session.create", "session-cycle-correlation", 0),
            target.clone(),
        )
        .unwrap();
    let (mut command, mut admission) = runtime_command_fixture(
        "runtime-cycle-correlation",
        RuntimeCommandKind::StartCycle,
        &target,
        "start-cycle",
    );
    command.payload["delivery_id"] = serde_json::json!("work-delivery:1:turn:1");
    command.payload_fingerprint = canonical_json_fingerprint(&command.payload);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let operations_before_missing_attempt = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-missing-attempt")
        .expect_err("StartCycle without a one-based provider attempt must write nothing");
    assert!(error.to_string().contains("one-based provider_attempt"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_missing_attempt
    );
    command.payload["provider_attempt"] = serde_json::json!(2);
    command.payload_fingerprint = canonical_json_fingerprint(&command.payload);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let accepted = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
        .unwrap();
    let attached = store
        .bind_agent_session_native_session(
            &service_context(
                "session.native.bind",
                "runtime-cycle-correlation:native-bind",
                target.version,
            ),
            &target.id,
            target.runtime_generation,
            settled_native_session("native-cycle-correlation"),
        )
        .expect("the provider may attach the first exact native session before input settlement");
    assert_eq!(
        attached
            .projection
            .native_session_ref
            .as_ref()
            .map(|native| native.native_session_id.as_str()),
        Some("native-cycle-correlation")
    );
    let settled = store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "node_daemon.provider_effect.settle",
                "runtime-cycle-correlation:settle",
                accepted.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({
                "phase": "input_accepted",
                "provider_receipt": {
                    "command": "deliver",
                    "response_id": "provider-receipt:1",
                    "success": true,
                },
            })),
            None,
            "t-input",
        )
        .unwrap();
    let correlation = ProviderCycleCorrelation {
        invocation_id: command.id.clone(),
        source_delivery_id: Some("work-delivery:1".into()),
        provider_input_id: "provider-input:1".into(),
        input_acceptance_receipt: "provider-receipt:1".into(),
        terminal_provider_input_id: Some("provider-input:1".into()),
        exact_terminal_ref: Some("provider-terminal:1".into()),
        native_session_id: "native-cycle-correlation".into(),
        agent_session_generation: target.runtime_generation,
        provider_attempt: 2,
    };
    let correlation_context = service_context(
        "node_daemon.provider_cycle.correlate",
        "runtime-cycle-correlation:terminal",
        settled.projection.version,
    );
    let operations_before_terminal = store.canonical_operations().unwrap();
    let mut missing_terminal = correlation.clone();
    missing_terminal.exact_terminal_ref = None;
    let missing_terminal_context = service_context(
        "node_daemon.provider_cycle.correlate",
        "runtime-cycle-correlation:missing-terminal",
        settled.projection.version,
    );
    let error = store
        .record_runtime_cycle_correlation(
            &missing_terminal_context,
            &command.id,
            &missing_terminal,
            "t-missing-terminal",
        )
        .expect_err("missing exact terminal identity must write nothing");
    assert!(error.to_string().contains("exact_terminal_ref"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_terminal
    );
    let mut crossed_before_write = correlation.clone();
    crossed_before_write.terminal_provider_input_id = Some("provider-input:old".into());
    let crossed_context = service_context(
        "node_daemon.provider_cycle.correlate",
        "runtime-cycle-correlation:crossed-terminal",
        settled.projection.version,
    );
    let error = store
        .record_runtime_cycle_correlation(
            &crossed_context,
            &command.id,
            &crossed_before_write,
            "t-crossed-before-write",
        )
        .expect_err("another provider input's terminal must write nothing");
    assert!(error.to_string().contains("different provider input"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_terminal
    );
    let observed = store
        .record_runtime_cycle_correlation(
            &correlation_context,
            &command.id,
            &correlation,
            "t-terminal",
        )
        .unwrap();
    assert_eq!(
        observed.projection.cycle_correlation,
        Some(correlation.clone())
    );
    let operations = store.canonical_operations().unwrap();
    let replay = store
        .record_runtime_cycle_correlation(
            &correlation_context,
            &command.id,
            &correlation,
            "t-replay",
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(store.canonical_operations().unwrap(), operations);

    let mut crossed = correlation;
    crossed.terminal_provider_input_id = Some("provider-input:old".into());
    let error = store
        .record_runtime_cycle_correlation(&correlation_context, &command.id, &crossed, "t-crossed")
        .expect_err("changed terminal under the same key must fail closed");
    assert!(error.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
    assert_eq!(store.canonical_operations().unwrap(), operations);
    fs::remove_dir_all(root).unwrap();
}
