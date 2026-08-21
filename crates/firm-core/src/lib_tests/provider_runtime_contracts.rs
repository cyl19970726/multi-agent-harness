use super::*;

#[cfg(test)]
fn capacity_snapshot(
    state: ProviderCapacityState,
    observed_unix_ms: u64,
) -> ProviderCapacitySnapshot {
    ProviderCapacitySnapshot {
        provider: "kimi".to_string(),
        execution_mode: "kimi_acp".to_string(),
        account: ProviderAccountRef {
            source: "oauth_credentials_file".to_string(),
            identifier: None,
            plan: None,
        },
        state,
        observed_at: "unix-ms:1000".to_string(),
        observed_unix_ms,
        reset_at: None,
        evidence_source: ProviderCapacityEvidence::ProviderError,
        confidence: ProviderCapacityConfidence::Observed,
        windows: Vec::new(),
        diagnosis: None,
        runtime_context: Vec::new(),
        detail: None,
    }
}

#[test]
fn capacity_default_state_is_unknown_and_never_available() {
    assert_eq!(
        ProviderCapacityState::default(),
        ProviderCapacityState::Unknown
    );
    assert!(!ProviderCapacityState::Unknown.is_known_unavailable());
    assert!(!ProviderCapacityState::Available.is_known_unavailable());
    assert!(!ProviderCapacityState::Limited.is_known_unavailable());
    assert!(ProviderCapacityState::Exhausted.is_known_unavailable());
    assert!(ProviderCapacityState::Unauthorized.is_known_unavailable());
}

#[test]
fn capacity_freshness_uses_the_observation_timestamp() {
    let snapshot = capacity_snapshot(ProviderCapacityState::Exhausted, 1_000);
    assert_eq!(
        snapshot.freshness(1_500, 1_000),
        ProviderCapacityFreshness::Fresh
    );
    assert_eq!(
        snapshot.freshness(5_000, 1_000),
        ProviderCapacityFreshness::Stale
    );
    // A future-dated or unstamped observation is never treated as fresh.
    assert_eq!(
        snapshot.freshness(500, 1_000),
        ProviderCapacityFreshness::Unknown
    );
    assert_eq!(
        capacity_snapshot(ProviderCapacityState::Exhausted, 0).freshness(5_000, 1_000),
        ProviderCapacityFreshness::Unknown
    );
}

#[test]
fn fresh_known_unavailable_capacity_blocks_start() {
    for state in [
        ProviderCapacityState::Exhausted,
        ProviderCapacityState::Unauthorized,
    ] {
        let snapshot = capacity_snapshot(state, 1_000);
        let decision = provider_capacity_start_decision(Some(&snapshot), 1_100, 1_000);
        assert!(decision.is_blocked(), "{state:?} must block a fresh start");
        assert!(
            decision.reason().contains("kimi_acp"),
            "the blocking reason names the execution mode: {}",
            decision.reason()
        );
    }
}

#[test]
fn unknown_absent_and_stale_capacity_never_block_and_never_claim_available() {
    let unknown = capacity_snapshot(ProviderCapacityState::Unknown, 1_000);
    assert!(!provider_capacity_start_decision(Some(&unknown), 1_100, 1_000).is_blocked());
    assert_ne!(unknown.state, ProviderCapacityState::Available);

    assert!(!provider_capacity_start_decision(None, 1_100, 1_000).is_blocked());

    let stale = capacity_snapshot(ProviderCapacityState::Exhausted, 1_000);
    let decision = provider_capacity_start_decision(Some(&stale), 100_000, 1_000);
    assert!(!decision.is_blocked());
    assert!(decision.reason().contains("no longer fresh"));
}

