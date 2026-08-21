use super::*;

    #[test]
    fn persistent_claude_omits_schema_arg_when_absent() {
        let spec = launch_spec_with_model_effort(None, None);
        let mut cmd = Command::new("claude");
        apply_claude_output_schema_arg(&mut cmd, &spec);

        assert!(command_args(&cmd).is_empty());
    }

