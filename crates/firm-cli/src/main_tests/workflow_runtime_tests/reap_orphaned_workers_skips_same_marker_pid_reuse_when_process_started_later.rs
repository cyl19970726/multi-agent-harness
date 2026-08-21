use super::*;

#[test]
fn reap_orphaned_workers_skips_same_marker_pid_reuse_when_process_started_later() {
    let store = temp_store("reap-worker-same-marker-pid-reuse");
    append_test_workflow_run(&store, "wfrun-terminal", WorkflowRunStatus::Completed, None);
    let stale_started_ms = current_unix_ms().saturating_sub(10_000);
    let mut child = spawn_sleep_process_group();
    let pid = child.id();
    let pidfile = write_test_worker_pidfile_with_started_ms(
        &store,
        "wfrun-terminal",
        pid,
        "sleep",
        stale_started_ms,
    );

    let summary = reap_orphaned_workers(&store, false).expect("reap workers");

    assert_eq!(summary["scanned"], 1);
    assert_eq!(summary["skipped_pid_reuse"], 1);
    assert!(
        pid_exists_libc(pid),
        "same-marker reused pid must not be killed when start time is newer"
    );
    assert!(!pidfile.exists(), "stale reused-pid pidfile is removed");
    kill_test_process_group(&mut child);
}
