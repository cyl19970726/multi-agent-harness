use super::*;

#[test]
fn gap_round_trips_json() {
    let gap = Gap {
        id: "gap-1".to_string(),
        goal_id: Some("goal-1".to_string()),
        task_id: None,
        category: "observability".to_string(),
        severity: GapSeverity::P1,
        status: GapStatus::Open,
        summary: "Dashboard does not surface open reviews per task.".to_string(),
        evidence_ids: vec!["evidence-1".to_string()],
        next_step: Some("Wire reviewsByTask into the task surface.".to_string()),
        owner_agent_id: Some("worker-1".to_string()),
        repro_ref: None,
        closing_test_ref: None,
        created_at: "2026-05-26T00:00:00Z".to_string(),
        updated_at: "2026-05-26T00:00:00Z".to_string(),
    };

    let json = serde_json::to_string(&gap).expect("serialize gap");
    let parsed: Gap = serde_json::from_str(&json).expect("deserialize gap");

    assert_eq!(parsed, gap);
    assert!(parsed.validate().is_ok());
    // Closed severity/status enums serialize to their snake_case wire values.
    assert!(json.contains("\"severity\":\"p1\""));
    assert!(json.contains("\"status\":\"open\""));
}

#[test]
fn gap_bug_round_trips_with_bug_fields() {
    // A Bug is a Gap with category="bug" carrying the optional repro/closing-test
    // refs; no separate Bug object exists.
    let bug = Gap {
        id: "gap-bug-1".to_string(),
        goal_id: None,
        task_id: Some("task-1".to_string()),
        category: "bug".to_string(),
        severity: GapSeverity::P0,
        status: GapStatus::InProgress,
        summary: "Snapshot serialization drops the new gaps key.".to_string(),
        evidence_ids: vec![],
        next_step: None,
        owner_agent_id: Some("worker-2".to_string()),
        repro_ref: Some("artifacts/repro-1.log".to_string()),
        closing_test_ref: Some("crates/firm-cli/src/main.rs::snapshot_test".to_string()),
        created_at: "2026-05-26T00:00:00Z".to_string(),
        updated_at: "2026-05-26T01:00:00Z".to_string(),
    };

    let json = serde_json::to_string(&bug).expect("serialize bug gap");
    let parsed: Gap = serde_json::from_str(&json).expect("deserialize bug gap");

    assert_eq!(parsed, bug);
    assert!(parsed.validate().is_ok());
    assert!(json.contains("\"status\":\"in_progress\""));
    assert_eq!(parsed.severity, GapSeverity::P0);
}

#[test]
fn vision_round_trips_json() {
    let vision = Vision {
        id: "vision-1".to_string(),
        summary: "Generic harness object-model with a closed learning loop.".to_string(),
        source_refs: vec!["docs/company-os/vision.md".to_string()],
        created_at: "2026-05-30T00:00:00Z".to_string(),
    };

    let json = serde_json::to_string(&vision).expect("serialize vision");
    let parsed: Vision = serde_json::from_str(&json).expect("deserialize vision");

    assert_eq!(parsed, vision);
    assert!(parsed.validate().is_ok());
}

#[test]
fn project_id_for_path_home_is_global() {
    let home = std::path::Path::new("/Users/me");
    assert_eq!(project_id_for_path(home, home), GLOBAL_PROJECT_ID);
}

#[test]
fn project_id_for_path_under_home_flattens_to_slug() {
    let home = std::path::Path::new("/Users/me");
    assert_eq!(
        project_id_for_path(std::path::Path::new("/Users/me/multi-agent-harness"), home),
        "multi-agent-harness"
    );
    assert_eq!(
        project_id_for_path(std::path::Path::new("/Users/me/ai-luodi/jyx3d"), home),
        "ai-luodi-jyx3d"
    );
}

