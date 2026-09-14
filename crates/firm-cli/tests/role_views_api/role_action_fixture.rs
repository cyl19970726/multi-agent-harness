use super::*;

/// Stable identities the role-action loop and its extracted scenarios share.
pub(super) const HOST_ID: &str = "agent-role-action-host";
pub(super) const WORKER_ID: &str = "agent-role-action-worker";
pub(super) const SIBLING_WORKER_ID: &str = "agent-role-action-sibling";
pub(super) const WORKER_NATIVE_SESSION_ID: &str = "019f-role-view-owner-session";

/// Everything the role-action loop needs before a server exists: the project
/// and Execution Space, the registered node, legacy Mission provenance, the
/// Team and its canonical AgentMembers, the HTTP credentials, and the second
/// Execution Space plus second Project Binding the cross-binding assertions
/// need.
///
/// Split out of `role_action_loop_is_authenticated_cas_bound_and_legacy_writers_are_gone`
/// (#949): it is pure setup, reads none of the loop's state, and its outputs
/// are exactly these fields.
pub(super) struct RoleActionFixture {
    pub home: TempHome,
    pub root: std::path::PathBuf,
    pub project_id: String,
    pub space_id: String,
    pub node_id: String,
    pub mission_id: String,
    pub store: HarnessStore,
    pub team: harness_core::AgentTeam,
    pub credentials: String,
    pub other_project_id: String,
}

pub(super) fn seed_role_action_fixture() -> RoleActionFixture {
    let home = TempHome::new("role-action-loop");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let space_id = current_space_id(&home);
    let worker_native_session_id = WORKER_NATIVE_SESSION_ID;
    let rollout_dir = home.home().join(".codex/sessions/2026/08/13");
    std::fs::create_dir_all(&rollout_dir).expect("Codex rollout fixture root");
    std::fs::write(
    rollout_dir.join(format!(
        "rollout-2026-08-13T00-00-00-{worker_native_session_id}.jsonl"
    )),
    format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{worker_native_session_id}\"}}}}\n\
         {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_reasoning\",\"turn_id\":\"turn-owner-1\",\"text\":\"raw-chain-of-thought-must-not-appear\"}}}}\n\
         {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"turn_id\":\"turn-owner-1\",\"message\":\"display-safe authored result\"}}}}\n\
         {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn-owner-1\"}}}}\n"
    ),
)
.expect("Codex rollout fixture");
    let run = |args: &[&str]| {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend_from_slice(args);
        let output = run_firm(&home, &root, &full);
        assert!(output.status.success(), "fixture {args:?}: {output:?}");
        output
    };
    let node: serde_json::Value =
        serde_json::from_slice(&run(&["node", "init"]).stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--project-binding-id",
        &project_id,
    ]);
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    let mission_id = "mission-role-action-loop".to_string();
    firm_env::seed_historical_mission(&home, &project_id, &mission_id, "Role action loop");
    let host_id = HOST_ID;
    let host = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        host_id,
        "Role Action Host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host: {host:?}");
    let worker_id = WORKER_ID;
    let worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        worker_id,
        "Role Action Worker",
        "builder",
        "codex",
        &[],
    );
    assert!(worker.status.success(), "worker: {worker:?}");
    let sibling_worker_id = SIBLING_WORKER_ID;
    let sibling_worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        sibling_worker_id,
        "Role Action Sibling",
        "builder",
        "codex",
        &[],
    );
    assert!(
        sibling_worker.status.success(),
        "sibling worker: {sibling_worker:?}"
    );
    run(&[
        "team",
        "create",
        "--name",
        "Role action team",
        "--description",
        "Store-live action integration",
        "--mission-id",
        &mission_id,
        "--host-agent-id",
        host_id,
        "--node-id",
        node_id,
        "--member",
        host_id,
        "--member",
        worker_id,
        "--member",
        sibling_worker_id,
    ]);
    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    let sibling_node_id = "10000000-0000-4000-8000-000000000002";
    store
        .insert_execution_node(&ExecutionNode {
            id: sibling_node_id.into(),
            display_name: "Sibling execution node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "2026-08-10T00:00:00Z".into(),
            updated_at: "2026-08-10T00:00:00Z".into(),
        })
        .expect("insert sibling Node");
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: sibling_node_id.into(),
                execution_space_id: space_id.clone(),
                project_binding_id: project_id.clone(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "2026-08-10T00:00:00Z".into(),
                updated_at: "2026-08-10T00:00:00Z".into(),
            },
            &space_id,
        )
        .expect("register sibling Node");
    let team = store
        .latest_teams()
        .expect("teams")
        .into_values()
        .next()
        .expect("default team");
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind":"agent_member","id":team.host_agent_id},
        "authority_actors": []
    },{
        "token": MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":worker_id},
        "authority_actors": []
    },{
        "token": SIBLING_MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":sibling_worker_id},
        "authority_actors": []
    },{
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
        "authority_actors": []
    },{
        "token": WRONG_OPERATOR_TOKEN,
        "actor": {"kind":"service","id":sibling_node_id},
        "authority_actors": []
    },{
        "token": DELEGATED_OPERATOR_TOKEN,
        "actor": {"kind":"human","id":"operator-human"},
        "authority_actors": [{"kind":"service","id":node_id}]
    }])
    .to_string();
    let other_space = run_firm(
        &home,
        &root,
        &[
            "space",
            "init",
            "--id",
            "role-action-empty-space",
            "--name",
            "Role Action Empty Space",
            "--project-binding",
            &project_id,
        ],
    );
    assert!(other_space.status.success(), "other space: {other_space:?}");
    let other_project_root = home.base().join("other-project-binding");
    std::fs::create_dir_all(&other_project_root).expect("other project root");
    let other_project = run_firm(&home, &other_project_root, &["init"]);
    assert!(
        other_project.status.success(),
        "other Project Binding: {other_project:?}"
    );
    let other_project_id = current_project_id(&home);
    assert_ne!(
        other_project_id, project_id,
        "cross-binding test requires two distinct Project Bindings"
    );
    let restored_project = run_firm(&home, &root, &["project", "switch", project_id.as_str()]);
    assert!(
        restored_project.status.success(),
        "restore primary Project Binding: {restored_project:?}"
    );
    RoleActionFixture {
        node_id: node_id.to_string(),
        home,
        root,
        project_id,
        space_id,
        mission_id,
        store,
        team,
        credentials,
        other_project_id,
    }
}
