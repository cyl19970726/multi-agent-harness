use super::*;

/// The one site of the four whose decision actually flips from accept to
/// refuse at the ceiling.
///
/// The guard asks "did the caller advance the driver generation exactly once?".
/// Written with `saturating_add(1)` it computed `u64::MAX` as the expected
/// successor of `u64::MAX`, so a caller that advanced *nothing* satisfied it
/// and a driver/composition transfer was admitted while the fence that names
/// the current driver stood still. `checked_add` makes the ceiling unsatisfiable
/// instead, which is the correct reading of "exactly once".
#[test]
fn driver_transfer_at_the_generation_ceiling_refuses_a_transfer_that_advanced_nothing() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "ceiling-transfer-agent", 0),
        "ceiling-transfer-agent",
    );
    let mut target = session("session-transfer-ceiling", "ceiling-transfer-agent");
    // Quiescent, so the transfer reaches the generation guard rather than
    // stopping at the Detached/Idle lane check.
    target.control_state.runtime_residency = RuntimeResidency::Detached;
    target.control_state.activity = RuntimeActivity::Idle;
    target.control_state.driver_generation = u64::MAX;
    store
        .create_agent_session(
            &service_context("session.create", "session-transfer-ceiling", 0),
            target.clone(),
        )
        .unwrap();

    // A real transfer: the composition fingerprint changes, so the guard runs.
    // The driver generation stays at the ceiling — this is the caller the fence
    // has to refuse, and the one the saturating arithmetic used to admit.
    let mut next = target.control_state.clone();
    next.composition_fingerprint = Some("composition:v2".into());
    next.driver_generation = u64::MAX;

    let before = store.canonical_operations().unwrap();
    let refusal = store
        .bind_agent_session_control_state(
            &service_context("session.control.bind", "transfer-ceiling", 1),
            &target.id,
            target.runtime_generation,
            next,
            "t2",
        )
        .expect_err("a transfer that advanced nothing must be refused at the ceiling");
    assert!(
        refusal.to_string().contains("MEMBER_RUN_GENERATION_FENCED"),
        "unexpected refusal: {refusal}"
    );
    assert!(
        refusal
            .to_string()
            .contains("must advance the driver generation exactly once"),
        "unexpected refusal: {refusal}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        before,
        "a refused transfer must not commit a projection"
    );

    // The lane is still exactly what it was: same driver generation, same
    // composition. The refusal did not half-apply the transfer.
    let unchanged = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == target.id)
        .expect("session still readable");
    assert_eq!(unchanged.control_state.driver_generation, u64::MAX);
    assert_eq!(
        unchanged.control_state.composition_fingerprint,
        target.control_state.composition_fingerprint
    );
    fs::remove_dir_all(root).unwrap();
}
