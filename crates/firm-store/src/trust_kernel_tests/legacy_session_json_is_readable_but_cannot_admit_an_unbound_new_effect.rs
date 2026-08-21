use super::*;

#[test]
fn legacy_session_json_is_readable_but_cannot_admit_an_unbound_new_effect() {
    let (store, root) = fabric_store();
    let mut legacy_json = serde_json::to_value(session("legacy-session", "legacy-agent")).unwrap();
    let legacy_object = legacy_json.as_object_mut().unwrap();
    legacy_object.remove("control_state");
    let legacy_identity = legacy_object
        .remove("agent_member_id")
        .expect("canonical AgentMember field");
    legacy_object.insert("agent_identity_id".into(), legacy_identity);
    let legacy: AgentSession = serde_json::from_value(legacy_json).unwrap();
    assert_eq!(legacy.control_state.driver_generation, 0);
    assert_eq!(legacy.control_state.driver_ref, RuntimeDriverRef::Unknown);
    let rewritten = serde_json::to_value(&legacy).unwrap();
    assert_eq!(rewritten["agent_member_id"], "legacy-agent");
    assert!(rewritten.get("agent_identity_id").is_none());
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "legacy-agent", 0),
            identity("legacy-agent"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "legacy-session", 0),
            legacy.clone(),
        )
        .unwrap();
    let (mut command, mut admission) = runtime_command_fixture(
        "legacy-unbound-command",
        RuntimeCommandKind::OpenRuntime,
        &legacy,
        "open_runtime",
    );
    command.binding = Default::default();
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let before = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-rejected")
        .expect_err("legacy readability must not become new effect authority");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before);
    assert!(store.runtime_commands("space-test").unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}
