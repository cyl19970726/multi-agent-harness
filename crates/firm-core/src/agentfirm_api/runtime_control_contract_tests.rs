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
    let _: ControlCommandEnvelope = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/control-command-envelope/valid/exact-interrupt.json"
    ))
    .expect("ControlCommandEnvelope fixture");
    let _: ProviderInvocation = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/provider-invocation/valid/exact-binding.json"
    ))
    .expect("ProviderInvocation fixture");
}