#[test]
fn capacity_is_independent_of_adapter_compatibility_and_round_trips_json() {
    // A reviewed-current adapter says nothing about runtime availability:
    // this is the Wave 2 evidence (`current` adapter, 403 at request time).
    let profile = ProviderIntegrationProfile {
        agent_runtime_provider: Some(AgentRuntimeProvider("claude".to_string())),
        model_route: None,
        provider: "claude".to_string(),
        execution_mode: "claude_agent_sdk".to_string(),
        execution_driver: MemberExecutionDriver::HostDriven,
        provider_version: Some("2.1.220".to_string()),
        adapter_contract_version: Some("claude-agent-sdk-v1".to_string()),
        reviewed_provider_versions: vec!["2.1.220".to_string()],
        compatibility_status: ProviderCompatibilityStatus::Current,
        adapter_reviewed_at: None,
        compatibility_note: None,
        interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
        ordinary_message_boundary: OrdinaryMessageBoundary::InTurn,
        plan_mode: ProviderFeatureMode::Emulated,
        goal_mode: ProviderFeatureMode::Emulated,
        tool_event_fidelity: ProviderEventFidelity::Structured,
        artifact_event_fidelity: ProviderEventFidelity::Structured,
        supports_cancel: true,
        supports_resume: true,
        observes_native_subagents: false,
        observes_background_tasks: false,
        thinking_transient_only: true,
        control_topology: ControlTopology::default(),
        composition_fingerprint: None,
        capability_fingerprint: None,
        capability_bindings: Vec::new(),
        binding_admission: ProviderBindingAdmission::Failed,
        adapter_bridge_revision: None,
        security_enforcement_locus: SecurityEnforcementLocus::default(),
    };
    let mut snapshot = capacity_snapshot(ProviderCapacityState::Unauthorized, 1_000);
    snapshot.provider = "claude".to_string();
    snapshot.execution_mode = "claude_agent_sdk".to_string();
    snapshot.diagnosis = Some("no HTTPS_PROXY in the Harness process".to_string());
    snapshot.runtime_context = vec![ProviderRuntimeContextFact {
        key: "HTTPS_PROXY".to_string(),
        present: false,
        note: Some("absent".to_string()),
    }];

    assert_eq!(
        profile.compatibility_status,
        ProviderCompatibilityStatus::Current
    );
    assert!(snapshot.state.is_known_unavailable());

    let encoded = serde_json::to_string(&snapshot).expect("serialize snapshot");
    let decoded: ProviderCapacitySnapshot =
        serde_json::from_str(&encoded).expect("deserialize snapshot");
    assert_eq!(decoded, snapshot);
    assert!(
        !encoded.contains("compatibility"),
        "capacity JSON must not carry adapter compatibility: {encoded}"
    );
}

#[test]
fn provider_integration_profile_fixtures_preserve_legacy_defaults_and_exact_bindings() {
    let legacy: ProviderIntegrationProfile = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/provider-integration-profile/valid/minimal.json"
    ))
    .expect("legacy ProviderIntegrationProfile fixture");
    assert!(legacy.agent_runtime_provider.is_none());
    assert!(legacy.model_route.is_none());
    assert!(legacy.capability_bindings.is_empty());
    assert_eq!(legacy.binding_admission, ProviderBindingAdmission::Failed);
    legacy
        .validate()
        .expect("legacy fail-closed profile validates");

    let exact: ProviderIntegrationProfile = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/provider-integration-profile/valid/pi-rpc.json"
    ))
    .expect("exact ProviderIntegrationProfile fixture");
    assert_eq!(
        exact.agent_runtime_provider,
        Some(AgentRuntimeProvider("pi".to_string()))
    );
    assert_eq!(exact.binding_admission, ProviderBindingAdmission::Active);
    assert_eq!(exact.capability_bindings.len(), 3);
    exact.validate().expect("exact active profile validates");

    let invalid: ProviderIntegrationProfile = serde_json::from_str(include_str!(
        "../../../../schemas/fixtures/provider-integration-profile/invalid/active-binding-without-evidence.json"
    ))
    .expect("invalid admission fixture is syntactically valid JSON");
    assert!(invalid.validate().is_err());
}

#[cfg(test)]
fn provider_compatibility_admission(
    policy: ProviderCompatibilityAdmissionPolicy,
) -> ProviderCompatibilityAdmission {
    ProviderCompatibilityAdmission {
        id: "admission-1".to_string(),
        project_id: "project-1".to_string(),
        store_id: "store-1".to_string(),
        provider: "claude".to_string(),
        execution_mode: "claude_agent_sdk".to_string(),
        provider_version: "2.1.220".to_string(),
        adapter_contract_version: "claude-agent-sdk-v1".to_string(),
        policy,
        actor: "operator-1".to_string(),
        evidence_refs: vec!["evidence-1".to_string()],
        admitted_at: "unix-ms:1".to_string(),
        lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
        predecessor_admission_id: None,
        reason: None,
    }
}

#[test]
fn provider_compatibility_admission_accepts_strict_and_advisory_exact_keys() {
    for policy in [
        ProviderCompatibilityAdmissionPolicy::Strict,
        ProviderCompatibilityAdmissionPolicy::Advisory,
    ] {
        let admission = provider_compatibility_admission(policy);
        assert!(admission.validate().is_ok());
        assert!(admission.is_active());
        assert_eq!(
            admission.exact_key(),
            (
                "claude",
                "claude_agent_sdk",
                "2.1.220",
                "claude-agent-sdk-v1"
            )
        );

        let encoded = serde_json::to_value(&admission).expect("serialize admission");
        let decoded: ProviderCompatibilityAdmission =
            serde_json::from_value(encoded).expect("deserialize admission");
        assert_eq!(decoded, admission);
    }
}

#[test]
fn provider_compatibility_admission_rejects_empty_evidence() {
    let mut admission =
        provider_compatibility_admission(ProviderCompatibilityAdmissionPolicy::Strict);
    admission.evidence_refs.clear();
    assert!(admission.validate().is_err());

    admission.evidence_refs.push("  ".to_string());
    assert!(admission.validate().is_err());
}

