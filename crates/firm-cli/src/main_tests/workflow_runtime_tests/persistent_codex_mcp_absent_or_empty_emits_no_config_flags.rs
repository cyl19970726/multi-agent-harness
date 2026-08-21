use super::*;

    #[test]
    fn persistent_codex_mcp_absent_or_empty_emits_no_config_flags() {
        for spec in [
            launch_spec_with_mcp(None),
            launch_spec_with_mcp(Some(LaunchMcp {
                servers: Vec::new(),
            })),
        ] {
            let mut cmd = Command::new("codex");
            apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

            let args = command_args(&cmd);
            assert!(args.is_empty());
            assert!(!args.iter().any(|arg| arg.contains("mcp_servers")));
        }
    }

