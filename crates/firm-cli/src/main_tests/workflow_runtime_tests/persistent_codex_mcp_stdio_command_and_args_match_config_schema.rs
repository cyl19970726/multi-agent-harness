use super::*;

#[test]
fn persistent_codex_mcp_stdio_command_and_args_match_config_schema() {
    let spec = launch_spec_with_mcp(Some(LaunchMcp {
        servers: vec![mcp_stdio_server("filesys", &["npx", "-y", "pkg"])],
    }));
    let mut cmd = Command::new("codex");
    apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

    assert_eq!(
        command_args(&cmd),
        vec![
            "-c",
            "mcp_servers.filesys.command=\"npx\"",
            "-c",
            "mcp_servers.filesys.args=[\"-y\",\"pkg\"]"
        ]
    );
}
