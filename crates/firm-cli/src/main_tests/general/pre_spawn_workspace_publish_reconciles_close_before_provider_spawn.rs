use super::*;

#[test]
fn pre_spawn_workspace_publish_reconciles_close_before_provider_spawn() {
    let (store, root) = temp_store("pre-spawn-close-race");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-pre-spawn-close",
            std::process::id(),
            "test://pre-spawn-close",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let snapshot = test_provider_environment_observation(&root);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let store_root = store.root().to_path_buf();
    let run_id = created.team_run.id.clone();
    let supervisor_id = lease.supervisor_id.clone();
    let generation = lease.generation;
    let prepared = member.clone();
    let worker = std::thread::spawn(move || {
        let worker_store = HarnessStore::new(store_root);
        let ledger = TeamRunLedger::new(
            &worker_store,
            &run_id,
            &supervisor_id,
            generation,
            Arc::new(AtomicBool::new(true)),
        );
        prepare_member_workspace_for_spawn_with_hook(&ledger, &prepared, &snapshot, |attempt, _| {
            if attempt == 0 {
                worker_barrier.wait();
                worker_barrier.wait();
            }
            Ok(())
        })
    });

    barrier.wait();
    let close = latch_member_close(
        &store,
        &created.team_run.id,
        &member.id,
        "host",
        "close wins before provider spawn",
    )
    .expect("latch Close");
    mark_member_coordination_closed(&store, &created.team_run.id, &member.id)
        .expect("close coordination");
    barrier.wait();

    assert!(matches!(
        worker
            .join()
            .expect("pre-spawn worker")
            .expect("reconcile Close"),
        PreSpawnWorkspacePreparation::Superseded
    ));
    let latest = latest_member_runs_in_append_order(&store)
        .expect("latest members")
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .expect("member");
    assert_eq!(latest.coordination_status, MemberCoordinationStatus::Closed);
    assert_eq!(latest.status, MemberRunStatus::Stopped);
    assert!(latest.finished_at.is_some());
    let applied = store
        .latest_team_member_close_request(&member.id)
        .expect("close request")
        .expect("close row");
    assert_eq!(applied.id, close.id);
    assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
    assert!(applied.applied_at.is_some());
    std::fs::remove_dir_all(root).expect("cleanup");
}
