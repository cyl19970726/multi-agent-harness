use super::*;

#[test]
fn legacy_agent_session_defaults_fail_closed_without_enabling_provider_driver() {
    let session: AgentSession = serde_json::from_value(serde_json::json!({
        "id": "session-1",
        "agent_member_id": "agent-1",
        "node_id": "node-1",
        "execution_space_id": "space-1",
        "node_daemon_id": "daemon-1",
        "node_daemon_generation": 1,
        "provider_kind": "codex",
        "provider_profile_ref": "profile-1",
        "permission_envelope_ref": "permission-1",
        "effective_permission_ceiling": "workspace_write",
        "lifecycle": "idle",
        "runtime_generation": 1,
        "queued_input_count": 0,
        "version": 1,
        "opened_at": "unix-ms:1",
        "last_active_at": "unix-ms:1"
    }))
    .expect("legacy AgentSession remains readable");

    assert_eq!(
        session.control_state.execution_driver,
        MemberExecutionDriver::HostDriven
    );
    assert_eq!(session.control_state.driver_generation, 0);
    assert_eq!(session.control_state.driver_ref, RuntimeDriverRef::Unknown);
    assert_eq!(
        session.control_state.continuation.activation,
        NativeContinuationActivation::Disarmed
    );
    assert_eq!(
        session.control_state.runtime_residency,
        RuntimeResidency::Unknown
    );
}

#[test]
fn legacy_runtime_command_keeps_phase_and_postcondition_unknown() {
    let command: RuntimeCommandRecord = serde_json::from_value(serde_json::json!({
            "id": "command-1",
            "execution_space_id": "space-1",
            "target_node_id": "node-1",
            "target_node_daemon_id": "daemon-1",
            "target_node_daemon_generation": 1,
            "authenticated_actor": {"kind": "service", "id": "daemon-1"},
            "command": "start_session",
            "required_capability": "agent_session.start",
            "idempotency_key": "start-session-1",
            "request_fingerprint": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "status": "requested",
            "effect_certainty": "none",
            "version": 1,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }))
        .expect("legacy RuntimeCommandRecord remains readable");

    assert_eq!(command.phase, RuntimeCommandPhase::Unknown);
    assert_eq!(
        command.postcondition_status,
        RuntimePostconditionStatus::Unknown
    );
    assert_eq!(command.binding.target_driver, RuntimeDriverRef::Unknown);
    assert_eq!(
        command.postcondition.desired_ack_level,
        RuntimeAcknowledgementLevel::Unknown
    );
}

#[test]
fn checked_in_runtime_control_fixtures_match_rust_serde() {
    let _: AgentSession = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/agent-session/valid/provider-driven-armed.json"
    ))
    .expect("AgentSession fixture");
    let _: RuntimeCommandRecord = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/runtime-command-record/valid/exact-start-cycle.json"
    ))
    .expect("RuntimeCommandRecord fixture");
    let interrupted: RuntimeCommandRecord = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/runtime-command-record/valid/exact-start-cycle-interrupted.json"
    ))
    .expect("RuntimeCommandRecord interrupted fixture");
    assert_eq!(
        interrupted
            .cycle_correlation
            .and_then(|correlation| correlation.interrupt_cause)
            .as_deref(),
        Some("host_control")
    );
    let _: ControlCommandEnvelope = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/control-command-envelope/valid/exact-interrupt.json"
    ))
    .expect("ControlCommandEnvelope fixture");
    let _: ProviderInvocation = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/provider-invocation/valid/exact-binding.json"
    ))
    .expect("ProviderInvocation fixture");
}

/// P2-1 (DEV-156 S3 review 01): a REAL serde-emitted RuntimeCommandRecord
/// must validate against the checked-in fail-closed schema — with and
/// without an interrupt cause. This binds the durable type to
/// `schemas/runtime-command-record.schema.json` so the two cannot drift
/// apart silently (the existence-only gates cannot see that divergence).
#[test]
fn emitted_runtime_command_record_matches_the_checked_in_schema() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../schemas/runtime-command-record.schema.json"
    ))
    .expect("runtime-command-record schema parses");
    let cycle_schema = schema
        .pointer("/$defs/cycle_correlation")
        .expect("schema carries $defs.cycle_correlation");
    assert_eq!(
        cycle_schema.get("additionalProperties"),
        Some(&serde_json::Value::Bool(false)),
        "cycle_correlation schema must fail closed"
    );
    let schema_keys = |value: &serde_json::Value| -> Vec<String> {
        let mut keys = value
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("cycle_correlation properties")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };
    let schema_property_keys = schema_keys(cycle_schema);
    assert!(
        schema_property_keys
            .iter()
            .any(|key| key == "interrupt_cause"),
        "the checked-in schema must name interrupt_cause: {schema_property_keys:?}"
    );

    let emitted_keys = |record: &RuntimeCommandRecord| -> (serde_json::Value, Vec<String>) {
        let emitted = serde_json::to_value(record).expect("serialize RuntimeCommandRecord");
        let correlation = emitted
            .get("cycle_correlation")
            .expect("emitted cycle_correlation")
            .clone();
        let mut keys = correlation
            .as_object()
            .expect("cycle_correlation object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        (correlation, keys)
    };

    // Without an interrupt: the key is OMITTED (skip_serializing_if), never
    // emitted as null, and the emitted key set is the schema's property set
    // minus the optional interrupt_cause.
    let mut record: RuntimeCommandRecord = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/runtime-command-record/valid/exact-start-cycle.json"
    ))
    .expect("RuntimeCommandRecord fixture");
    record
        .cycle_correlation
        .as_mut()
        .expect("fixture carries a correlation")
        .interrupt_cause = None;
    let (correlation, keys) = emitted_keys(&record);
    assert!(
        correlation.get("interrupt_cause").is_none(),
        "a non-interrupted cycle must omit interrupt_cause: {correlation}"
    );
    let expected_without: Vec<String> = schema_property_keys
        .iter()
        .filter(|key| key.as_str() != "interrupt_cause")
        .cloned()
        .collect();
    assert_eq!(keys, expected_without, "emitted keys must match the schema");

    // With an interrupt: the attributed label is emitted and the emitted key
    // set is EXACTLY the schema's property set (additionalProperties: false
    // would reject anything else).
    record
        .cycle_correlation
        .as_mut()
        .expect("fixture carries a correlation")
        .interrupt_cause = Some("host_control".to_string());
    let (correlation, keys) = emitted_keys(&record);
    assert_eq!(
        correlation.get("interrupt_cause").and_then(|v| v.as_str()),
        Some("host_control")
    );
    assert_eq!(
        keys, schema_property_keys,
        "emitted keys must match the schema"
    );

    // Every schema-required key is present in both emitted shapes.
    let required = cycle_schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .expect("cycle_correlation required list")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    for key in required {
        assert!(keys.contains(&key), "emitted record misses required {key}");
    }
}
