use super::*;

    #[test]
    fn persistent_codex_mcp_quotes_non_bare_id_key_path() {
        let spec = launch_spec_with_mcp(Some(LaunchMcp {
            servers: vec![mcp_stdio_server("my id.v1", &["npx"])],
        }));
        let mut cmd = Command::new("codex");
        apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

        assert_eq!(
            command_args(&cmd),
            vec!["-c", "mcp_servers.\"my id.v1\".command=\"npx\""]
        );
    }

