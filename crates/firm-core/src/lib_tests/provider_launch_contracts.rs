use super::*;

#[cfg(test)]
fn sample_member() -> ProviderLaunchProfile {
    ProviderLaunchProfile {
        id: "agent-1".to_string(),
        name: "Worker".to_string(),
        description: "A worker member".to_string(),
        role: "worker".to_string(),
        provider: "codex".to_string(),
        model: Some("o3".to_string()),
        profile: None,
        provider_config: ProviderLaunchConfig::default(),
        capabilities: vec!["code".to_string()],
        team_ids: vec![],
        prompt_ref: Some(".firm/prompts/worker.md".to_string()),
        skill_refs: vec!["firm-workflow".to_string()],
        workspace_policy: None,
        provider_cwd_hint: Some("../worktrees/task-1".to_string()),
        permission_profile: None,
        runtime_workspace_roots: Vec::new(),
        status: ProviderLaunchStatus::Idle,
        current_task_id: None,
        current_proposal_id: None,
        provider_runtime_id: None,
        native_session: None,
        provider_thread_id: None,
        provider_agent_path: None,
        provider_agent_nickname: None,
        provider_agent_role: None,
        control_endpoint: None,
        created_at: "2026-05-26T00:00:00Z".to_string(),
        last_seen_at: None,
    }
}

