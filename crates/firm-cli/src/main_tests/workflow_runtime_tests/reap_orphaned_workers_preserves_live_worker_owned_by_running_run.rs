use super::*;

#[test]
fn reap_orphaned_workers_preserves_live_worker_owned_by_running_run() {
    let store = temp_store("reap-worker-live-run");
    append_test_workflow_run(
        &store,
        "wfrun-live-owner",
        WorkflowRunStatus::Running,
        Some(std::process::id()),
    );
    let mut child = spawn_sleep_process_group();
    let pid = child.id();
    let pidfile = write_test_worker_pidfile(&store, "wfrun-live-owner", pid, "sleep");

    let summary = reap_orphaned_workers(&store, false).expect("reap workers");

    assert_eq!(summary["scanned"], 1);
    assert_eq!(summary["kept_running"], 1);
    assert!(pid_exists_libc(pid), "live owned worker is not killed");
    assert!(pidfile.exists(), "live owned worker pidfile is kept");
    kill_test_process_group(&mut child);
}
