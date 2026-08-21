use super::*;

#[test]
fn exclusive_migration_guard_blocks_normal_store_writers_until_drop() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-migration-guard-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = HarnessStore::new(&root);
    store.init().expect("init store");
    let guard = store
        .acquire_exclusive_migration_guard()
        .expect("migration guard");
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let writer_store = store.clone();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).expect("signal writer start");
        let result = writer_store.append_mission(&Mission {
            id: "mission-after-migration".into(),
            title: "Blocked writer".into(),
            objective: "Prove the migration guard shares the writer lock".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Planned,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        });
        done_tx.send(result).expect("signal writer completion");
    });

    started_rx.recv().expect("writer started");
    assert!(
        done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "normal writer must remain blocked while migration guard is alive"
    );
    assert!(
        !root.join("missions.jsonl").exists(),
        "blocked writer must not mutate the ledger"
    );

    drop(guard);
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("writer unblocked after guard drop")
        .expect("writer append succeeds");
    writer.join().expect("writer thread");
    assert_eq!(store.missions().expect("missions").len(), 1);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
