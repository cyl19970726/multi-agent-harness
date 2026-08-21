use super::*;

#[test]
fn append_uses_unlocked_existing_lock_file() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-stale-lock-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = HarnessStore::new(&root);
    store.init().expect("init store");
    std::fs::write(root.join(".store.lock"), "left by interrupted writer\n")
        .expect("write existing lock file");
    let mission = Mission {
        id: "mission-stale-lock".into(),
        title: "Stale lock".into(),
        objective: "Verify an unlocked existing lock file is reusable".into(),
        context: String::new(),
        desired_outcome: None,
        status: MissionStatus::Running,
        legacy_wave_ids: Vec::new(),
        outcome_summary: None,
        completed_by: None,
        created_at: "2026-05-26T00:00:00Z".into(),
        updated_at: "2026-05-26T00:00:00Z".into(),
        completed_at: None,
    };

    store
        .append_mission(&mission)
        .expect("append with unlocked lock file");
    assert_eq!(store.missions().expect("read missions"), vec![mission]);

    std::fs::remove_dir_all(root).expect("remove temp store");
}
