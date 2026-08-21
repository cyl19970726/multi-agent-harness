use super::*;

#[test]
fn runtime_command_effect_matrix_is_exactly_replayable_and_fingerprint_closed() {
    let cases = [
        ("start", RuntimeCommandKind::StartSession, false),
        ("resume", RuntimeCommandKind::ResumeSession, false),
        ("turn", RuntimeCommandKind::DispatchProvider, true),
        ("input", RuntimeCommandKind::DispatchProvider, true),
        ("interrupt", RuntimeCommandKind::CancelProviderTurn, true),
        ("stop", RuntimeCommandKind::StopSession, false),
    ];
    for (operation, kind, needs_active_turn) in cases {
        let (store, root) = fabric_store();
        let identity_id = format!("runtime-{operation}");
        let session_id = format!("session-{operation}");
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "host",
                    "identity.create",
                    &format!("identity-{operation}"),
                    0,
                ),
                identity(&identity_id),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", &format!("session-create-{operation}"), 0),
                session(&session_id, &identity_id),
            )
            .unwrap();
        if needs_active_turn {
            store
                .transition_agent_session(
                    &service_context(
                        "session.activate",
                        &format!("session-activate-{operation}"),
                        1,
                    ),
                    &session_id,
                    AgentSessionStatus::Active,
                    "t-active",
                )
                .unwrap();
        }
        let current_session = store
            .fabric_agent_sessions("space-test")
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == session_id)
            .unwrap();
        let command_id = format!("runtime-{operation}");
        let (command, admission_context) =
            runtime_command_fixture(&command_id, kind, &current_session, operation);
        let accepted = store
            .prepare_runtime_command(
                &admission_context,
                &command,
                current_unix_ms(),
                "t-accepted",
            )
            .unwrap();
        assert_eq!(accepted.projection.status, RuntimeCommandStatus::Accepted);

        let operations_after_accept = store.canonical_operations().unwrap();
        let accepted_replay = store
            .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t-replay")
            .unwrap();
        assert!(accepted_replay.replayed, "{operation} accepted replay");
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_accept
        );

        let mut drifted = command.clone();
        drifted.payload["operation"] = serde_json::json!(format!("{operation}-drift"));
        drifted.payload_fingerprint = canonical_json_fingerprint(&drifted.payload);
        let mut drifted_context = admission_context.clone();
        drifted_context.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&drifted).unwrap());
        let conflict = store
            .prepare_runtime_command(&drifted_context, &drifted, current_unix_ms(), "t-drift")
            .expect_err("changed full fingerprint must conflict");
        assert!(conflict.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_accept
        );

        let settled = store
            .settle_runtime_command(
                &service_context(
                    "node_daemon.runtime.settle",
                    &format!("{command_id}:settle"),
                    accepted.projection.version,
                ),
                &command_id,
                RuntimeCommandStatus::Applied,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"operation": operation, "applied": true})),
                None,
                "t-applied",
            )
            .unwrap();
        assert_eq!(settled.projection.status, RuntimeCommandStatus::Applied);
        let operations_after_settle = store.canonical_operations().unwrap();
        let terminal_replay = store
            .prepare_runtime_command(
                &admission_context,
                &command,
                current_unix_ms(),
                "t-terminal-replay",
            )
            .unwrap();
        assert!(terminal_replay.replayed, "{operation} terminal replay");
        assert_eq!(
            terminal_replay.projection.status,
            RuntimeCommandStatus::Applied
        );
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_settle
        );
        fs::remove_dir_all(root).unwrap();
    }
}
