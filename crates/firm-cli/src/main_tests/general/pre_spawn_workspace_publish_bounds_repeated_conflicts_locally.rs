use super::*;

#[test]
fn pre_spawn_workspace_publish_bounds_repeated_conflicts_locally() {
    let (store, root) = temp_store("pre-spawn-bounded-conflicts");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-pre-spawn-conflicts",
            std::process::id(),
            "test://pre-spawn-conflicts",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let member = created.member_runs[0].clone();
    let snapshot = test_provider_environment_observation(&root);
    let mut conflicts = 0usize;
    let result =
        prepare_member_workspace_for_spawn_with_hook(&ledger, &member, &snapshot, |_, observed| {
            conflicts += 1;
            let mut drift = observed.clone();
            drift.name = format!("conflict-{conflicts}");
            store_conflict_as_usage(store.compare_and_append_member_run(observed, &drift))?;
            Ok(())
        })
        .expect("exhausted CAS contention is local");
    assert!(matches!(result, PreSpawnWorkspacePreparation::Retry));
    assert_eq!(conflicts, PROVIDER_MEMBER_CAS_RETRIES);
    let latest = ledger
        .latest_member_run(&member.id)
        .expect("latest member")
        .expect("member exists");
    assert_eq!(latest.name, "conflict-3");
    assert!(latest.provider_environment_observation.is_none());
    std::fs::remove_dir_all(root).expect("cleanup");
}
