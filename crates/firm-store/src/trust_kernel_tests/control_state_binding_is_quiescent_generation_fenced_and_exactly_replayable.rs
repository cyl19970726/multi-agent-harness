use super::*;

#[test]
fn control_state_binding_is_quiescent_generation_fenced_and_exactly_replayable() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "control-bind-agent", 0),
            identity("control-bind-agent"),
        )
        .unwrap();
    let mut target = session("session-control-bind", "control-bind-agent");
    target.control_state.runtime_residency = RuntimeResidency::Detached;
    target.control_state.activity = RuntimeActivity::Idle;
    store
        .create_agent_session(
            &service_context("session.create", "session-control-bind", 0),
            target.clone(),
        )
        .unwrap();
    let mut next = target.control_state.clone();
    next.driver_generation = 2;
    next.composition_fingerprint = Some("composition:v2".into());
    let mutation = service_context("session.control.bind", "control-bind", 1);
    let first = store
        .bind_agent_session_control_state(
            &mutation,
            &target.id,
            target.runtime_generation,
            next.clone(),
            "t2",
        )
        .unwrap();
    assert_eq!(first.projection.control_state.driver_generation, 2);
    let before_replay = store.canonical_operations().unwrap();
    let replay = store
        .bind_agent_session_control_state(
            &mutation,
            &target.id,
            target.runtime_generation,
            next,
            "t2",
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(store.canonical_operations().unwrap(), before_replay);

    let error = store
        .bind_agent_session_control_state(
            &service_context("session.control.bind", "control-bind-stale", 2),
            &target.id,
            target.runtime_generation.saturating_add(1),
            first.projection.control_state,
            "t3",
        )
        .expect_err("stale runtime generation must not mutate control state");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_replay);
    fs::remove_dir_all(root).unwrap();
}