#[cfg(test)]
fn sample_message() -> RegistryMessage {
    RegistryMessage {
        id: "msg-1".to_string(),
        task_id: Some("task-1".to_string()),
        from_agent_id: "leader-1".to_string(),
        to_agent_id: Some("agent-1".to_string()),
        channel: Some("team".to_string()),
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: "Implement the launch spec.".to_string(),
        evidence_ids: vec![],
        created_at: "2026-05-26T00:00:00Z".to_string(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    }
}

#[test]
fn launch_spec_composes_from_member_and_message() {
    let mut member = sample_member();
    member.provider_config.sandbox_policy = Some("workspace-write".to_string());
    member.provider_config.effort = Some("high".to_string());
    member.runtime_workspace_roots = vec!["crates/firm-core".to_string()];
    member.provider_config.runtime_workspace_roots = vec!["crates/firm-cli".to_string()];
    let message = sample_message();

    let spec = build_launch_spec(&member, &message);

    // Pillar 1 base configuration flows through unchanged.
    assert_eq!(spec.prompt_ref.as_deref(), Some(".firm/prompts/worker.md"));
    assert_eq!(spec.model.as_deref(), Some("o3"));
    assert_eq!(spec.effort.as_deref(), Some("high"));
    assert_eq!(spec.skill_refs, vec!["firm-workflow".to_string()]);
    // Pillar 2 workspace flows through as the cwd / worktree root.
    assert_eq!(spec.workspace.as_deref(), Some("../worktrees/task-1"));
    // The turn input carries the message envelope + content.
    assert!(spec.message_content.contains("message_id: msg-1"));
    assert!(spec.message_content.contains("kind: message"));
    assert!(spec.message_content.contains("task_id: task-1"));
    assert!(spec.message_content.contains("Implement the launch spec."));
    // Fields with no neutral source yet are empty/none, not invented.
    assert!(spec.tools.is_empty());
    assert!(spec.mcp.is_none());
    // A fresh member (no prior provider thread/session) carries no resume token.
    assert!(spec.resume.is_none());
    assert!(spec.output.is_none());
}

#[test]
fn launch_spec_carries_resume_from_member_provider_thread_id() {
    // A member that already has a provider thread/session id (from a prior
    // delivery) must produce a spec that resumes that session, so memory
    // carries across deliveries instead of starting fresh each turn.
    let mut member = sample_member();
    member.provider_thread_id = Some("thread-abc-123".to_string());
    let message = sample_message();

    let spec = build_launch_spec(&member, &message);

    assert_eq!(spec.resume.as_deref(), Some("thread-abc-123"));
}

#[test]
fn launch_spec_maps_codex_sandbox_vocabulary_onto_neutral_permission() {
    // Each Codex sandbox spelling (dashed and camelCase) maps onto the neutral
    // permission enum; no Codex wire vocabulary survives onto the spec.
    let cases = [
        ("read-only", LaunchPermission::ReadOnly),
        ("readOnly", LaunchPermission::ReadOnly),
        ("workspace-write", LaunchPermission::WorkspaceWrite),
        ("workspaceWrite", LaunchPermission::WorkspaceWrite),
        ("danger-full-access", LaunchPermission::FullAccess),
        ("dangerFullAccess", LaunchPermission::FullAccess),
    ];
    for (policy, expected) in cases {
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some(policy.to_string());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(
            spec.permission, expected,
            "policy {policy} should map to {expected:?}"
        );
    }
}

#[test]
fn launch_spec_writable_roots_dedupe_and_drop_on_read_only() {
    // workspace_write carries de-duplicated member + provider_config roots.
    let mut member = sample_member();
    member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
    member.runtime_workspace_roots = vec!["shared".to_string(), "a".to_string()];
    member.provider_config.runtime_workspace_roots = vec!["shared".to_string(), "b".to_string()];
    let spec = build_launch_spec(&member, &sample_message());
    assert_eq!(
        spec.writable_roots,
        vec!["shared".to_string(), "a".to_string(), "b".to_string()],
        "writable roots must be member-then-config order, de-duplicated"
    );

    // read_only never carries writable roots even if the member declares them.
    member.provider_config.sandbox_policy = Some("read-only".to_string());
    let spec = build_launch_spec(&member, &sample_message());
    assert_eq!(spec.permission, LaunchPermission::ReadOnly);
    assert!(
        spec.writable_roots.is_empty(),
        "a read-only turn must not carry writable roots"
    );
}

#[test]
fn launch_spec_absent_sandbox_policy_falls_back_to_safe_default() {
    // A member that never declared a sandbox policy must not be silently
    // elevated; it falls back to the default posture.
    let member = sample_member();
    assert!(member.provider_config.sandbox_policy.is_none());
    let spec = build_launch_spec(&member, &sample_message());
    assert_eq!(spec.permission, LaunchPermission::default());
}

#[test]
fn launch_spec_round_trips_json() {
    let mut member = sample_member();
    member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
    member.provider_config.effort = Some("medium".to_string());
    member.provider_config.output_schema = Some(serde_json::json!({
        "type": "object",
        "properties": { "verdict": { "type": "string" } },
        "required": ["verdict"]
    }));
    member.runtime_workspace_roots = vec!["crates".to_string()];
    let spec = build_launch_spec(&member, &sample_message());

    let json = serde_json::to_string(&spec).expect("serialize launch spec");
    let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize launch spec");
    assert_eq!(parsed, spec);
    // The neutral permission serializes to its snake_case wire spelling, not
    // the Codex `workspaceWrite` vocabulary it was mapped from.
    assert!(json.contains("\"permission\":\"workspace_write\""));
    assert!(json.contains("\"effort\":\"medium\""));
    assert!(json.contains("\"output_schema\""));
    assert_eq!(
        parsed.output_schema, member.provider_config.output_schema,
        "launch spec should round-trip the optional output schema"
    );
    assert!(!json.contains("workspaceWrite"));
}

#[test]
fn effort_defaults_to_none_for_legacy_json() {
    let provider_config: ProviderLaunchConfig = serde_json::from_value(serde_json::json!({
        "service_tier": "default"
    }))
    .expect("legacy provider config without effort should deserialize");
    assert!(provider_config.effort.is_none());
    assert!(provider_config.output_schema.is_none());

    let spec: LaunchSpec = serde_json::from_value(serde_json::json!({
        "message_content": "legacy turn",
        "model": "o3",
        "permission": "workspace_write"
    }))
    .expect("legacy launch spec without effort should deserialize");
    assert!(spec.effort.is_none());
    assert!(spec.output_schema.is_none());
}

#[test]
fn build_launch_spec_carries_output_schema_from_provider_config() {
    let mut member = sample_member();
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"]
    });
    member.provider_config.output_schema = Some(schema.clone());
    let spec = build_launch_spec(&member, &sample_message());
    assert_eq!(spec.output_schema, Some(schema));
}

