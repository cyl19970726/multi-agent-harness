use super::*;

#[test]
fn terminal_team_runs_do_not_materialize_host_binding_stale_attention() {
    for status in [
        TeamRunStatus::Completed,
        TeamRunStatus::Failed,
        TeamRunStatus::Cancelled,
    ] {
        let root = team_test_root(&format!("terminal-stale-{status:?}"));
        let store = HarnessStore::new(&root);
        let (run, _, _) = seed_host_attention_fixture(
            &store,
            &format!("terminal-stale-{status:?}"),
            Some("thread-a"),
        );
        let mut terminal = run;
        terminal.status = status;
        terminal.completed_at = Some("unix-ms:2".into());
        terminal.updated_at = "unix-ms:2".into();
        append_sparse_row(
            &root,
            "team_runs.jsonl",
            &serde_json::to_string(&terminal).expect("serialize terminal run"),
        );
        assert!(store
            .reconcile_host_binding_stale_attentions(100, "unix-ms:100")
            .expect("reconcile")
            .is_empty());
        assert!(!store
            .host_attentions()
            .unwrap()
            .iter()
            .any(|attention| attention.kind == HostAttentionKind::HostBindingStale));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}
