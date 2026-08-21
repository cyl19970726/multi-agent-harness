use super::*;

#[test]
fn runtime_command_failure_certainty_and_torn_rows_recover_without_duplicate_effect() {
    let outcomes = [
        (
            "socket-lost-before-effect",
            RuntimeCommandStatus::Failed,
            RuntimeEffectCertainty::NotApplied,
        ),
        (
            "socket-lost-after-effect",
            RuntimeCommandStatus::RecoveryRequired,
            RuntimeEffectCertainty::Unknown,
        ),
        (
            "provider-terminal-callback-race",
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
        ),
    ];
    for (label, status, certainty) in outcomes {
        let (store, root) = fabric_store();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", &format!("identity-{label}"), 0),
                identity(label),
            )
            .unwrap();
        let session_id = format!("session-{label}");
        store
            .create_agent_session(
                &service_context("session.create", &format!("session-{label}"), 0),
                session(&session_id, label),
            )
            .unwrap();
        let current = store
            .fabric_agent_sessions("space-test")
            .unwrap()
            .pop()
            .unwrap();
        let command_id = format!("runtime-{label}");
        let (command, admission_context) = runtime_command_fixture(
            &command_id,
            RuntimeCommandKind::StartSession,
            &current,
            label,
        );
        let admitted = store
            .prepare_runtime_command(
                &admission_context,
                &command,
                current_unix_ms(),
                "t-prepared",
            )
            .unwrap();
        let ledger = root.join("agentfirm_trust_operations.jsonl");
        let mut torn = fs::OpenOptions::new().append(true).open(&ledger).unwrap();
        torn.write_all(b"{\"torn_prepared\":").unwrap();
        torn.sync_all().unwrap();
        assert_eq!(store.runtime_commands("space-test").unwrap().len(), 1);

        store
            .settle_runtime_command(
                &service_context(
                    "node_daemon.runtime.settle",
                    &format!("{command_id}:settle"),
                    admitted.projection.version,
                ),
                &command_id,
                status,
                certainty,
                (certainty == RuntimeEffectCertainty::Applied)
                    .then(|| serde_json::json!({"effect": "observed"})),
                (certainty != RuntimeEffectCertainty::Applied).then(|| label.to_string()),
                "t-settled",
            )
            .unwrap();
        let mut torn = fs::OpenOptions::new().append(true).open(&ledger).unwrap();
        torn.write_all(b"{\"torn_completed\":").unwrap();
        torn.sync_all().unwrap();
        let recovered = store.runtime_commands("space-test").unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, status);
        assert_eq!(recovered[0].effect_certainty, certainty);
        let operations_before_replay = store.canonical_operations().unwrap();
        let replay = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t-replay")
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.projection.status, status);
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_before_replay
        );
        fs::remove_dir_all(root).unwrap();
    }
}
