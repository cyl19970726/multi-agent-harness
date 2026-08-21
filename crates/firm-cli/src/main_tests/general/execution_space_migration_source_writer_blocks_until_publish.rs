use super::*;

#[test]
fn execution_space_migration_source_writer_blocks_until_publish() {
    let (root, firm_home, project_context) = migration_test_project("source-writer");
    let source_store = HarnessStore::new(project_context.store_root.clone());
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer_slot = Arc::new(Mutex::new(None));
    let hook_writer_slot = Arc::clone(&writer_slot);

    execution_space_migrate_from_project_with_hooks(
        &firm_home,
        &migration_args(&project_context.id, "writer-space", false),
        move || {
            let writer = std::thread::spawn(move || {
                started_tx.send(()).expect("writer started");
                let result = source_store.append_mission(&Mission {
                    id: "mission-concurrent-writer".into(),
                    title: "Concurrent writer".into(),
                    objective: "Wait for migration publication".into(),
                    context: String::new(),
                    desired_outcome: None,
                    status: MissionStatus::Planned,
                    legacy_wave_ids: Vec::new(),
                    outcome_summary: None,
                    completed_by: None,
                    created_at: "unix-ms:2".into(),
                    updated_at: "unix-ms:2".into(),
                    completed_at: None,
                });
                let _ = done_tx.send(result);
            });
            *hook_writer_slot.lock().expect("writer slot") = Some(writer);
            started_rx.recv().expect("writer started signal");
            assert!(
                done_rx
                    .recv_timeout(std::time::Duration::from_millis(100))
                    .is_err(),
                "ordinary Store writer must block while migration guard is held"
            );
            Ok(())
        },
        |home, lock, id, name, binding, now| {
            execution_space::register_and_activate_locked(
                home,
                lock,
                id,
                name,
                Some(binding.to_string()),
                None,
                now,
            )
        },
    )
    .expect("migration");

    writer_slot
        .lock()
        .expect("writer slot")
        .take()
        .expect("writer handle")
        .join()
        .expect("writer thread");
    assert_eq!(
        HarnessStore::new(project_context.store_root.clone())
            .missions()
            .unwrap()
            .len(),
        1
    );
    assert!(
        !execution_space::space_store_root(&firm_home, "writer-space")
            .join("missions.jsonl")
            .exists(),
        "writer append happens only after snapshot publication"
    );
    fs::remove_dir_all(root).expect("cleanup");
}
