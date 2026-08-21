use super::*;

#[test]
fn concurrent_appends_write_complete_jsonl_rows() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-concurrent-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = Arc::new(HarnessStore::new(&root));
    let worker_count = 8;
    let appends_per_worker = 25;
    let barrier = Arc::new(Barrier::new(worker_count));
    let mut handles = Vec::new();

    for worker in 0..worker_count {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            for index in 0..appends_per_worker {
                let mission = Mission {
                    id: format!("mission-{worker}-{index}"),
                    title: "Concurrent".into(),
                    objective: "Exercise concurrent append integrity".into(),
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
                store.append_mission(&mission).expect("append mission");
            }
        }));
    }

    for handle in handles {
        handle.join().expect("worker thread");
    }

    let missions = store.missions().expect("read missions");
    assert_eq!(missions.len(), worker_count * appends_per_worker);
    let ids = missions
        .iter()
        .map(|mission| mission.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), worker_count * appends_per_worker);

    std::fs::remove_dir_all(root).expect("remove temp store");
}
