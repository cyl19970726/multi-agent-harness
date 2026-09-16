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
        "../../../../schemas/fixtures/agent-session/valid/host-driven-disarmed-continuation.json"
    ))
    .expect("AgentSession fixture");
    // ADR 0067: the retired provider-driven shape must fail closed, not decode
    // into a managed driver.
    serde_json::from_str::<AgentSession>(include_str!(
        "../../../../schemas/fixtures/agent-session/invalid/retired-provider-driven.json"
    ))
    .expect_err("the retired provider_driven execution driver must not decode");
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

#[test]
fn legacy_command_status_folds_without_inventing_effect_or_execution_authority() {
    use RuntimeCommandPhase as Phase;
    let base: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/runtime-command-record/valid/phase-only-start-cycle.json"
    ))
    .unwrap();
    let phases = [
        Phase::Prepared,
        Phase::Dispatched,
        Phase::ProviderAcknowledged,
        Phase::Observed,
        Phase::Settled,
        Phase::Rejected,
        Phase::RecoveryRequired,
        Phase::Unknown,
    ];
    let statuses = [
        ("requested", Phase::Unknown),
        ("accepted", Phase::Prepared),
        ("quiesced", Phase::Observed),
        ("applied", Phase::Settled),
        ("failed", Phase::Rejected),
        ("recovery_required", Phase::RecoveryRequired),
    ];
    for (status, expected) in statuses {
        for phase in std::iter::once(None).chain(phases.into_iter().map(Some)) {
            let mut wire = base.clone();
            wire["status"] = status.into();
            match phase {
                Some(phase) => wire["phase"] = serde_json::to_value(phase).unwrap(),
                None => {
                    wire.as_object_mut().unwrap().remove("phase");
                }
            }
            let command: RuntimeCommandRecord = serde_json::from_value(wire).unwrap();
            let wanted = match phase {
                Some(phase) if phase == expected => phase,
                Some(_) => Phase::Unknown,
                None if status == "accepted" => Phase::Unknown,
                None => expected,
            };
            assert_eq!(command.phase, wanted, "legacy {status} / {phase:?}");
            assert_eq!(command.effect_certainty, RuntimeEffectCertainty::Unknown);
            assert_eq!(
                command.postcondition_status,
                RuntimePostconditionStatus::Unknown
            );
            let emitted = serde_json::to_value(&command).unwrap();
            assert!(
                emitted.get("status").is_none(),
                "current writers are phase-only"
            );
            let reread: RuntimeCommandRecord = serde_json::from_value(emitted).unwrap();
            assert_eq!(reread, command);
        }
    }
    for phase in phases {
        let mut wire = base.clone();
        wire["phase"] = serde_json::to_value(phase).unwrap();
        let command: RuntimeCommandRecord = serde_json::from_value(wire).unwrap();
        assert_eq!(
            command.phase, phase,
            "new phase-only rows retain their phase"
        );
    }
    for (key, value) in [
        ("status", serde_json::json!("future")),
        ("status", serde_json::Value::Null),
        ("phase", serde_json::json!("future")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut wire = base.clone();
        wire[key] = value;
        assert!(serde_json::from_value::<RuntimeCommandRecord>(wire).is_err());
    }
}

/// ADR 0076, X1b item D. The four retired continuation/injection command kinds
/// stay as DECODE-ONLY wire values: ADR 0067 and ADR 0068 retired the paths,
/// not the vocabulary, and a durable row written before those cutovers must
/// still read back as the kind it was written as.
///
/// Evidence for keeping rather than deleting them: across the five read-only
/// September store copies — 128,987 JSONL rows in 66 files — not one carries
/// any of the four as a command kind, and there is no constructor in the tree.
/// But 1,293 `member_runs` rows DO carry three of the same spellings as
/// `provider_profile.capability_bindings[].capability`, a different vocabulary
/// that happens to share the words. A deletion justified by "we grepped for the
/// string and found rows" would have been justified by the wrong rows, so the
/// wire values stay and this test holds them.
#[test]
fn the_retired_continuation_and_injection_command_kinds_still_decode() {
    use super::RuntimeCommandKind;
    for (wire, expected) in [
        (
            "activate_continuation",
            RuntimeCommandKind::ActivateContinuation,
        ),
        (
            "inhibit_continuation",
            RuntimeCommandKind::InhibitContinuation,
        ),
        (
            "inject_current_cycle",
            RuntimeCommandKind::InjectCurrentCycle,
        ),
        (
            "queue_at_native_boundary",
            RuntimeCommandKind::QueueAtNativeBoundary,
        ),
    ] {
        let decoded: RuntimeCommandKind =
            serde_json::from_value(serde_json::Value::String(wire.to_string()))
                .unwrap_or_else(|error| panic!("{wire} must still decode: {error}"));
        assert_eq!(decoded, expected, "{wire}");
        assert_eq!(
            serde_json::to_value(decoded).expect("re-serialize"),
            serde_json::Value::String(wire.to_string()),
            "{wire} must round-trip to the same frozen spelling"
        );
    }
}
