use super::*;

#[test]
fn operator_remote_fabric_never_invents_green_health_from_local_journal() {
    let home = TempHome::new("operator-remote-fabric-health");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let node: serde_json::Value =
        serde_json::from_slice(&run_firm(&home, &root, &["node", "init"]).stdout)
            .expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");

    let layout =
        harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(home.firm_home())
            .expect("Remote Fabric layout");
    let local = layout
        .open_node_local("company-test", node_id)
        .expect("Node-local journal");
    local
        .bind_gateway_session(&harness_fabric::transport::FabricSessionFence {
            company_id: "company-test".into(),
            node_id: node_id.into(),
            gateway_generation: 1,
            node_daemon_id: format!("node-daemon:{node_id}"),
            node_daemon_generation: 1,
            control_plane_generation: 1,
        })
        .expect("local journal has an observed session");

    let credentials = serde_json::json!([{
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let route = format!("/v1/views/operator/{node_id}?project={project_id}&company=company-test");
    let (status, operator) =
        serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator view: {operator}");
    let fabric = &operator["data"]["remote_fabric"];
    assert_eq!(fabric["state"], "unknown");
    assert_eq!(fabric["control_plane_online"], serde_json::Value::Null);
    assert_eq!(fabric["control_plane_metrics"], serde_json::Value::Null);
    assert!(fabric["reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("not health truth")));
}
