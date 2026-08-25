use super::*;

#[test]
fn role_views_allow_loopback_operator_reads_and_gets_are_store_pure() {
    let home = TempHome::new("role-views-http");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind":"human","id":"local-operator"},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let route = format!("/v1/views/global-work?project={project_id}");
    let before = ledger_digest(serve.fixture_store_root());
    let (status, local) = serve.get_json(&route);
    assert_eq!(status, 200, "loopback Operator RoleView: {local}");
    assert_eq!(local["view_kind"], "global_work");
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before,
        "loopback GET changed canonical ledgers"
    );
    let viewer_route = format!("/v1/views/viewer-context?project={project_id}");
    let (status, viewer) = serve.get_json(&viewer_route);
    assert_eq!(status, 200, "loopback ViewerContext: {viewer}");
    assert_eq!(viewer["view_kind"], "viewer_context");
    assert_eq!(
        viewer["data"]["viewer_actor_ref"]["id"],
        "local-dashboard-operator"
    );
    assert_eq!(viewer["data"]["teams"], serde_json::json!([]));
    let (status, invalid) =
        serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", "invalid-token")]);
    assert_eq!(status, 401, "invalid runtime context: {invalid}");
    assert_eq!(invalid["error"]["code"], "NOT_AUTHORIZED");
    let (status, global) = serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Global Work RoleView: {global}");
    assert_eq!(global["view_kind"], "global_work");
    assert_eq!(global["schema_version"], "agentfirm.role_views.v1");
    assert_eq!(global["data"]["items"], serde_json::json!([]));
    assert_eq!(
        global["data"]["pending_migration_work_ids"],
        serde_json::json!([])
    );
    assert_eq!(
        global["data"]["page"]["next_cursor"],
        serde_json::Value::Null
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before,
        "GET changed canonical ledgers"
    );

    for path in [
        "/v1/views/team-workspace/missing",
        "/v1/views/host-console/missing",
        "/v1/views/agent-workspace/missing",
        "/v1/views/member-workbench/missing",
        "/v1/views/operator/missing",
        // Retired by the Global Work cutover (DOC-106): the Company Work view
        // name no longer resolves.
        "/v1/views/company-work",
    ] {
        let route = format!("{path}?project={project_id}");
        let (status, body) = serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", TOKEN)]);
        assert_eq!(status, 404, "{path}: {body}");
    }
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before,
        "404 GETs changed canonical ledgers"
    );
}
