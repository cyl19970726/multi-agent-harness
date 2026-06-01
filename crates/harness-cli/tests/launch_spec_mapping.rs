#[cfg(test)]
mod launch_spec_mapping_tests {
    use std::collections::HashMap;

    // Minimal mock types for testing (these would come from harness_core in real code)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum LaunchPermission {
        ReadOnly,
        WorkspaceWrite,
        FullAccess,
    }

    impl LaunchPermission {
        fn to_claude_mode(&self) -> &'static str {
            match self {
                LaunchPermission::ReadOnly => "plan",
                LaunchPermission::WorkspaceWrite => "acceptEdits",
                LaunchPermission::FullAccess => "bypassPermissions",
            }
        }

        fn to_codex_sandbox(&self) -> &'static str {
            match self {
                LaunchPermission::ReadOnly => "read-only",
                LaunchPermission::WorkspaceWrite => "workspace-write",
                LaunchPermission::FullAccess => "danger-full-access",
            }
        }
    }

    // Test cases for permission mapping
    #[test]
    fn test_launch_permission_to_claude_mode() {
        assert_eq!(LaunchPermission::ReadOnly.to_claude_mode(), "plan");
        assert_eq!(
            LaunchPermission::WorkspaceWrite.to_claude_mode(),
            "acceptEdits"
        );
        assert_eq!(
            LaunchPermission::FullAccess.to_claude_mode(),
            "bypassPermissions"
        );
    }

    #[test]
    fn test_launch_permission_to_codex_sandbox() {
        assert_eq!(LaunchPermission::ReadOnly.to_codex_sandbox(), "read-only");
        assert_eq!(
            LaunchPermission::WorkspaceWrite.to_codex_sandbox(),
            "workspace-write"
        );
        assert_eq!(
            LaunchPermission::FullAccess.to_codex_sandbox(),
            "danger-full-access"
        );
    }

    // Test command-line argument construction for Claude
    #[test]
    fn test_claude_argv_construction_with_model() {
        // Simulating the flag construction logic
        let mut args = vec!["claude".to_string(), "-p".to_string(), "prompt".to_string()];
        args.push("--output-format".to_string());
        args.push("stream-json".to_string());
        args.push("--verbose".to_string());

        let model = Some("claude-opus-4-8");
        if let Some(m) = model {
            args.push("--model".to_string());
            args.push(m.to_string());
        }

        let permission = LaunchPermission::WorkspaceWrite;
        let mode = permission.to_claude_mode();
        args.push("--permission-mode".to_string());
        args.push(mode.to_string());

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"claude-opus-4-8".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"acceptEdits".to_string()));
    }

    #[test]
    fn test_claude_argv_omits_flags_when_unset() {
        let mut args = vec!["claude".to_string(), "-p".to_string(), "prompt".to_string()];
        args.push("--output-format".to_string());
        args.push("stream-json".to_string());
        args.push("--verbose".to_string());

        let model: Option<&str> = None;
        if let Some(m) = model {
            args.push("--model".to_string());
            args.push(m.to_string());
        }

        // Model should not be in args when unset
        assert!(!args.contains(&"--model".to_string()));
    }

    #[test]
    fn test_codex_argv_construction_with_permission() {
        let mut args = vec!["codex".to_string(), "exec".to_string(), "--json".to_string()];

        let permission = LaunchPermission::ReadOnly;
        let sandbox = permission.to_codex_sandbox();
        args.push("--sandbox".to_string());
        args.push(sandbox.to_string());

        let model = Some("o3");
        if let Some(m) = model {
            args.push("-m".to_string());
            args.push(m.to_string());
        }

        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"read-only".to_string()));
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"o3".to_string()));
    }

    #[test]
    fn test_all_permission_values_mapped() {
        // Ensure all three permission values have valid mappings
        let permissions = vec![
            LaunchPermission::ReadOnly,
            LaunchPermission::WorkspaceWrite,
            LaunchPermission::FullAccess,
        ];

        for perm in permissions {
            let claude_mode = perm.to_claude_mode();
            let codex_sandbox = perm.to_codex_sandbox();

            // Should not be empty
            assert!(!claude_mode.is_empty());
            assert!(!codex_sandbox.is_empty());

            // Claude modes
            assert!(
                matches!(
                    claude_mode,
                    "plan" | "acceptEdits" | "bypassPermissions"
                ),
                "Invalid Claude permission mode: {}",
                claude_mode
            );

            // Codex sandboxes
            assert!(
                matches!(
                    codex_sandbox,
                    "read-only" | "workspace-write" | "danger-full-access"
                ),
                "Invalid Codex sandbox: {}",
                codex_sandbox
            );
        }
    }

    #[test]
    fn test_allowed_tools_comma_separation() {
        let tools = vec!["Bash", "Edit", "Read"];
        let tools_arg = tools.join(",");

        assert_eq!(tools_arg, "Bash,Edit,Read");

        // Verify it can be split back
        let recovered: Vec<&str> = tools_arg.split(',').collect();
        assert_eq!(recovered, vec!["Bash", "Edit", "Read"]);
    }

    #[test]
    fn test_empty_tools_omitted_from_args() {
        let mut args = vec!["claude".to_string(), "-p".to_string()];

        let tools: Vec<&str> = Vec::new();
        if !tools.is_empty() {
            args.push("--allowedTools".to_string());
            args.push(tools.join(","));
        }

        assert!(!args.contains(&"--allowedTools".to_string()));
    }

    #[test]
    fn test_workspace_roots_added_dir() {
        let mut args = vec!["claude".to_string(), "-p".to_string()];

        let workspace = Some("/path/to/workspace");
        if let Some(ws) = workspace {
            args.push("--add-dir".to_string());
            args.push(ws.to_string());
        }

        let writable_roots = vec!["/var/output", "/tmp/cache"];
        for root in writable_roots {
            args.push("--add-dir".to_string());
            args.push(root.to_string());
        }

        // Count occurrences of --add-dir
        let add_dir_count = args.iter().filter(|&a| a == "--add-dir").count();
        assert_eq!(add_dir_count, 3); // 1 workspace + 2 writable_roots
    }
}
