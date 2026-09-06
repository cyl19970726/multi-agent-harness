use super::*;

fn prepared() -> (
    HarnessStore,
    std::path::PathBuf,
    ControlCommandEnvelope,
    MutationContext,
) {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "phase-identity", 0),
            identity("phase-agent"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "phase-session", 0),
            session("phase-session", "phase-agent"),
        )
        .unwrap();
    let session = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .pop()
        .unwrap();
    let (command, context) = runtime_command_fixture(
        "phase-command",
        RuntimeCommandKind::StartSession,
        &session,
        "phase-start",
    );
    store
        .prepare_runtime_command(&context, &command, current_unix_ms(), "prepared")
        .unwrap();
    (store, root, command, context)
}

fn rewrite_fixture(root: &std::path::Path, mut change: impl FnMut(&mut serde_json::Value)) {
    let path = root.join("agentfirm_trust_operations.jsonl");
    let content = fs::read_to_string(&path).unwrap();
    let rows = content
        .lines()
        .map(|line| {
            let mut row = serde_json::from_str(line).unwrap();
            change(&mut row);
            serde_json::to_string(&row).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{rows}\n")).unwrap();
}

#[test]
fn legacy_unknown_phase_blocks_new_effects_and_does_not_grant_settlement_or_recovery() {
    for conflicting in [false, true] {
        let (store, root, mut command, mut admission) = prepared();
        rewrite_fixture(&root, |row| {
            if row["operation"]["event"]["aggregate_id"] == "phase-command" {
                let projection = &mut row["operation"]["resulting_projection"];
                projection["status"] = "accepted".into();
                if conflicting {
                    projection["phase"] = "settled".into();
                } else {
                    projection.as_object_mut().unwrap().remove("phase");
                }
            }
        });
        let before = fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap();
        let record = store.runtime_commands("space-test").unwrap().pop().unwrap();
        assert_eq!(record.phase, RuntimeCommandPhase::Unknown);
        assert!(!store
            .runtime_command_is_publicly_recoverable(&record, current_unix_ms())
            .unwrap());
        assert!(store
            .settle_runtime_command(
                &service_context("node_daemon.runtime.settle", "unknown-settle", 1),
                &record.id,
                RuntimeCommandPhase::Settled,
                RuntimeEffectCertainty::Applied,
                Some(serde_json::json!({"native": "receipt"})),
                None,
                "no-grant"
            )
            .is_err());
        command.id = "phase-successor".into();
        command.idempotency_key = "phase-successor".into();
        admission.idempotency_key = command.idempotency_key.clone();
        admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&command).unwrap());
        let error = store
            .prepare_runtime_command(&admission, &command, current_unix_ms(), "no-drive")
            .unwrap_err();
        assert!(
            error.to_string().contains("RUNTIME_EFFECT_UNKNOWN"),
            "{error}"
        );
        assert_eq!(
            fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
            before
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn legacy_settlement_replay_preserves_original_event_and_rejects_payload_drift() {
    for (phase, status) in [
        (RuntimeCommandPhase::Observed, "quiesced"),
        (RuntimeCommandPhase::RecoveryRequired, "recovery_required"),
    ] {
        let (store, root, command, _) = prepared();
        let context = service_context("node_daemon.runtime.settle", "phase-settle", 1);
        let result = Some(serde_json::json!({"receipt": "unconfirmed"}));
        store
            .settle_runtime_command(
                &context,
                &command.id,
                phase,
                RuntimeEffectCertainty::Unknown,
                result.clone(),
                None,
                "settled",
            )
            .unwrap();
        rewrite_fixture(&root, |row| {
            if row["operation"]["event"]["aggregate_id"] == command.id
                && row["operation"]["event"]["transition"] == "settled"
            {
                let event = &mut row["operation"]["event"];
                event["payload"].as_object_mut().unwrap().remove("phase");
                event["payload"]["status"] = status.into();
                event["canonical_request_fingerprint"] =
                    canonical_json_fingerprint(&event["payload"]).into();
                row["operation"]["resulting_projection"]["status"] = status.into();
            }
        });
        let before = fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap();
        let replay = store
            .settle_runtime_command(
                &context,
                &command.id,
                phase,
                RuntimeEffectCertainty::Unknown,
                result.clone(),
                None,
                "replay",
            )
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.event.payload["status"], status);
        assert!(replay.event.payload.get("phase").is_none());
        let error = store
            .settle_runtime_command(
                &context,
                &command.id,
                phase,
                RuntimeEffectCertainty::Unknown,
                Some(serde_json::json!({"receipt": "different"})),
                None,
                "drift",
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("IDEMPOTENCY_KEY_REUSED"),
            "{error}"
        );
        let mut other_actor = context.clone();
        other_actor.authenticated_actor.id = "another-daemon".into();
        assert!(store
            .settle_runtime_command(
                &other_actor,
                &command.id,
                phase,
                RuntimeEffectCertainty::Unknown,
                result,
                None,
                "unauthorized"
            )
            .is_err());
        assert_eq!(
            fs::read(root.join("agentfirm_trust_operations.jsonl")).unwrap(),
            before
        );
        fs::remove_dir_all(root).unwrap();
    }
}
