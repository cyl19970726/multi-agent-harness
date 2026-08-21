use super::*;

#[test]
fn reap_orphaned_workers_skips_pid_reuse_when_marker_does_not_match() {
    let store = temp_store("reap-worker-pid-reuse");
    append_test_workflow_run(&store, "wfrun-terminal", WorkflowRunStatus::Completed, None);
    let mut child = spawn_sleep_process_group();
    let pid = child.id();
    let pidfile = write_test_worker_pidfile(&store, "wfrun-terminal", pid, "codex");

    let summary = reap_orphaned_workers(&store, false).expect("reap workers");

    assert_eq!(summary["scanned"], 1);
    assert_eq!(summary["skipped_pid_reuse"], 1);
    assert!(
        pid_exists_libc(pid),
        "marker mismatch must not kill live pid"
    );
    assert!(!pidfile.exists(), "stale reused-pid pidfile is removed");
    kill_test_process_group(&mut child);
}
