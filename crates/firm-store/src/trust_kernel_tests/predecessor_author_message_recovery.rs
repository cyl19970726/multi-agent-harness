use super::*;

fn prepared_message() -> (
    HarnessStore,
    PathBuf,
    ControlCommandEnvelope,
    MutationContext,
) {
    let (store, root) = fabric_store();
    let lane = session("unused-session", "unused-member");
    let (mut command, mut admission) = runtime_command_fixture(
        "prepared-message-crash",
        RuntimeCommandKind::AuthorMessage,
        &lane,
        "author-message",
    );
    command.binding = Default::default();
    command.payload = serde_json::json!({"draft": {
        "address_kind": "direct_agent",
        "target_ref": {"kind": "control_plane_actor", "id": "operator"},
        "recipients": [{"kind": "control_plane_actor", "id": "operator"}],
        "kind": "message", "body": "recovery regression", "correlation_id": "recovery-934",
        "response_intent": "informational", "schema_version": 1
    }});
    command.payload_fingerprint = canonical_json_fingerprint(&command.payload);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "prepared")
        .unwrap();
    let mut operator = admission;
    operator.authenticated_actor.id = lane.node_id.clone();
    operator.authority_actor = None;
    operator.command_name = "node_daemon.predecessor_recover".into();
    operator.idempotency_key = "recover-message-crash".into();
    operator.request_fingerprint = None;
    (store, root, command, operator)
}

fn recover_message(
    store: &HarnessStore,
    operator: &MutationContext,
) -> StoreResult<NodeDaemonPredecessorRecovery> {
    store.recover_node_daemon_predecessor(
        operator,
        "11111111-1111-4111-8111-111111111111",
        "daemon-1",
        1,
        "instance-1",
        true,
        true,
        "test:exact-process-and-groups-terminated",
        current_unix_ms() + 61_000,
        "recovered",
    )
}

#[test]
fn predecessor_author_message_without_authored_effect_is_not_applied() {
    let (store, root, command, operator) = prepared_message();
    recover_message(&store, &operator)
        .expect("complete absence proof settles the unexecuted local AuthorMessage");
    let record = store
        .runtime_commands("space-test")
        .unwrap()
        .into_iter()
        .find(|record| record.id == command.id)
        .unwrap();
    assert_eq!(record.effect_certainty, RuntimeEffectCertainty::NotApplied);
    assert!(store
        .canonical_operations()
        .unwrap()
        .iter()
        .all(|operation| operation.event.aggregate_kind != "message"));
    fs::remove_dir_all(root).unwrap();
}

fn authored_message(store: &HarnessStore, command: &ControlCommandEnvelope) -> Message {
    let mut value = command.payload["draft"].clone();
    let object = value.as_object_mut().unwrap();
    let extra = serde_json::json!({
        "id": format!("message:{}", command.idempotency_key),
        "idempotency_key": command.idempotency_key,
        "source_execution_space_id": command.execution_space_id,
        "source_node_id": command.target_node_id,
        "source_node_daemon_id": command.target_node_daemon_id,
        "source_authority_generation": command.target_node_daemon_generation,
        "sender_actor_ref": command.authenticated_actor,
        "body_digest": firm_core::agentfirm_api::message_body_digest("recovery regression"),
        "content_fingerprint": "pending", "created_at": "authored"
    });
    object.extend(extra.as_object().unwrap().clone());
    let mut message: Message = serde_json::from_value(value).unwrap();
    message.content_fingerprint = firm_core::agentfirm_api::message_content_fingerprint(&message);
    let mut author = service_context("message.author", "prepared-message-crash:effect", 0);
    author.authority_actor = Some(command.authenticated_actor.clone());
    store.author_message(&author, message.clone()).unwrap();
    message
}

