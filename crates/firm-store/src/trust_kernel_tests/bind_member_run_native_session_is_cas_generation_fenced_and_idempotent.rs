use super::*;

#[test]
fn bind_member_run_native_session_is_cas_generation_fenced_and_idempotent() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-bind-native", "team-run-bind-native");
    let run = MemberRun {
        id: "member-run-bind-native".into(),
        agent_member_id: "fixture-host".into(),
        team_run_id: "team-run-bind-native".into(),
        role_snapshot: "implementer".into(),
        provider_profile_snapshot: Some("codex/codex_app_server".into()),
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: MemberCoordinationStatus::Active,
        runtime_status: MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: None,
        native_session: None,
        version: 1,
        started_at: "t1".into(),
        last_event_at: None,
        finished_at: None,
    };
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-bind-native", 0),
            run,
        )
        .unwrap();
    let native = settled_native_session("thread-settled-2");

    let stale_generation = store
        .bind_member_run_native_session(
            &context(
                "host",
                "member_run.native.bind",
                "bind-run-stale-generation",
                1,
            ),
            "member-run-bind-native",
            2,
            native.clone(),
            "t2",
        )
        .expect_err("a settled binding from another runtime generation is fenced");
    assert!(
        stale_generation
            .to_string()
            .contains("MEMBER_RUN_GENERATION_FENCED"),
        "{stale_generation}"
    );
    let stale_version = store
        .bind_member_run_native_session(
            &context(
                "host",
                "member_run.native.bind",
                "bind-run-stale-version",
                0,
            ),
            "member-run-bind-native",
            1,
            native.clone(),
            "t2",
        )
        .expect_err("the bind CAS rejects a stale expected version");
    assert!(
        stale_version.to_string().contains("VERSION_CONFLICT"),
        "{stale_version}"
    );

    let bound = store
        .bind_member_run_native_session(
            &context("host", "member_run.native.bind", "bind-run-native", 1),
            "member-run-bind-native",
            1,
            native.clone(),
            "t2",
        )
        .expect("first settle binds the native Session");
    assert!(!bound.replayed);
    assert_eq!(bound.projection.version, 2);
    assert_eq!(bound.projection.native_session.as_ref(), Some(&native));
    assert_eq!(
        bound.projection.coordination_status,
        MemberCoordinationStatus::Active
    );
    assert_eq!(bound.projection.runtime_status, MemberRuntimeStatus::Idle);
    assert_eq!(bound.projection.runtime_generation, 1);
    assert_eq!(bound.projection.last_event_at.as_deref(), Some("t2"));

    let replay = store
        .bind_member_run_native_session(
            &context("host", "member_run.native.bind", "bind-run-native", 1),
            "member-run-bind-native",
            1,
            native.clone(),
            "t2",
        )
        .expect("the exact same bind replays");
    assert!(replay.replayed);
    assert_eq!(replay.projection.version, 2);
    assert_eq!(replay.event.id, bound.event.id);

    let conflicting = store
        .bind_member_run_native_session(
            &context("host", "member_run.native.bind", "bind-run-conflict", 2),
            "member-run-bind-native",
            1,
            settled_native_session("thread-other"),
            "t3",
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
