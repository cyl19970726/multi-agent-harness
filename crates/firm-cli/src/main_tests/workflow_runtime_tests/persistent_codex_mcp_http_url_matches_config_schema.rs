use super::*;

#[test]
fn persistent_codex_mcp_http_url_matches_config_schema() {
    let spec = launch_spec_with_mcp(Some(LaunchMcp {
        servers: vec![mcp_http_server("remote", "https://example.com/mcp")],
    }));
    let mut cmd = Command::new("codex");
    apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

    assert_eq!(
        command_args(&cmd),
        vec!["-c", "mcp_servers.remote.url=\"https://example.com/mcp\""]
    );
}
