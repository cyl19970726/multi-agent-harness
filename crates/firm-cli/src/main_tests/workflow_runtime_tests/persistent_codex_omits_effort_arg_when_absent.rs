use super::*;

    #[test]
    fn persistent_codex_omits_effort_arg_when_absent() {
        let spec = launch_spec_with_model_effort(Some("o4-mini"), None);
        let mut cmd = Command::new("codex");
        apply_codex_model_and_effort_args(&mut cmd, &spec);

        let args = command_args(&cmd);
        assert_eq!(args, vec!["-m", "o4-mini"]);
        assert!(!args.iter().any(|arg| arg == "-c"));
        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));
    }

