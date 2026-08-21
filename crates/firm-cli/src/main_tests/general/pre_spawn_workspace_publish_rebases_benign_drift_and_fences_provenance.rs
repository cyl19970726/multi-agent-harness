use super::*;

#[test]
fn pre_spawn_workspace_publish_rebases_benign_drift_and_fences_provenance() {
    let (store, root) = temp_store("pre-spawn-rebase-fences");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-pre-spawn-rebase",
            std::process::id(),
            "test://pre-spawn-rebase",
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
    let first = created.member_runs[0].clone();
    let snapshot = test_provider_environment_observation(&root);
    let published = match prepare_member_workspace_for_spawn_with_hook(
        &ledger,
        &first,
        &snapshot,
        |attempt, observed| {
            if attempt == 0 {
                let mut renamed = observed.clone();
                renamed.name = "BenignRename".into();
                store_conflict_as_usage(store.compare_and_append_member_run(observed, &renamed))?;
            }
            Ok(())
        },
    )
    .expect("benign drift rebases")
    {
        PreSpawnWorkspacePreparation::Ready(member) => *member,
        _ => panic!("member remains spawnable"),
    };
    assert_eq!(published.name, "BenignRename");
    assert_eq!(
        published.provider_environment_observation.as_ref(),
        Some(&snapshot)
    );

    let second = created.member_runs[1].clone();
    let mut new_generation = second.clone();
    new_generation.runtime_generation += 1;
    new_generation.native_session = Some(NativeSessionRef {
        provider: second.provider.clone(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "new-generation-session".into(),
        native_locator_kind: "thread_id".into(),
        provider_version: None,
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: None,
        parent_native_session_id: None,
    });
    store
        .compare_and_advance_member_run_generation(&second, &new_generation)
        .expect("advance generation and session");
    assert!(matches!(
        prepare_member_workspace_for_spawn(&ledger, &second, &snapshot)
            .expect("provenance fence is a local skip"),
        PreSpawnWorkspacePreparation::Superseded
    ));
    let latest = ledger
        .latest_member_run(&second.id)
        .expect("latest member")
        .expect("member exists");
    assert_eq!(latest.runtime_generation, new_generation.runtime_generation);
    assert_eq!(latest.native_session, new_generation.native_session);
    assert!(latest.provider_environment_observation.is_none());
    std::fs::remove_dir_all(root).expect("cleanup");
}
