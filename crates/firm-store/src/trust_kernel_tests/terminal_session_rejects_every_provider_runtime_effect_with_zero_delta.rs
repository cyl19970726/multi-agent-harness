use super::*;

#[test]
fn terminal_session_rejects_every_provider_runtime_effect_with_zero_delta() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-terminal", 0),
            identity("terminal"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "session-terminal", 0),
            session("session-terminal", "terminal"),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.close", "session-terminal-close", 1),
            "session-terminal",
            AgentSessionStatus::Closed,
            "t-closed",
        )
        .unwrap();
    let closed = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .pop()
        .unwrap();
    let operations_before = store.canonical_operations().unwrap();
    for (operation, kind) in [
        ("start", RuntimeCommandKind::StartSession),
        ("resume", RuntimeCommandKind::ResumeSession),
        ("turn", RuntimeCommandKind::DispatchProvider),
        ("input", RuntimeCommandKind::DispatchProvider),
        ("interrupt", RuntimeCommandKind::CancelProviderTurn),
        ("stop", RuntimeCommandKind::StopSession),
    ] {
        let (command, context) =
            runtime_command_fixture(&format!("terminal-{operation}"), kind, &closed, operation);
        store
            .prepare_runtime_command(&context, &command, current_unix_ms(), "t-rejected")
            .expect_err("terminal AgentSession must reject runtime effects");
        assert_eq!(store.canonical_operations().unwrap(), operations_before);
        assert!(store.runtime_commands("space-test").unwrap().is_empty());
    }
    fs::remove_dir_all(root).unwrap();
}