#[test]
fn provider_compatibility_admission_rejects_invalid_lifecycle_metadata() {
    let mut active =
        provider_compatibility_admission(ProviderCompatibilityAdmissionPolicy::Advisory);
    active.reason = Some("not valid on an active row".to_string());
    assert!(active.validate().is_err());

    for lifecycle in [
        ProviderCompatibilityAdmissionLifecycle::Revoked,
        ProviderCompatibilityAdmissionLifecycle::Superseded,
    ] {
        let mut terminal =
            provider_compatibility_admission(ProviderCompatibilityAdmissionPolicy::Strict);
        terminal.lifecycle = lifecycle;
        assert!(!terminal.is_active());
        assert!(terminal.validate().is_err());

        terminal.predecessor_admission_id = Some(" ".to_string());
        terminal.reason = Some("provider contract changed".to_string());
        assert!(terminal.validate().is_err());

        terminal.predecessor_admission_id = Some("admission-1".to_string());
        terminal.reason = Some(String::new());
        assert!(terminal.validate().is_err());

        terminal.reason = Some("provider contract changed".to_string());
        assert!(terminal.validate().is_ok());
    }
}

#[test]
fn provider_compatibility_admission_rejects_unknown_fields() {
    let admission = provider_compatibility_admission(ProviderCompatibilityAdmissionPolicy::Strict);
    let mut value = serde_json::to_value(admission).expect("serialize admission");
    value["source_reviewed"] = serde_json::json!(true);
    let error = serde_json::from_value::<ProviderCompatibilityAdmission>(value)
        .expect_err("admission wire format must reject unknown fields");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn provider_compatibility_block_cause_is_typed_and_rejects_unknown_fields() {
    let cause = ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "cause-1".into(),
        member_run_id: "member-1".into(),
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        provider_version: "9.9.9".into(),
        adapter_contract_version: "codex-app-server-v1".into(),
        boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:1".into(),
    };
    cause.validate().expect("valid typed cause");
    let mut value = serde_json::to_value(&cause).unwrap();
    value["forged_authority"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProviderCompatibilityBlockCause>(value).is_err());

    let mut inconsistent = cause;
    inconsistent.compatibility_status = ProviderCompatibilityStatus::Unavailable;
    assert!(inconsistent.validate().is_err());
    inconsistent.source = ProviderCompatibilityBlockSource::ProbeFailure;
    inconsistent.probe_error = Some("probe failed".into());
    inconsistent.validate().expect("typed probe failure");
}

#[test]
fn member_run_rows_without_capacity_stay_readable_and_absent_is_not_available() {
    let row = serde_json::json!({
        "id": "member-run-1",
        "team_run_id": "team-run-1",
        "agent_member_id": "member-1",
        "name": "Integration",
        "role": "Integration Engineer",
        "provider": "claude",
        "status": "idle",
        "started_at": "unix-ms:1"
    });
    let member: ProviderRuntimeProjection =
        serde_json::from_value(row).expect("provider runtime projection");
    assert_eq!(member.provider_capacity, None);
    assert!(!provider_capacity_start_decision(
        member.provider_capacity.as_ref(),
        1_000,
        PROVIDER_CAPACITY_DEFAULT_TTL_MS
    )
    .is_blocked());
}

/// The emit/schema contract for canonical MemberRun.
///
/// `schemas/member-run.schema.json` keeps `additionalProperties: false`, so
/// any field the emitter serialises that the schema does not declare makes an
/// emitted MemberRun fail validation against its own schema.
#[test]
fn emitted_member_run_keys_are_declared_in_member_run_schema() {
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/member-run.schema.json"
        ))
        .expect("read member-run schema"),
    )
    .expect("parse member-run schema");
    assert_eq!(
        schema["additionalProperties"],
        serde_json::Value::Bool(false),
        "this test only means something while the schema is closed"
    );

    let member = agentfirm_api::MemberRun {
        id: "member-run-1".into(),
        agent_member_id: "member-1".into(),
        team_run_id: "team-run-1".into(),
        role_snapshot: "Platform Development".into(),
        provider_profile_snapshot: Some("claude-sdk-v1".into()),
        requested_controls: serde_json::json!({"model": "claude-opus"}),
        effective_controls: serde_json::json!({"model": "claude-opus"}),
        coordination_status: agentfirm_api::MemberCoordinationStatus::Active,
        runtime_status: agentfirm_api::MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: Some("workspace-1".into()),
        native_session: None,
        version: 1,
        started_at: "unix-ms:1785591600000".into(),
        last_event_at: None,
        finished_at: None,
    };

    let encoded = serde_json::to_value(&member).expect("encode member run");
    let declared = schema["properties"].as_object().expect("schema properties");
    let undeclared = encoded
        .as_object()
        .expect("encoded member run")
        .keys()
        .filter(|key| !declared.contains_key(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
            undeclared.is_empty(),
            "emitted MemberRun fields are not declared in member-run.schema.json (additionalProperties is false): {undeclared:?}"
        );
    let decoded: agentfirm_api::MemberRun =
        serde_json::from_value(encoded).expect("decode canonical member run");
    assert_eq!(decoded, member);
}
