use super::*;

#[test]
fn reap_finalizes_runs_whose_host_process_is_dead_regardless_of_age() {
    let store = temp_store("reap-pid");
    // A child we immediately reap, so its pid is guaranteed dead on this host.
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let dead_pid = child.id();
    child.wait().expect("wait true");

    let now = current_unix_ms();
    // Created "now" (well under the 4h backstop) but its driver pid is dead —
    // so it must be reaped on pid-liveness alone, not the age window.
    store
        .append_workflow_run(&WorkflowRun {
            id: "wfrun-dead".into(),
            workflow_name: "demo".into(),
            project_binding_id: None,
            status: WorkflowRunStatus::Running,
            step_ids: vec!["wfstep-dead".into()],
            created_at: format!("unix-ms:{now}"),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("op".into()),
            design_intent: None,
            spec: None,
            host_pid: Some(dead_pid),
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        })
        .expect("append run");
    // A still-open step under it must be closed to Failed by the reaper too.
    store
        .append_workflow_step(&WorkflowStep {
            id: "wfstep-dead".into(),
            run_id: "wfrun-dead".into(),
            phase: "scan".into(),
            label: "scan-context".into(),
            native_session: None,
            status: WorkflowStepStatus::Running,
            output_summary: None,
            result: None,
            started_at: format!("unix-ms:{now}"),
            ended_at: None,
            terminal_reason: None,
            partial: false,
        })
        .expect("append step");
    // A run with a LIVE pid (this test process) must be left alone.
    store
        .append_workflow_run(&WorkflowRun {
            id: "wfrun-live".into(),
            workflow_name: "demo".into(),
            project_binding_id: None,
            status: WorkflowRunStatus::Running,
            step_ids: vec![],
            created_at: format!("unix-ms:{now}"),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("op".into()),
            design_intent: None,
            spec: None,
            host_pid: Some(std::process::id()),
            dry_run: false,
            terminal_reason: None,
            partial_output_available: false,
        })
        .expect("append live run");

    let reaped = reap_stale_workflow_runs(&store).expect("reap");
    assert_eq!(reaped, 1, "only the dead-pid run is reaped");

    let runs = latest_workflow_runs_in_append_order(&store).expect("read runs");
    let find = |id: &str| runs.iter().find(|r| r.id == id).expect("run present");
    assert_eq!(find("wfrun-dead").status, WorkflowRunStatus::Failed);
    assert!(find("wfrun-dead")
        .summary
        .as_deref()
        .unwrap_or("")
        .contains("no longer alive"));
    assert_eq!(
        find("wfrun-live").status,
        WorkflowRunStatus::Running,
        "a run whose driver is still alive must not be reaped"
    );

    let steps = latest_workflow_steps_in_append_order(&store).expect("read steps");
    let step = steps
        .iter()
        .find(|s| s.id == "wfstep-dead")
        .expect("step present");
    assert_eq!(
        step.status,
        WorkflowStepStatus::Failed,
        "the reaped run's open step is closed to Failed"
    );
    assert!(step.ended_at.is_some());
}
