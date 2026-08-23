use super::*;

#[test]
fn native_session_open_target_supports_codex_and_rejects_unsafe_sessions() {
    let codex = native_session_open_target(&native_open_test_member(
        "codex",
        "codex_app_server",
        "thread-1",
    ))
    .expect("Codex app-server native thread has a registered Desktop target");
    assert_eq!(codex["uri"], "codex://threads/thread-1");
    assert_eq!(codex["desktop_session_id"], "thread-1");

    let unsupported =
        native_session_open_target(&native_open_test_member("kimi", "kimi_acp", "session-1"))
            .expect_err("Kimi has no registered native Desktop target");
    assert!(unsupported
        .to_string()
        .contains("has no reviewed Desktop open target"));

    let unsafe_id = native_session_open_target(&native_open_test_member(
        "claude",
        "claude_agent_sdk",
        "session&unexpected=true",
    ))
    .expect_err("unsafe deep-link value must fail closed");
    assert!(unsafe_id.to_string().contains("unsafe native session id"));
}
