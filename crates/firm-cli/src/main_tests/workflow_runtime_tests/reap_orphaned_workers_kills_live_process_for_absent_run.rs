use super::*;

#[test]
fn reap_orphaned_workers_kills_live_process_for_absent_run() {
    let store = temp_store("reap-worker-kill");
    let mut child = spawn_sleep_process_group();
    let pid = child.id();
    let pidfile = write_test_worker_pidfile(&store, "wfrun-missing", pid, "sleep");

    let summary = reap_orphaned_workers(&store, false).expect("reap workers");
    wait_for_child_exit(&mut child);

    assert_eq!(summary["scanned"], 1);
    assert_eq!(summary["killed"], 1);
    assert!(
        !pid_exists_libc(pid),
        "worker pid should be gone after wait"
    );
    assert!(!pidfile.exists(), "killed worker pidfile is removed");
}
