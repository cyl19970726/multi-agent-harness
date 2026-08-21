use super::*;

#[test]
fn close_admission_is_serialized_with_successor_supervisor_generation() {
    let (store, root) = temp_store("successor-close-admission-fence");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let first = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-before-close",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire first Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &first);
    let close = pending_close_request(
        &created.team_run.id,
        &member.id,
        "host",
        "close before successor provider spawn",
    );

    let current_error = store
        .latch_team_member_close_without_current_supervisor(&close, current_unix_ms_u64())
        .expect_err("current generation must fence stale-runtime Close");
    assert!(
        current_error
            .to_string()
            .contains("TEAM_SUPERVISOR_LEASE_CURRENT"),
        "unexpected current-generation error: {current_error}"
    );
    assert!(
        store
            .team_member_close_requests()
            .expect("close requests")
            .is_empty(),
        "fenced Close was persisted"
    );

    store
        .release_team_supervisor_lease(
            &created.team_run.id,
            &first.supervisor_id,
            first.generation,
            current_unix_ms_u64(),
        )
        .expect("release first generation");
    store
        .latch_team_member_close_without_current_supervisor(&close, current_unix_ms_u64())
        .expect("latch Close while no generation owns the run");
    let successor = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-after-close",
            std::process::id(),
            "tcp://127.0.0.1:2",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire successor generation");
    assert!(successor.generation > first.generation);

    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &successor.supervisor_id,
        successor.generation,
        Arc::new(AtomicBool::new(true)),
    );
    assert!(matches!(
        prepare_member_workspace_for_spawn(
            &ledger,
            &member,
            &test_provider_environment_observation(&root),
        )
        .expect("successor reconciles pending Close"),
        PreSpawnWorkspacePreparation::Superseded
    ));
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest members")
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .expect("member");
    assert!(latest.coordination_is_closed());
    assert_eq!(latest.status, MemberRunStatus::Stopped);
    assert_eq!(
        store
            .latest_team_member_close_request(&member.id)
            .expect("close request")
            .expect("close row")
            .status,
        TeamMemberCloseStatus::Applied
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
