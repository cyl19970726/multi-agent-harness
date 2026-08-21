use super::*;

#[test]
fn remote_fabric_mcp_surface_is_read_only_and_server_resolves_local_node() {
    let home = TempHome::new("mcp-remote-fabric-read");
    let project_id = init_project(&home, "mcp-remote-fabric-project");
    let project_root = home.base().join("mcp-remote-fabric-project");
    let node = run_firm(&home, &project_root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let mut mcp = McpClient::spawn(&home, &project_id, &[]);
    let listed = mcp.request("tools/list", serde_json::json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools array");
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "remote_fabric_status"));
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "remote_fabric_operation_show"));

    let status = call_payload(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "remote_fabric_status",
            "arguments": {"company_id": "company-mcp-test"}
        }),
    ));
    assert_eq!(status["read_only"].as_bool(), Some(true));
    assert_eq!(status["company_id"], "company-mcp-test");
    assert!(status["local_node_id"].is_string());
    assert!(status["node_local"].is_null());
    assert!(status["control_plane"].is_null());

    let error = call_error_text(&mcp.request(
        "tools/call",
        serde_json::json!({
            "name": "remote_fabric_operation_show",
            "arguments": {"company_id": "company-mcp-test", "operation_id": "operation-1"}
        }),
    ));
    assert!(error.contains("Control Plane Store is unavailable"));
}
