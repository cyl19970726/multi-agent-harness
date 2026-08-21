use super::*;
use std::io::Write;
use std::path::PathBuf;
#[test]
fn query_is_closed_and_bounded() {
    assert!(Query::parse("/v1/views/global-work?limit=201").is_err());
    assert!(Query::parse("/v1/views/global-work?mystery=x").is_err());
    assert_eq!(
        Query::parse("/v1/views/global-work?team_id=a&team_id=b")
            .unwrap()
            .values["team_id"],
        ["a", "b"]
    );
    assert_eq!(
        Query::parse("/v1/views/global-work?assignee_kind=unassigned")
            .unwrap()
            .values["assignee_kind"],
        ["unassigned"]
    );
}

#[test]
fn empty_global_view_is_zero_match_and_read_only() {
    let root = PathBuf::from(format!(
        "/tmp/agentfirm-role-view-purity-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    let stores = vec![("space-empty".to_string(), HarnessStore::new(&root))];
    let view = global_work_view(&stores, &Query::parse("/v1/views/global-work").unwrap()).unwrap();
    assert_eq!(view["view_kind"], json!("global_work"));
    assert_eq!(view["data"]["items"], json!([]));
    assert_eq!(view["data"]["pending_migration_work_ids"], json!([]));
    assert_eq!(view["data"]["page"]["next_cursor"], Value::Null);
    assert!(
        !root.exists(),
        "read-only RoleView must not initialize a Store"
    );
}

#[test]
fn historical_duplicate_active_membership_fails_role_view_closed() {
    let duplicate = vec![
        json!({"id":"membership-1","team_id":"team-1","agent_member_id":"agent-1","state":"active","membership_generation":1}),
        json!({"id":"membership-2","team_id":"team-1","agent_member_id":"agent-1","state":"active","membership_generation":2}),
    ];
    let error = ensure_active_membership_cardinality(&duplicate)
        .expect_err("ambiguous historical authority must fail closed");
    assert!(error.contains("IDENTITY_CONFLICT"));
}

#[test]
fn host_delivery_reconcile_projection_is_team_scoped() {
    let team_work_ids = BTreeSet::from(["work-team-a"]);
    let team_delivery = json!({"id":"delivery-a","work_id":"work-team-a","status":"failed"});
    let sibling_delivery = json!({"id":"delivery-b","work_id":"work-team-b","status":"failed"});
    assert!(delivery_requires_team_reconcile(
        &team_delivery,
        &team_work_ids
    ));
    assert!(!delivery_requires_team_reconcile(
        &sibling_delivery,
        &team_work_ids
    ));
}

#[test]
fn collaboration_projection_filters_by_team_before_any_page_limit() {
    let root = PathBuf::from(format!(
        "/tmp/agentfirm-role-view-collaboration-page-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let fixture =
        serde_json::from_str::<harness_core::collaboration::WorkDelegationV1>(include_str!(
            "../../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
        ))
        .unwrap();
    let ledger = root.join("agentfirm_collaboration_operations.jsonl");
    let mut writer = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&ledger)
        .unwrap();
    for index in 0..=205_u64 {
        let mut delegation = fixture.clone();
        delegation.id = if index == 205 {
            "zzz-visible-after-company-first-200".into()
        } else {
            format!("noise-{index:03}")
        };
        delegation.source_work_attestation_id = format!("attestation-{index}");
        delegation.source_work_ref.work_id = format!("source-work-{index}");
        delegation.source_team_id = "noise-source-team".into();
        delegation.source_work_ref.team_id = delegation.source_team_id.clone();
        delegation.target_placement.team_id = if index == 205 {
            "team-visible".into()
        } else {
            "noise-target-team".into()
        };
        let operation = harness_store::CollaborationOperation {
            store_version: harness_core::collaboration::COLLABORATION_STORE_VERSION.into(),
            company_id: "company-1".into(),
            command_name: "fixture.insert".into(),
            authenticated_actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: "fixture".into(),
            },
            idempotency_key: format!("fixture-{index}"),
            request_fingerprint: format!("sha256:{index:064x}"),
            aggregate_kind: "work_delegation_v1".into(),
            aggregate_id: delegation.id.clone(),
            store_sequence: index + 1,
            resulting_revision: delegation.revision,
            resulting_projection: serde_json::to_value(&delegation).unwrap(),
            immutable_side_records: Vec::new(),
            created_at: format!("unix-ms:{index}"),
        };
        writeln!(writer, "{}", serde_json::to_string(&operation).unwrap()).unwrap();
    }
    writer.flush().unwrap();
    let (as_of, visible) =
        list_team_collaboration_delegations(&HarnessStore::new(&root), "company-1", "team-visible")
            .unwrap();
    assert_eq!(as_of, 206);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, "zzz-visible-after-company-first-200");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn message_delivery_state_distinguishes_every_canonical_outcome() {
    let row = |status: &str| json!({"status": status});
    assert_eq!(message_delivery_state(&[]), "unsettled");
    assert_eq!(message_delivery_state(&[&row("queued")]), "queued");
    assert_eq!(message_delivery_state(&[&row("routed")]), "queued");
    assert_eq!(message_delivery_state(&[&row("claimed")]), "delivered");
    assert_eq!(
        message_delivery_state(&[&row("provider_received")]),
        "delivered"
    );
    assert_eq!(
        message_delivery_state(&[&row("acknowledged")]),
        "acknowledged"
    );
    assert_eq!(message_delivery_state(&[&row("failed")]), "failed");
    assert_eq!(message_delivery_state(&[&row("expired")]), "failed");
    assert_eq!(message_delivery_state(&[&row("invalidated")]), "failed");
    assert_eq!(
        message_delivery_state(&[&row("acknowledged"), &row("queued")]),
        "queued",
        "one pending recipient keeps the Message queued"
    );
    assert_eq!(
        message_delivery_state(&[&row("acknowledged"), &row("provider_received")]),
        "delivered"
    );
    assert_eq!(
        message_delivery_state(&[&row("acknowledged"), &row("failed")]),
        "failed"
    );
}

fn host_run_fixture(host_thread_id: Option<&str>, mode: HostControlMode) -> AgentTeamRun {
    AgentTeamRun {
        id: "run-1".into(),
        agent_team_id: "team-1".into(),
        execution_node_id: "node-1".into(),
        project_binding_id: "project-1".into(),
        previous_run_id: None,
        host_surface: "codex".into(),
        host_thread_id: host_thread_id.map(str::to_owned),
        host_actor: None,
        host_control_mode: mode,
        objective: "test".into(),
        execution_root: None,
        status: harness_core::TeamRunStatus::Running,
        member_run_ids: Vec::new(),
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    }
}

#[test]
fn host_session_mode_distinguishes_managed_external_and_unbound() {
    assert_eq!(
        host_session_mode(Some(&host_run_fixture(
            Some("thread-1"),
            HostControlMode::External
        ))),
        "external_interactive"
    );
    assert_eq!(
        host_session_mode(Some(&host_run_fixture(
            Some("thread-1"),
            HostControlMode::Managed
        ))),
        "harness_managed"
    );
    assert_eq!(
        host_session_mode(Some(&host_run_fixture(None, HostControlMode::External))),
        "unbound"
    );
    assert_eq!(
        host_session_mode(Some(&host_run_fixture(
            Some("  "),
            HostControlMode::Managed
        ))),
        "unbound",
        "a blank thread id is not a binding"
    );
    assert_eq!(host_session_mode(None), "unbound");
}

#[test]
fn exact_session_history_survives_a_member_adapter_generation_change() {
    let sessions = vec![json!({
        "id":"agent-session-1",
        "execution_space_id":"space-1",
        "agent_member_id":"member-1",
        "lifecycle":"idle",
        "provider_kind":"codex",
        "runtime_generation":1,
        "native_session_ref":{
            "provider":"codex",
            "execution_mode":"codex_app_server",
            "native_session_id":"native-thread-1",
            "native_locator_kind":"codex_rollout",
            "adapter_contract_version":"codex-app-server-v1",
            "availability":"available",
            "supports_resume":true
        }
    })];

    // A MemberRun may now be adapter generation 2 after Reopen while this
    // machine-owned AgentSession remains generation 1. Exact identity,
    // provider and native-session binding still authorize owner history.
    let (session, native) = exact_agent_session_binding(
        &sessions,
        "space-1",
        "member-1",
        "native-thread-1",
        Some("codex_app_server"),
    )
    .expect("same native AgentSession remains the exact history authority");
    assert_eq!(session["runtime_generation"], 1);
    assert_eq!(native.native_session_id, "native-thread-1");
}

#[test]
fn interrupt_action_requires_the_exact_active_verified_runtime_binding() {
    let member_run = json!({"id":"member-run-1","runtime_generation":2});
    let pending = json!({
        "id":"member-run-1",
        "runtime_generation":2,
        "provider_profile":{"capability_bindings":[{
            "capability":"interrupt_current_cycle",
            "status":"review_required",
            "admission":"pending_dependency"
        }]}
    });
    assert!(!member_run_has_active_provider_capability(
        &[pending],
        &member_run,
        "interrupt_current_cycle"
    ));

    let active = json!({
        "id":"member-run-1",
        "runtime_generation":2,
        "provider_profile":{"capability_bindings":[{
            "capability":"interrupt_current_cycle",
            "status":"verified",
            "admission":"active"
        }]}
    });
    assert!(member_run_has_active_provider_capability(
        &[active],
        &member_run,
        "interrupt_current_cycle"
    ));

    let stale_generation = json!({
        "id":"member-run-1",
        "runtime_generation":1,
        "provider_profile":{"capability_bindings":[{
            "capability":"interrupt_current_cycle",
            "status":"verified",
            "admission":"active"
        }]}
    });
    assert!(!member_run_has_active_provider_capability(
        &[stale_generation],
        &member_run,
        "interrupt_current_cycle"
    ));
}

#[test]
fn ready_capability_admission_requires_active_core_bindings() {
    let active_binding = |capability: &str| json!({"capability":capability,"status":"verified","admission":"active"});
    let active = json!({"capability_bindings":[
        active_binding("open_or_resume"),
        active_binding("start_cycle"),
        active_binding("observe")
    ]});
    assert_eq!(
        provider_core_capability_admission(Some(&active)).0,
        "active"
    );

    let pending = json!({"capability_bindings":[
        active_binding("open_or_resume"),
        {"capability":"start_cycle","status":"review_required","admission":"pending_dependency"},
        active_binding("observe")
    ]});
    assert_eq!(
        provider_core_capability_admission(Some(&pending)).0,
        "review_required"
    );

    let missing = json!({"capability_bindings":[active_binding("open_or_resume")]});
    assert_eq!(
        provider_core_capability_admission(Some(&missing)).0,
        "unavailable"
    );
    assert_eq!(provider_core_capability_admission(None).0, "unknown");
}