#[test]
fn predecessor_author_message_applied_is_not_replayed_and_recovery_is_idempotent() {
    let (store, root, command, operator) = prepared_message();
    let message = authored_message(&store, &command);
    let deliveries = store.fabric_message_deliveries("space-test").unwrap();
    recover_message(&store, &operator).unwrap();
    let records = store.runtime_commands("space-test").unwrap();
    assert_eq!(records[0].effect_certainty, RuntimeEffectCertainty::Applied);
    assert_eq!(store.fabric_messages("space-test").unwrap(), vec![message]);
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries
    );
    let history = store.canonical_operations().unwrap();
    assert!(recover_message(&store, &operator).unwrap().already_released);
    assert_eq!(store.canonical_operations().unwrap(), history);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn predecessor_author_message_rejects_incomplete_history_without_settlement() {
    for damage in ["tail", "gap", "payload"] {
        let (store, root, command, operator) = prepared_message();
        if damage == "payload" {
            authored_message(&store, &command);
        }
        let path = root.join("agentfirm_trust_operations.jsonl");
        let original = fs::read_to_string(&path).unwrap();
        let damaged = match damage {
            "tail" => format!("{original}{{\"unfinished\":"),
            "gap" => {
                let mut rows = original
                    .lines()
                    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                    .collect::<Vec<_>>();
                rows[0]["operation"]["event"]["store_sequence"] = serde_json::json!(2);
                rows.iter().map(|row| format!("{row}\n")).collect()
            }
            _ => {
                let mut rows = original
                    .lines()
                    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                    .collect::<Vec<_>>();
                let row = rows
                    .iter_mut()
                    .find(|row| row["operation"]["event"]["aggregate_kind"] == "message")
                    .unwrap();
                row["operation"]["resulting_projection"]["body"] =
                    serde_json::json!("conflicting payload");
                rows.iter().map(|row| format!("{row}\n")).collect()
            }
        };
        fs::write(&path, &damaged).unwrap();
        assert!(
            recover_message(&store, &operator).is_err(),
            "{damage} must fail closed"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            damaged,
            "{damage} must not append settlement"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn predecessor_author_message_rejects_live_or_wrong_instance_without_settlement() {
    let (store, root, _command, operator) = prepared_message();
    let before = store.canonical_operations().unwrap();
    for (instance, now) in [
        ("instance-1", current_unix_ms()),
        ("wrong-instance", current_unix_ms() + 61_000),
    ] {
        let result = store.recover_node_daemon_predecessor(
            &operator,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            instance,
            true,
            true,
            "test:dead",
            now,
            "recovered",
        );
        assert!(result.is_err());
        assert_eq!(store.canonical_operations().unwrap(), before);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn predecessor_author_message_does_not_settle_when_another_command_is_unknown() {
    let (store, root, command, operator) = prepared_message();
    let mut other = store.runtime_commands("space-test").unwrap().remove(0);
    other.id = "unresolved-provider-effect".into();
    other.command = RuntimeCommandKind::StopSession;
    let mutation = service_context("test.seed_unknown_provider", "seed-other", 0);
    store
        .commit_trust_projection_unlocked(
            &mutation,
            "runtime_command",
            &other.id,
            "accepted",
            serde_json::json!({"fixture": "unresolved provider effect"}),
            &other,
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    let before = store.canonical_operations().unwrap();
    let error = recover_message(&store, &operator).unwrap_err();
    assert!(error.to_string().contains("COMMAND_UNSETTLED"));
    assert_eq!(store.canonical_operations().unwrap(), before);
    let message_command = store
        .runtime_commands("space-test")
        .unwrap()
        .into_iter()
        .find(|record| record.id == command.id)
        .unwrap();
    assert_eq!(
        message_command.effect_certainty,
        RuntimeEffectCertainty::Unknown
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn predecessor_author_message_rejects_orphaned_delivery_evidence() {
    let (store, root, command, operator) = prepared_message();
    let mutation = service_context("test.seed_orphan", "seed-orphan", 0);
    store
        .commit_trust_projection_unlocked(
            &mutation,
            "message_delivery",
            "orphan",
            "queued",
            serde_json::json!({}),
            &serde_json::json!({"message_id": format!("message:{}", command.idempotency_key)}),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    let before = store.canonical_operations().unwrap();
    let error = recover_message(&store, &operator).unwrap_err();
    assert!(error.to_string().contains("related canonical records"));
    assert_eq!(store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
