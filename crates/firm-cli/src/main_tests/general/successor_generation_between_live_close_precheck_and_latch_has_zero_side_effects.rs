use super::*;

#[test]
fn successor_generation_between_live_close_precheck_and_latch_has_zero_side_effects() {
    let (store, root) = temp_store("live-close-generation-toctou");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let first = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-close-precheck",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire first Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &first);
    let events_before = store
        .legacy_team_run_events()
        .expect("events before stale Close");
    let token = "d".repeat(64);
    let capability = test_collaboration_capability(&store, &first, &member, &token);
    let (control_rx, _control_registration) = register_live_member_control(&member, &capability, 1);
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    let successor_generation = std::cell::Cell::new(0);

    let error = dispatch_local_live_member_control_with_close_admission_hook(
        &store,
        &first.supervisor_id,
        first.generation,
        &supervisor_valid,
        &authority_gate,
        LiveMemberControlRequest::Close {
            team_run_id: created.team_run.id.clone(),
            member_run_id: member.id.clone(),
            reason: "must be rejected after takeover".into(),
            requested_by: "host".into(),
        },
        || {
            // Deterministic interleaving: the optimistic precheck and all
            // capability checks have passed, then a successor takes the
            // Store lease before the atomic Close admission point.
            store
                .release_team_supervisor_lease(
                    &created.team_run.id,
                    &first.supervisor_id,
                    first.generation,
                    current_unix_ms_u64(),
                )
                .expect("release prechecked generation");
            let successor = store
                .acquire_test_supervisor_lease(
                    &created.team_run.id,
                    "supervisor-close-successor",
                    std::process::id(),
                    "tcp://127.0.0.1:2",
                    current_unix_ms_u64(),
                    60_000,
                )
                .expect("acquire successor generation");
            successor_generation.set(successor.generation);
        },
    )
    .expect_err("stale generation must lose atomic Close admission");

    assert!(
        error.is_supervisor_lease_lost(),
        "unexpected stale-generation error: {error}"
    );
    assert!(successor_generation.get() > first.generation);
    assert!(
        store
            .team_member_close_requests()
            .expect("close requests")
            .is_empty(),
        "stale generation persisted Close"
    );
    assert_eq!(
        store
            .legacy_team_run_events()
            .expect("events after stale Close"),
        events_before,
        "stale generation emitted a lifecycle event"
    );
    assert!(
        matches!(
            control_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "stale generation sent a provider control command"
    );
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest members")
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .expect("member");
    assert!(latest.coordination_is_active());
    std::fs::remove_dir_all(root).expect("cleanup");
}
