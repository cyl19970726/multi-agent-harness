use super::*;

#[test]
fn persistent_codex_mcp_single_command_omits_args() {
    let spec = launch_spec_with_mcp(Some(LaunchMcp {
        servers: vec![mcp_stdio_server("single", &["mcp-bin"])],
    }));
    let mut cmd = Command::new("codex");
    apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

    let args = command_args(&cmd);
    assert_eq!(args, vec!["-c", "mcp_servers.single.command=\"mcp-bin\""]);
    assert!(!args.iter().any(|arg| arg.contains(".args=")));
}
