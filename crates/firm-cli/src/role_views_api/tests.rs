use super::*;
use std::io::Write;
use std::path::PathBuf;

#[test]
fn provider_native_session_read_is_local_operator_only() {
    let mut identity = ReadIdentity {
        actor: ActorRef {
            kind: ActorKind::AgentMember,
            id: "member-a".into(),
        },
        authority_actors: vec![ActorRef {
            kind: ActorKind::AgentMember,
            id: "host-a".into(),
        }],
        local_operator: false,
    };
    assert!(
        !identity.may_read_native_session(),
        "an AgentMember credential is not a provider-transcript grant"
    );
    identity.local_operator = true;
    assert!(identity.may_read_native_session());
}

#[test]
fn role_view_exposes_host_work_accept_only_to_an_exact_active_peer() {
    assert_eq!(
        work_review_disabled(Some("host-a"), "host-a", true, false),
        Some("Host-owned Work requires one exact active non-owner Team peer reviewer"),
        "Host owner must not see self-accept as enabled"
    );
    assert_eq!(
        work_review_disabled(Some("host-a"), "host-a", false, true),
        None,
        "one exact active peer may accept Host-owned Work"
    );
    assert_eq!(
        work_review_disabled(Some("member-a"), "host-a", true, false),
        None,
        "ordinary Member Work remains Host-reviewed"
    );
    assert_eq!(
        work_review_disabled(Some("member-a"), "host-a", false, true),
        Some("authenticated actor is not this Team's exact Host"),
        "a peer reviewer cannot accept another Member's Work"
    );
}

#[test]
fn query_is_closed_and_bounded() {
    assert!(Query::parse("/v1/views/global-work?limit=201").is_err());
    assert!(Query::parse("/v1/views/agent-workspace/team-a?session_limit=201").is_err());
    assert!(Query::parse("/v1/views/agent-workspace/team-a?session_before=old").is_err());
    assert!(Query::parse(
        "/v1/views/agent-workspace/team-a?agent_id=member-a&session_before=81&session_limit=80"
    )
    .is_ok());
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
fn empty_viewer_context_is_authenticated_zero_match_and_read_only() {
    let root = PathBuf::from(format!(
        "/tmp/agentfirm-viewer-context-purity-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    let identity = ReadIdentity {
        actor: ActorRef {
            kind: ActorKind::AgentMember,
            id: "member-with-no-team".into(),
        },
        authority_actors: Vec::new(),
        local_operator: false,
    };
    let view = viewer_context_view("space-empty", &HarnessStore::new(&root), Some(&identity))
        .expect("authenticated zero-Team context is a valid read");
    assert_eq!(view["view_kind"], json!("viewer_context"));
    assert_eq!(view["data"]["teams"], json!([]));
    assert_eq!(
        view["data"]["viewer_actor_ref"],
        json!({"kind":"agent_member","id":"member-with-no-team"})
    );
    assert!(
        !root.exists(),
        "read-only ViewerContext must not initialize a Store"
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

#[test]
fn work_graph_derives_hard_edges_and_canonical_ready_attention_sets() {
    let works = vec![
        json!({
            "work_id":"work-a",
            "prerequisite_work_ids":[],
            "readiness":{"state":"ready"}
        }),
        json!({
            "work_id":"work-b",
            "prerequisite_work_ids":["work-a"],
            "readiness":{"state":"waiting_prerequisites"}
        }),
        json!({
            "work_id":"work-c",
            "prerequisite_work_ids":["work-a","work-b"],
            "readiness":{"state":"requires_host_attention"}
        }),
    ];
    let graph = work_graph(&works);
    assert_eq!(graph["nodes"].as_array(), Some(&works));
    assert_eq!(graph["ready_work_ids"], json!(["work-a"]));
    assert_eq!(graph["attention_work_ids"], json!(["work-c"]));
    assert_eq!(
        graph["edges"],
        json!([
            {"prerequisite_work_id":"work-a","dependent_work_id":"work-b","kind":"hard"},
            {"prerequisite_work_id":"work-a","dependent_work_id":"work-c","kind":"hard"},
            {"prerequisite_work_id":"work-b","dependent_work_id":"work-c","kind":"hard"}
        ])
    );
}