#[test]
fn launch_permission_wire_values_are_neutral() {
    assert_eq!(LaunchPermission::ReadOnly.as_str(), "read_only");
    assert_eq!(LaunchPermission::WorkspaceWrite.as_str(), "workspace_write");
    assert_eq!(LaunchPermission::FullAccess.as_str(), "full_access");
    // Round-trip each variant through serde to confirm the wire spelling.
    for variant in [
        LaunchPermission::ReadOnly,
        LaunchPermission::WorkspaceWrite,
        LaunchPermission::FullAccess,
    ] {
        let json = serde_json::to_string(&variant).expect("serialize permission");
        assert_eq!(json, format!("\"{}\"", variant.as_str()));
        let parsed: LaunchPermission = serde_json::from_str(&json).expect("deserialize permission");
        assert_eq!(parsed, variant);
    }
}

#[test]
fn delivery_handle_passes_endpoint_through_verbatim() {
    // The neutral delivery handle preserves any endpoint scheme verbatim; it
    // does not interpret or strip `unix://` (that stays in the CLI layer).
    for endpoint in [
        "unix:///tmp/agent/codex.sock",
        "exec://session/abc",
        "/tmp/plain/path",
    ] {
        let handle = DeliveryHandle::from_endpoint(endpoint);
        assert_eq!(handle.endpoint(), endpoint);
        let json = serde_json::to_string(&handle).expect("serialize handle");
        let parsed: DeliveryHandle = serde_json::from_str(&json).expect("deserialize handle");
        assert_eq!(parsed, handle);
        assert_eq!(parsed.endpoint(), endpoint);
    }
}

#[test]
fn launch_mcp_block_round_trips_when_present() {
    // The MCP block is omitted by build_launch_spec today, but the neutral
    // shape must round-trip so later WPs can populate it.
    let mcp = LaunchMcp {
        servers: vec![LaunchMcpServer {
            id: "fs".to_string(),
            transport: Some("stdio".to_string()),
            command: vec!["mcp-fs".to_string(), "--root".to_string()],
            url: None,
            allowed_tools: vec!["read".to_string()],
        }],
    };
    let json = serde_json::to_string(&mcp).expect("serialize mcp");
    let parsed: LaunchMcp = serde_json::from_str(&json).expect("deserialize mcp");
    assert_eq!(parsed, mcp);
}

#[test]
fn build_launch_spec_carries_mcp_from_provider_config() {
    let mut member = sample_member();
    member.provider_config.mcp = Some(LaunchMcp {
        servers: vec![LaunchMcpServer {
            id: "fs".to_string(),
            transport: Some("stdio".to_string()),
            command: vec!["mcp-fs".to_string()],
            url: None,
            allowed_tools: vec![],
        }],
    });
    let spec = build_launch_spec(&member, &sample_message());
    assert!(
        spec.mcp.is_some(),
        "launch spec should carry mcp from provider_config"
    );
    let mcp = spec.mcp.as_ref().unwrap();
    assert_eq!(mcp.servers.len(), 1);
    assert_eq!(mcp.servers[0].id, "fs");
}

#[test]
fn build_launch_spec_removes_harness_mutation_mcp_but_keeps_unrelated_servers() {
    let mut member = sample_member();
    member.provider_config.mcp = Some(LaunchMcp {
        servers: vec![
            LaunchMcpServer {
                id: "harness".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["firm".to_string(), "mcp".to_string()],
                url: None,
                allowed_tools: Vec::new(),
            },
            LaunchMcpServer {
                id: "fs".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["mcp-fs".to_string()],
                url: None,
                allowed_tools: vec!["read".to_string()],
            },
        ],
    });

    let spec = build_launch_spec(&member, &sample_message());
    let mcp = spec.mcp.expect("unrelated MCP server remains available");
    assert_eq!(mcp.servers.len(), 1);
    assert_eq!(mcp.servers[0].id, "fs");
}

