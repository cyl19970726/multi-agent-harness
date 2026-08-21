use super::*;

    #[test]
    fn persistent_claude_effort_arg_matches_ephemeral_mapping() {
        let spec = launch_spec_with_model_effort(Some("opus"), Some("medium"));
        let mut cmd = Command::new("claude");
        apply_claude_model_and_effort_args(&mut cmd, &spec);

        assert_eq!(
            command_args(&cmd),
            vec!["--model", "opus", "--effort", "medium"]
        );
    }

