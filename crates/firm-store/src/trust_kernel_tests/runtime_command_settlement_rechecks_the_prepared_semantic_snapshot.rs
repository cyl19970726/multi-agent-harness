use super::*;

#[test]
fn runtime_command_settlement_rechecks_the_prepared_semantic_snapshot() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "settle-precondition", 0),
            identity("settle-precondition"),
        )
        .unwrap();
    let target = session("session-settle-precondition", "settle-precondition");
    store
        .create_agent_session(
            &service_context("session.create", "session-settle-precondition", 0),
            target.clone(),
        )
        .unwrap();
    let (mut command, mut admission) = runtime_command_fixture(
        "settle-precondition-command",
        RuntimeCommandKind::OpenRuntime,
        &target,
        "open_runtime",
    );
    command.precondition.expected_session_version = Some(target.version);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let accepted = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "settle-precondition:activate", 1),
            &target.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap();
    let before_settle = store.canonical_operations().unwrap();
    let error = store
        .settle_runtime_command(
            &service_context(
                "node_daemon.runtime.settle",
                "settle-precondition:settle",
                accepted.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "stale"})),
            None,
            "t-settle",
        )
        .expect_err("settlement must not bless a command whose semantic snapshot drifted");
    assert!(error.to_string().contains("expected_session_version"));
    assert_eq!(store.canonical_operations().unwrap(), before_settle);
    assert_eq!(
        store.runtime_commands("space-test").unwrap()[0].status,
        RuntimeCommandStatus::Accepted
    );
    fs::remove_dir_all(root).unwrap();
}