#[test]
fn project_id_for_path_outside_home_is_stable_hash() {
    let home = std::path::Path::new("/Users/me");
    let id = project_id_for_path(std::path::Path::new("/opt/work/thing"), home);
    assert!(id.starts_with("proj-"), "external path → hashed id: {id}");
    // Stable across calls (a durable id must not change run-to-run).
    assert_eq!(
        id,
        project_id_for_path(std::path::Path::new("/opt/work/thing"), home)
    );
    // Distinct paths → distinct ids.
    assert_ne!(
        id,
        project_id_for_path(std::path::Path::new("/opt/work/other"), home)
    );
}

#[test]
fn project_store_root_is_under_projects() {
    let home = std::path::Path::new("/Users/me/.firm");
    assert_eq!(
        project_store_root(home, "ai-luodi-jyx3d"),
        std::path::Path::new("/Users/me/.firm/projects/ai-luodi-jyx3d")
    );
    assert_eq!(
        project_store_root(home, GLOBAL_PROJECT_ID),
        std::path::Path::new("/Users/me/.firm/projects/_global")
    );
}

#[test]
fn project_context_round_trips_json() {
    let ctx = ProjectContext {
        id: "ai-luodi-jyx3d".into(),
        project_root: std::path::PathBuf::from("/Users/me/ai-luodi/jyx3d"),
        store_root: std::path::PathBuf::from("/Users/me/.firm/projects/ai-luodi-jyx3d"),
        kind: ProjectKind::Repo,
        is_git_repo: true,
    };
    let json = serde_json::to_string(&ctx).expect("serialize");
    assert_eq!(
        serde_json::from_str::<ProjectContext>(&json).expect("deserialize"),
        ctx
    );
    // kind is snake_case on the wire.
    assert!(json.contains("\"kind\":\"repo\""));
}

#[test]
fn validation_rejects_missing_required_id() {
    let member = ProviderLaunchProfile {
        id: "".to_string(),
        name: "Leader".to_string(),
        description: "Lead agent".to_string(),
        role: "leader".to_string(),
        provider: "codex".to_string(),
        model: None,
        profile: None,
        provider_config: ProviderLaunchConfig::default(),
        capabilities: vec![],
        team_ids: vec![],
        prompt_ref: None,
        skill_refs: vec![],
        workspace_policy: None,
        provider_cwd_hint: None,
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
    };

    assert_eq!(
        member.validate(),
        Err(ValidationError::Required {
            field: "ProviderLaunchProfile.id"
        })
    );
}

#[test]
fn message_sender_kind_defaults_to_agent_and_persists_operator() {
    // A record persisted before sender_kind existed omits the field entirely.
    // It must deserialize as SenderKind::Agent (additive-optional backfill).
    let legacy_json = r#"{
            "id": "msg-legacy",
            "task_id": null,
            "from_agent_id": "leader-1",
            "to_agent_id": "agent-1",
            "channel": null,
            "kind": "message",
            "delivery_status": "queued",
            "content": "hello",
            "evidence_ids": [],
            "created_at": "2026-05-26T00:00:00Z",
            "delivery": null
        }"#;
    let legacy: RegistryMessage =
        serde_json::from_str(legacy_json).expect("deserialize legacy message");
    assert_eq!(legacy.sender_kind, SenderKind::Agent);
    assert!(legacy.validate().is_ok());

    // An operator-authored message uses the reserved "operator" from id and
    // round-trips its sender_kind without loss.
    let operator = RegistryMessage {
        id: "msg-op".to_string(),
        task_id: None,
        from_agent_id: "operator".to_string(),
        to_agent_id: Some("agent-1".to_string()),
        channel: None,
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: "do the thing".to_string(),
        evidence_ids: vec![],
        created_at: "2026-05-26T00:00:00Z".to_string(),
        delivery: None,
        sender_kind: SenderKind::Operator,
    };
    let json = serde_json::to_string(&operator).expect("serialize operator message");
    assert!(
        json.contains("\"sender_kind\":\"operator\""),
        "operator message must serialize sender_kind as snake_case: {json}"
    );
    let parsed: RegistryMessage =
        serde_json::from_str(&json).expect("deserialize operator message");
    assert_eq!(parsed, operator);
    assert_eq!(parsed.sender_kind, SenderKind::Operator);
    assert!(parsed.validate().is_ok());
}
