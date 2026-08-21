use super::*;

#[test]
fn reap_stale_workflow_runs_finalizes_old_running_rows() {
    let store = temp_store("reap-stale");
    let now = current_unix_ms();
    let mk = |id: &str, created: u128| WorkflowRun {
        id: id.into(),
        workflow_name: "demo".into(),
        project_binding_id: None,
        status: WorkflowRunStatus::Running,
        step_ids: vec![],
        created_at: format!("unix-ms:{created}"),
        ended_at: None,
        summary: None,
        args: None,
        agents_spawned: 0,
        final_output: None,
        initiated_by: Some("op".into()),
        design_intent: None,
        spec: None,
        host_pid: None,
        dry_run: false,
        terminal_reason: None,
        partial_output_available: false,
    };
    // One Running run 5h old -> reaped to Failed; one started "now" -> stays.
    store
        .append_workflow_run(&mk("wfrun-old", now.saturating_sub(5 * 60 * 60 * 1000)))
        .expect("append old");
    store
        .append_workflow_run(&mk("wfrun-fresh", now))
        .expect("append fresh");

    let reaped = reap_stale_workflow_runs(&store).expect("reap");
    assert_eq!(reaped, 1);

    let runs = latest_workflow_runs_in_append_order(&store).expect("read");
    let find = |id: &str| runs.iter().find(|r| r.id == id).expect("run present");
    assert_eq!(find("wfrun-old").status, WorkflowRunStatus::Failed);
    assert!(find("wfrun-old")
        .summary
        .as_deref()
        .unwrap_or("")
        .contains("reaped"));
    assert!(find("wfrun-old").ended_at.is_some());
    assert_eq!(find("wfrun-fresh").status, WorkflowRunStatus::Running);
}