#[test]
fn build_launch_spec_mcp_none_when_absent() {
    let member = sample_member();
    assert!(member.provider_config.mcp.is_none());
    let spec = build_launch_spec(&member, &sample_message());
    assert!(
        spec.mcp.is_none(),
        "launch spec mcp should be none when member has no mcp"
    );
}

#[test]
fn build_launch_spec_mcp_round_trips_json() {
    let mut member = sample_member();
    member.provider_config.mcp = Some(LaunchMcp {
        servers: vec![LaunchMcpServer {
            id: "api".to_string(),
            transport: Some("http".to_string()),
            command: vec![],
            url: Some("http://localhost:3000".to_string()),
            allowed_tools: vec!["query".to_string()],
        }],
    });
    let spec = build_launch_spec(&member, &sample_message());
    let json = serde_json::to_string(&spec).expect("serialize spec");
    let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize spec");
    assert_eq!(parsed.mcp, spec.mcp);
}

#[test]
fn provider_capabilities_codex_matches_doc_table() {
    let cap = ProviderCapabilities::codex_exec();
    assert!(cap.streaming, "Codex exec has --json streaming");
    assert!(cap.resume, "Codex exec has --session resume");
    assert!(
        !cap.mid_turn_approval,
        "Codex exec has policy pre-approve, no mid-turn"
    );
    assert!(cap.subagents, "Codex supports subagents");
    assert!(cap.mcp, "Codex exec has --config mcp_servers");
    assert!(!cap.hooks, "Codex exec has limited hooks");
    assert!(cap.schema, "Codex exec has --output-schema");
    assert!(!cap.cost, "Codex reports token usage only, no USD");
}

#[test]
fn provider_capabilities_claude_matches_doc_table() {
    let cap = ProviderCapabilities::claude_exec();
    assert!(cap.streaming, "Claude -p has --output-format stream-json");
    assert!(cap.resume, "Claude has --resume");
    assert!(!cap.mid_turn_approval, "Claude -p has no mid-turn approval");
    assert!(cap.subagents, "Claude supports subagents");
    assert!(cap.mcp, "Claude has --mcp-config");
    assert!(!cap.hooks, "Claude has no documented hooks");
    assert!(cap.schema, "Claude has --json-schema");
    assert!(cap.cost, "Claude reports result.total_cost_usd");
}

#[test]
fn provider_capabilities_round_trips_json() {
    let cap = ProviderCapabilities::codex_exec();
    let json = serde_json::to_string(&cap).expect("serialize capabilities");
    let parsed: ProviderCapabilities =
        serde_json::from_str(&json).expect("deserialize capabilities");
    assert_eq!(parsed, cap);
}

#[test]
fn provider_capabilities_display_shows_enabled_features() {
    let cap = ProviderCapabilities::codex_exec();
    let display = cap.to_string();
    assert!(display.contains("streaming"));
    assert!(display.contains("resume"));
    assert!(display.contains("mcp"));
    assert!(display.contains("subagents"));
    assert!(
        !display.contains("mid_turn_approval"),
        "disabled features should not show"
    );
}

#[test]
fn supports_streaming_exec_check() {
    let mut cap = ProviderCapabilities::codex_exec();
    assert!(
        cap.supports_streaming_exec(),
        "streaming + no mid-turn should be ok"
    );
    cap.mid_turn_approval = true;
    assert!(
        !cap.supports_streaming_exec(),
        "mid-turn approval blocks streaming exec"
    );
}

