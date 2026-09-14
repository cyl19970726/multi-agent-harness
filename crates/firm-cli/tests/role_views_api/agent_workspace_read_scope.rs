use super::*;

/// The AgentWorkspace RoleView's read contract: who may read a member's
/// workspace, what a cross-binding or non-owner read is allowed to see, how the
/// exact Host read differs from the owner's, and that an attach leaves the
/// projection `attached`. Provider-native history stays behind the exact
/// session the reader is entitled to.
///
/// Split out of `role_action_loop_is_authenticated_cas_bound_and_legacy_writers_are_gone`
/// (#949): these are read-only assertions over the loop's state -- three
/// inputs, nothing handed back.
pub(super) struct AgentWorkspaceReadScopeContext<'a> {
    pub serve: &'a ServeHandle,
    pub home: &'a TempHome,
    pub root: &'a std::path::Path,
    pub store: &'a HarnessStore,
    pub space_id: &'a str,
    pub project_id: &'a str,
    pub other_project_id: &'a str,
    pub run_id: &'a str,
    pub worker_id: &'a str,
    pub sibling_worker_id: &'a str,
    pub member_run_id: &'a str,
    pub team: &'a harness_core::AgentTeam,
}

pub(super) fn assert_agent_workspace_read_scope(context: AgentWorkspaceReadScopeContext<'_>) {
    let AgentWorkspaceReadScopeContext {
        serve,
        home,
        root,
        store,
        space_id,
        project_id,
        other_project_id,
        run_id,
        worker_id,
        sibling_worker_id,
        member_run_id,
        team,
    } = context;
    let member_agent_workspace_route =
        format!("/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={worker_id}");
    let (status, host_selected_member) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", TOKEN)],
    );
    assert_eq!(
        status, 200,
        "Host-selected Member AgentWorkspace: {host_selected_member}"
    );
    assert_eq!(host_selected_member["view_kind"], "agent_workspace");
    assert_eq!(
        host_selected_member["data"]["projection_scope"],
        "team_session_read"
    );
    assert_eq!(
        host_selected_member["data"]["selected_agent"]["agent_member_ref"]["id"],
        worker_id
    );
    assert_eq!(
        host_selected_member["data"]["selected_agent"]["current_member_run_ref"], member_run_id,
        "Host-selected Team Session read resolves the exact MemberRun binding"
    );
    assert!(host_selected_member["data"]
        .get("persisted_session_projection")
        .is_some());
    assert!(host_selected_member["data"]
        .get("current_session")
        .is_some());
    assert!(host_selected_member["data"]
        .get("live_provider_activity")
        .is_none());
    assert!(host_selected_member["data"]
        .get("session_event_projection")
        .is_none());
    assert!(host_selected_member["data"].get("runtime_fabric").is_none());
    assert!(
        host_selected_member["allowed_actions"]
            .as_array()
            .expect("Host public controls")
            .iter()
            .any(|action| action["kind"] == "close_member_run"
                && action["target_ref"]["id"] == member_run_id),
        "Host control projection remains available beside the Team Session read"
    );
    let before_owner_projection_ledgers = ledger_digest(serve.fixture_store_root());
    let before_owner_projection_source = file_tree_digest(&home.home().join(".codex"));
    let (status, member_self_workspace) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "exact-self Member AgentWorkspace: {member_self_workspace}"
    );
    assert!(member_self_workspace["data"]
        .get("live_provider_activity")
        .is_none());
    assert!(
    member_self_workspace["data"]
        .get("persisted_session_projection")
        .is_some(),
    "exact-self view must carry a persisted Session projection or an explicit unavailable result"
);
    let current_session = &member_self_workspace["data"]["current_session"];
    assert!(current_session["agent_session_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("agent-session:")));
    assert_eq!(current_session["provider"], "codex");
    assert_eq!(
        member_self_workspace["data"]["configuration"]["effective_permission_ceiling"],
        current_session["effective_permission_ceiling"]
    );
    let owner_projection = &member_self_workspace["data"]["persisted_session_projection"];
    assert_eq!(owner_projection["available"], true);
    assert!(owner_projection["records"]
        .as_array()
        .expect("persisted native records")
        .iter()
        .any(|record| record["provider_turn_id"] == "turn-owner-1"));
    let serialized_owner_projection =
        serde_json::to_string(owner_projection).expect("projection JSON");
    assert!(serialized_owner_projection.contains("display-safe authored result"));
    assert!(serialized_owner_projection.contains("raw-chain-of-thought-must-not-appear"));
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_owner_projection_ledgers,
        "on-demand provider projection must not write Harness ledgers"
    );
    assert_eq!(
        file_tree_digest(&home.home().join(".codex")),
        before_owner_projection_source,
        "on-demand provider projection must not rewrite provider-native storage"
    );
    let cross_binding_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={other_project_id}&agent_id={worker_id}"
    );
    let (status, cross_binding_workspace) =
        serve.get_json_with_headers(&cross_binding_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "cross-binding owner view");
    let cross_binding_projection = &cross_binding_workspace["data"]["persisted_session_projection"];
    assert!(
    cross_binding_projection["available"] == false,
    "same Execution Space must not expose a Session through another Project Binding: {cross_binding_workspace}"
);
    assert_eq!(
        cross_binding_projection["agent_session_id"],
        serde_json::Value::Null,
        "cross-binding projection must not expose an AgentSession id"
    );
    assert_eq!(
        cross_binding_workspace["data"].get("live_provider_activity"),
        None,
        "same Execution Space must not expose the retired live overlay"
    );
    for retired_history_field in ["sessions", "selected_session_id", "session_activity"] {
        assert!(
            member_self_workspace["data"]
                .get(retired_history_field)
                .is_none(),
            "legacy provider field {retired_history_field} must remain retired"
        );
    }
    let (status, sibling_local_operator) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", SIBLING_MEMBER_TOKEN)],
    );
    assert_eq!(
    status, 200,
    "loopback sibling context gets only the local Operator read projection: {sibling_local_operator}"
);
    assert_eq!(
        sibling_local_operator["data"]["projection_scope"],
        "team_session_read"
    );
    assert_eq!(
        sibling_local_operator["allowed_actions"],
        serde_json::json!([]),
        "local Operator read must not borrow sibling mutation authority"
    );
    let sibling_self_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={sibling_worker_id}"
    );
    let (status, sibling_self_unavailable) = serve.get_json_with_headers(
        &sibling_self_route,
        &[("X-AgentFirm-Token", SIBLING_MEMBER_TOKEN)],
    );
    assert_eq!(status, 200, "unavailable exact-self projection");
    let unavailable = &sibling_self_unavailable["data"]["persisted_session_projection"];
    assert_eq!(unavailable["available"], false);
    assert!(unavailable["reason_code"].as_str().is_some());
    let host_agent_workspace_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={}",
        team.host_agent_id
    );
    let (status, exact_host_workspace) =
        serve.get_json_with_headers(&host_agent_workspace_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 200,
        "exact Host AgentWorkspace: {exact_host_workspace}"
    );
    assert_eq!(
        exact_host_workspace["data"]["selected_agent"]["is_host"],
        true
    );
    assert!(
        exact_host_workspace["data"]
            .get("persisted_session_projection")
            .is_some(),
        "exact Host self view carries an explicit owner projection state"
    );
    let (status, member_local_operator_host) = serve.get_json_with_headers(
        &host_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
    status, 200,
    "loopback Member context gets only the local Operator Host projection: {member_local_operator_host}"
);
    assert_eq!(
        member_local_operator_host["data"]["projection_scope"],
        "team_session_read"
    );
    assert_eq!(
        member_local_operator_host["allowed_actions"],
        serde_json::json!([]),
        "local Operator Host read must not borrow Host mutation authority"
    );
    let member_run_version = store
        .trust_member_runs(space_id)
        .expect("MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("canonical MemberRun")
        .version
        .to_string();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "role-view@example.invalid"],
        vec!["config", "user.name", "Role View Test"],
        vec!["add", "-A"],
        vec!["commit", "--allow-empty", "-m", "workspace proof fixture"],
    ] {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(&args)
            .output()
            .expect("run git workspace fixture command");
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }
    let before_hostile_workspace = ledger_digest(serve.fixture_store_root());
    let workspace_route = format!(
        "/v1/agentfirm/member-runs/{member_run_id}/workspace/provision?project={project_id}"
    );
    let workspace_headers = action_headers(
        MEMBER_TOKEN,
        "hostile-workspace-escape",
        member_run_version.as_str(),
    );
    let (status, workspace_rejected) = serve.post_json_with_headers(
        &workspace_route,
        &serde_json::json!({
            "action":"provision_workspace",
            "project_binding_id":project_id,
            "mode":"inherit",
            "ownership":"shared_project",
            "canonical_root":home.base()
        }),
        &workspace_headers,
    );
    assert_eq!(
        status, 409,
        "workspace escape must fail closed: {workspace_rejected}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_hostile_workspace,
        "hostile workspace intent changed durable state"
    );
    let safe_workspace_headers = action_headers(
        TOKEN,
        "safe-workspace-provision",
        member_run_version.as_str(),
    );
    let (status, provisioned_workspace) = serve.post_json_with_headers(
        &workspace_route,
        &serde_json::json!({
            "action":"provision_workspace",
            "project_binding_id":project_id,
            "mode":"worktree",
            "ownership":"managed",
            "canonical_root":root
        }),
        &safe_workspace_headers,
    );
    assert_eq!(
        status, 200,
        "server-observed workspace provision: {provisioned_workspace}"
    );
    assert!(provisioned_workspace["projection"]["git_common_dir"]
        .as_str()
        .is_some());
    assert_eq!(provisioned_workspace["projection"]["lifecycle"], "ready");
    let attach_workspace_route =
        format!("/v1/agentfirm/member-runs/{member_run_id}/workspace/attach?project={project_id}");
    let attached_workspace = assert_exact_role_action_replay(
        serve,
        &attach_workspace_route,
        &serde_json::json!({"action":"attach_workspace"}),
        &action_headers(TOKEN, "safe-workspace-attach", "3"),
        "workspace attach",
    );
    assert_eq!(attached_workspace["projection"]["lifecycle"], "attached");
}
