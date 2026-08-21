use super::*;

    #[test]
    fn native_session_open_target_rejects_unsupported_or_unsafe_sessions() {
        let codex = native_session_open_target(&native_open_test_member(
            "codex",
            "codex_app_server",
            "thread-1",
        ))
        .expect_err("Codex has no registered native UI target");
        assert!(codex.to_string().contains("supports only Claude Agent SDK"));

        let unsafe_id = native_session_open_target(&native_open_test_member(
            "claude",
            "claude_agent_sdk",
            "session&unexpected=true",
        ))
        .expect_err("unsafe deep-link value must fail closed");
        assert!(unsafe_id.to_string().contains("unsafe native session id"));
    }

