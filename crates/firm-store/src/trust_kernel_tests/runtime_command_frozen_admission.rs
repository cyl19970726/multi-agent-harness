use super::*;

const FROZEN_WIRE_NAMES: &[&str] = &[
    "reopen_member",
    "retire_member",
    "delete_native_session",
    "cancel_pending_input",
    "activate_continuation",
    "replace_continuation_condition",
    "clear_continuation",
    "stop_background_task",
    "transfer_execution_driver",
    "inspect_command_effect",
    "reconcile_unknown_effect",
    "abort_if_not_applied",
];

#[test]
fn frozen_dynamic_commands_are_readable_but_new_prepare_has_zero_delta() {
    for name in FROZEN_WIRE_NAMES {
        let (store, root) = fabric_store();
        let kind = serde_json::from_value(serde_json::json!(name)).unwrap();
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", "frozen-member", 0),
                identity("member"),
            )
            .unwrap();
        store
            .create_agent_session(
                &service_context("session.create", "frozen-session", 0),
                session("session", "member"),
            )
            .unwrap();
        let current_session = store
            .fabric_agent_sessions("space-test")
            .unwrap()
            .pop()
            .unwrap();
        let (command, admission) =
            runtime_command_fixture("frozen-command", kind, &current_session, "frozen");
        // Exercise the same unrestricted envelope decoding as a remote caller.
        let command: ControlCommandEnvelope =
            serde_json::from_value(serde_json::to_value(command).unwrap()).unwrap();
        let before = fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap();
        let error = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "must-not-write")
            .unwrap_err();
        assert!(
            error.to_string().contains("RUNTIME_COMMAND_KIND_FROZEN"),
            "{name}: {error}"
        );
        assert_eq!(
            fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
            before
        );
        assert!(store.runtime_commands("space-test").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn control_lifecycle_and_observation_kinds_keep_existing_admission_rules() {
    for name in [
        "author_message",
        "start_session",
        "stop_session",
        "resume_session",
        "dispatch_provider",
        "cancel_provider_turn",
        "open_runtime",
        "resume_native_session",
        "release_runtime",
        "close_member",
        "start_cycle",
        "inject_current_cycle",
        "queue_at_native_boundary",
        "interrupt_current_cycle",
        "inhibit_continuation",
        "resume_continuation",
        "quiesce_execution_lane",
        "drain_runtime",
        "reattach_live_runtime",
        "inspect_continuation",
    ] {
        let (store, root) = fabric_store();
        let kind = serde_json::from_value(serde_json::json!(name)).unwrap();
        let (command, admission) = runtime_command_fixture(
            "retained",
            kind,
            &session("missing-session", "missing-member"),
            "retained",
        );
        let result = store.prepare_runtime_command(
            &admission,
            &command,
            current_unix_ms(),
            "existing-rules",
        );
        if name == "author_message" {
            assert!(!result.unwrap().replayed);
        } else {
            let error = result.expect_err("missing exact session must still fail closed");
            assert!(
                !error.to_string().contains("RUNTIME_COMMAND_KIND_FROZEN"),
                "{name}: {error}"
            );
            assert!(store.runtime_commands("space-test").unwrap().is_empty());
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn frozen_historical_admission_replays_exactly_without_writing_or_allowing_new_keys() {
    for name in FROZEN_WIRE_NAMES {
        let (store, root) = fabric_store();
        let (mut command, mut admission) = runtime_command_fixture(
            "historical-command",
            RuntimeCommandKind::AuthorMessage,
            &session("session", "member"),
            "historical",
        );
        store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "old")
            .unwrap();
        command.command = serde_json::from_value(serde_json::json!(name)).unwrap();
        command.required_capability = runtime_command_capability(command.command).into();
        let fingerprint = runtime_command_envelope_fingerprint(&command).unwrap();
        admission.request_fingerprint = Some(fingerprint.clone());
        // Construct a pre-cutover journal fixture, never mutate a real store.
        let path = root.join("agentfirm_trust_operations.jsonl");
        let rows = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| {
                let mut row: serde_json::Value = serde_json::from_str(line).unwrap();
                if row["operation"]["event"]["aggregate_id"] == command.id {
                    row["operation"]["event"]["canonical_request_fingerprint"] =
                        fingerprint.clone().into();
                    let projection = &mut row["operation"]["resulting_projection"];
                    projection["command"] = serde_json::json!(name);
                    projection["required_capability"] = command.required_capability.clone().into();
                    projection["request_fingerprint"] = fingerprint.clone().into();
                }
                serde_json::to_string(&row).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, format!("{rows}\n")).unwrap();
        let before = fs::read(&path).unwrap();
        let historical = store.runtime_commands("space-test").unwrap().pop().unwrap();
        assert_eq!(historical.command, command.command);
        let replay = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "replay")
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.projection, historical);
        assert_eq!(fs::read(&path).unwrap(), before);
        command.payload["changed"] = true.into();
        command.payload_fingerprint = canonical_json_fingerprint(&command.payload);
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let drift = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "drift")
            .unwrap_err();
        assert!(
            drift.to_string().contains("IDEMPOTENCY_KEY_REUSED"),
            "{drift}"
        );
        command.id = "new-command".into();
        command.idempotency_key = "new-command".into();
        admission.idempotency_key = command.idempotency_key.clone();
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let fresh = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "new")
            .unwrap_err();
        assert!(
            fresh.to_string().contains("RUNTIME_COMMAND_KIND_FROZEN"),
            "{fresh}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
}
