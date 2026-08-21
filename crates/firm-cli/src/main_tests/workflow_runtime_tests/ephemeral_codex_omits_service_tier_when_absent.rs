use super::*;

    #[test]
    fn ephemeral_codex_omits_service_tier_when_absent() {
        let mut cmd = Command::new("codex");
        apply_codex_ephemeral_model_effort_service_tier_args(&mut cmd, None, None, None);

        let args = command_args(&cmd);
        assert!(args.is_empty());
        assert!(!args.iter().any(|arg| arg.starts_with("service_tier=")));
    }

