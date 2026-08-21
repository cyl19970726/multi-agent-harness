use super::*;

    #[test]
    fn persistent_claude_omits_effort_arg_when_absent() {
        let spec = launch_spec_with_model_effort(Some("opus"), None);
        let mut cmd = Command::new("claude");
        apply_claude_model_and_effort_args(&mut cmd, &spec);

        let args = command_args(&cmd);
        assert_eq!(args, vec!["--model", "opus"]);
        assert!(!args.iter().any(|arg| arg == "--effort"));
    }

