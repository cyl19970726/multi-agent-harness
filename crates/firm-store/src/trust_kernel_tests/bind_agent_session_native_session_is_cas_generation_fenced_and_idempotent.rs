use super::*;

#[test]
fn bind_agent_session_native_session_is_cas_generation_fenced_and_idempotent() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-bind-native", 0),
            identity("bind-native"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "session-bind-native", 0),
            session("session-bind-native", "bind-native"),
        )
        .unwrap();
    let native = settled_native_session("thread-settled-1");

    let stale_generation = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-stale-generation", 1),
            "session-bind-native",
            2,
            native.clone(),
        )
        .expect_err("a settled binding from another runtime generation is fenced");
    assert!(
        stale_generation
            .to_string()
            .contains("MEMBER_RUN_GENERATION_FENCED"),
        "{stale_generation}"
    );
    let stale_version = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-stale-version", 0),
            "session-bind-native",
            1,
            native.clone(),
        )
        .expect_err("the bind CAS rejects a stale expected version");
    assert!(
        stale_version.to_string().contains("VERSION_CONFLICT"),
        "{stale_version}"
    );

    let bound = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-session", 1),
            "session-bind-native",
            1,
            native.clone(),
        )
        .expect("first settle binds the native Session");
    assert!(!bound.replayed);
    assert_eq!(bound.projection.version, 2);
    assert_eq!(bound.projection.native_session_ref.as_ref(), Some(&native));
    assert_eq!(bound.projection.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(bound.projection.runtime_generation, 1);

    let replay = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-session", 1),
            "session-bind-native",
            1,
            native.clone(),
        )
        .expect("the exact same bind replays");
    assert!(replay.replayed);
    assert_eq!(replay.projection.version, 2);
    assert_eq!(replay.event.id, bound.event.id);

    let rewritten = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-session-again", 2),
            "session-bind-native",
            1,
            native.clone(),
        )
        .expect("rebinding the same native id is idempotent in effect");
    assert_eq!(rewritten.projection.version, 3);
    assert_eq!(
        rewritten
            .projection
            .native_session_ref
            .as_ref()
            .map(|value| value.native_session_id.as_str()),
        Some("thread-settled-1")
    );

    let conflicting = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-conflict", 3),
            "session-bind-native",
            1,
            settled_native_session("thread-other"),
        )
        .expect_err("a different native id cannot overwrite the binding");
    assert!(
        conflicting
            .to_string()
            .contains("already binds another provider-native Session"),
        "{conflicting}"
    );
    fs::remove_dir_all(root).unwrap();
}
