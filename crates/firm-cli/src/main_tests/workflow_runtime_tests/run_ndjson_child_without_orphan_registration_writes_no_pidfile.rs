use super::*;

#[test]
fn run_ndjson_child_without_orphan_registration_writes_no_pidfile() {
    let root = std::env::temp_dir().join(format!("mah-no-pidfile-{}", generated_id("t")));
    let session_dir = root.join("runtimes/test-workers").join("s");
    fs::create_dir_all(&session_dir).expect("mkdir");
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("printf '{\"type\":\"item\"}\\n'");

    let run = run_ndjson_child(
        cmd,
        &session_dir,
        "s",
        "out.ndjson",
        1_000,
        None,
        None,
        "ephemeral worker",
    )
    .expect("run");

    assert!(run.process_success);
    assert_eq!(run.events.len(), 1);
    assert!(
        !root.join("worker_pids").exists(),
        "no pid registry is created unless a caller registers the worker"
    );
    let _ = fs::remove_dir_all(&root);
}