#[test]
fn workspace_observability_fields_round_trip_without_contents() {
    let snapshot = MemberWorkspaceSnapshot {
        cwd: "/projects/harness/worktrees/member-1".into(),
        project_binding_id: Some("harness".into()),
        resolution_source: Some("member_worktree".into()),
        git_head: Some("0123456789abcdef".into()),
        git_branch: Some("feature/member-1".into()),
        instruction_roots: vec!["/projects/harness".into()],
        skill_roots: vec!["/projects/harness/.agents/skills".into()],
    };
    assert!(snapshot.validate().is_ok());

    let json = serde_json::to_value(&snapshot).expect("serialize workspace snapshot");
    assert_eq!(json["cwd"], "/projects/harness/worktrees/member-1");
    assert!(json.get("instruction_contents").is_none());
    assert!(json.get("skill_contents").is_none());
    assert!(json.get("credentials").is_none());
    assert!(json.get("transcript").is_none());
    assert!(json.get("thinking").is_none());
    assert_eq!(
        serde_json::from_value::<MemberWorkspaceSnapshot>(json).expect("deserialize snapshot"),
        snapshot
    );
}

#[test]
fn workspace_observability_validation_rejects_empty_locators() {
    let snapshot = MemberWorkspaceSnapshot {
        cwd: " ".into(),
        project_binding_id: None,
        resolution_source: None,
        git_head: None,
        git_branch: None,
        instruction_roots: Vec::new(),
        skill_roots: Vec::new(),
    };
    assert_eq!(
        snapshot.validate(),
        Err(ValidationError::Required {
            field: "MemberWorkspaceSnapshot.cwd"
        })
    );

    let snapshot = MemberWorkspaceSnapshot {
        cwd: "/projects/harness".into(),
        project_binding_id: None,
        resolution_source: None,
        git_head: None,
        git_branch: None,
        instruction_roots: vec![String::new()],
        skill_roots: Vec::new(),
    };
    assert_eq!(
        snapshot.validate(),
        Err(ValidationError::Required {
            field: "MemberWorkspaceSnapshot.instruction_roots"
        })
    );
}

#[test]
fn workspace_rows_deserialize_with_optional_observability_fields() {
    let team: AgentTeamRun = serde_json::from_str(
            r#"{"id":"tr-1","agent_team_id":"team-1","execution_node_id":"0f95cac7-5ff8-4c76-8f36-9c8f208815d3","project_binding_id":"project-1","host_surface":"codex-app","objective":"work","status":"planning","created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#,
        )
        .expect("deserialize team run");
    assert!(team.execution_root.is_none());

    let member: ProviderRuntimeProjection = serde_json::from_str(
            r#"{"id":"mr-1","team_run_id":"tr-1","agent_member_id":"member-1","name":"worker","role":"worker","provider":"codex","status":"idle","started_at":"unix-ms:1"}"#,
        )
        .expect("deserialize provider runtime projection");
    assert!(member.provider_cwd_hint.is_none());
    assert!(member.provider_environment_observation.is_none());
    assert_eq!(
        member.provider_controls,
        ProviderExecutionControls::default(),
        "historical rows stay readable without inventing requested or effective controls"
    );
}

#[test]
fn provider_execution_controls_separate_intent_from_native_receipt() {
    let mut controls = ProviderExecutionControls::requested(
        Some("gpt-5.6-sol".into()),
        Some("max".into()),
        Some("priority".into()),
    );

    assert_eq!(controls.model.status, ProviderControlStatus::Requested);
    assert_eq!(controls.model.effective, None);
    controls
        .model
        .mark_effective(Some("gpt-5.6-sol".into()), "confirmed by provider response");
    controls
        .service_tier
        .mark_unsupported("provider exposes no service tier");
    controls
        .reasoning_effort
        .mark_review_required("installed provider version is not reviewed");

    assert_eq!(controls.model.status, ProviderControlStatus::Effective);
    assert_eq!(
        controls.service_tier.status,
        ProviderControlStatus::Unsupported
    );
    assert_eq!(
        controls.reasoning_effort.status,
        ProviderControlStatus::ReviewRequired
    );
    assert_eq!(controls.reasoning_effort.effective, None);

    let encoded = serde_json::to_string(&controls).expect("serialize controls");
    let decoded: ProviderExecutionControls =
        serde_json::from_str(&encoded).expect("deserialize controls");
    assert_eq!(decoded, controls);
}
