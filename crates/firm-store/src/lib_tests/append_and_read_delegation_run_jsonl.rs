use super::*;

#[test]
fn append_and_read_delegation_run_jsonl() {
    let root = team_test_root("delegation-run");
    let store = HarnessStore::new(&root);
    let delegation = DelegationRun {
        id: "dr-1".into(),
        team_run_id: "tr-1".into(),
        parent_member_run_id: "mr-1".into(),
        parent_task_id: Some("task-1".into()),
        mode: DelegationMode::HarnessWorker,
        provider: "claude".into(),
        provider_child_thread_id: None,
        workflow_run_id: Some("wfr-1".into()),
        objective: "Research X".into(),
        status: DelegationStatus::Running,
        evidence_ids: vec!["ev-1".into()],
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:2".into(),
    };

    store
        .append_delegation_run(&delegation)
        .expect("append delegation run");
    append_sparse_row(
        &root,
        "delegation_runs.jsonl",
        r#"{"id":"dr-sparse","team_run_id":"tr-1","parent_member_run_id":"mr-1","mode":"provider_native","provider":"codex","objective":"obj","status":"planned","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
    );

    let delegations = store.delegation_runs().expect("read delegation runs");
    assert_eq!(delegations.len(), 2);
    assert_eq!(delegations[0], delegation);
    let sparse = &delegations[1];
    assert_eq!(sparse.id, "dr-sparse");
    assert_eq!(sparse.mode, DelegationMode::ProviderNative);
    assert_eq!(sparse.status, DelegationStatus::Planned);
    assert!(sparse.parent_task_id.is_none());
    assert!(sparse.provider_child_thread_id.is_none());
    assert!(sparse.workflow_run_id.is_none());
    assert!(sparse.evidence_ids.is_empty());

    std::fs::remove_dir_all(root).expect("remove temp store");
}
