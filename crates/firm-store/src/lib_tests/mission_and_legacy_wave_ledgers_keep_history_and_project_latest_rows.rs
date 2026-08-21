use super::*;

#[test]
fn mission_and_legacy_wave_ledgers_keep_history_and_project_latest_rows() {
    let root = std::env::temp_dir().join(format!(
        "firm-store-mission-wave-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_millis()
    ));
    let store = HarnessStore::new(&root);
    let mission = Mission {
        id: "mission-1".into(),
        title: "Import a Mission with Legacy Wave history".into(),
        objective: "Add the migration foundation".into(),
        context: String::new(),
        desired_outcome: Some("A compatible, durable contract".into()),
        status: MissionStatus::Planned,
        legacy_wave_ids: vec!["wave-1".into()],
        outcome_summary: None,
        completed_by: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    let mut updated_mission = mission.clone();
    updated_mission.status = MissionStatus::Running;
    updated_mission.updated_at = "unix-ms:2".into();
    store.append_mission(&mission).expect("append mission");
    store
        .append_mission(&updated_mission)
        .expect("append updated mission");

    let wave = LegacyWave {
        id: "wave-1".into(),
        mission_id: "mission-1".into(),
        index: 1,
        title: "Contract".into(),
        objective: "Define the additive contract".into(),
        context: String::new(),
        revision: 1,
        updated_by: Some("host".into()),
        exit_criteria: Some("Schema and store rows validate".into()),
        status: LegacyWaveStatus::Running,
        executor_kind: LegacyWaveExecutorKind::AgentTeam,
        executor_run_ids: vec!["team-run-1".into()],
        accepted_run_id: None,
        plan_note: None,
        outcome_summary: None,
        artifact_refs: vec!["schemas/mission.schema.json".into()],
        gate_status: LegacyWaveGateStatus::Pending,
        gate_note: None,
        accepted_by: None,
        accepted_at: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    let mut accepted_wave = wave.clone();
    accepted_wave.status = LegacyWaveStatus::Completed;
    accepted_wave.accepted_run_id = Some("team-run-1".into());
    accepted_wave.gate_status = LegacyWaveGateStatus::Accepted;
    accepted_wave.accepted_by = Some("host".into());
    accepted_wave.accepted_at = Some("unix-ms:2".into());
    accepted_wave.updated_at = "unix-ms:2".into();
    // The test-only Legacy Wave writer is gone with DOC-108; historical
    // rows are written to the ledger directly, exactly like an export
    // restore.
    store
        .append_jsonl("waves.jsonl", &wave)
        .expect("append legacy wave");
    store
        .append_jsonl("waves.jsonl", &accepted_wave)
        .expect("append accepted legacy wave");

    assert_eq!(store.missions().expect("raw missions").len(), 2);
    assert_eq!(store.legacy_waves().expect("raw waves").len(), 2);
    assert_eq!(
        store.latest_missions().expect("latest missions"),
        vec![updated_mission]
    );
    assert_eq!(
        store.latest_legacy_waves().expect("latest waves"),
        vec![accepted_wave]
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
