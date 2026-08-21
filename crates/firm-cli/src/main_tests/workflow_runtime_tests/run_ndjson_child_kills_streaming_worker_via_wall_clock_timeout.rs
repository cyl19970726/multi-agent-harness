use super::*;

#[test]
fn run_ndjson_child_kills_streaming_worker_via_wall_clock_timeout() {
    // A worker that never goes idle is still bounded by the per-leaf wall-clock
    // timeout.
    let root = std::env::temp_dir().join(format!("mah-wall-{}", generated_id("t")));
    let session_dir = root.join("runtimes/test-workers").join("s");
    fs::create_dir_all(&session_dir).expect("mkdir");
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("for i in $(seq 1 30); do printf '{\"type\":\"item\"}\\n'; sleep 0.1; done");

    let start = Instant::now();
    let run = run_ndjson_child(
        cmd,
        &session_dir,
        "s",
        "out.ndjson",
        5_000,
        Some(1_000),
        None,
        "ephemeral worker",
    )
    .expect("run");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "wall-clock timeout should fire near the cap; took {elapsed:?}"
    );
    assert!(run.wall_timed_out, "the wall-clock timeout must fire");
    assert!(run.timed_out, "wall-clock timeouts are terminal timeouts");
    assert!(!run.process_success);
    assert!(run.warnings.iter().any(|warning| {
        warning == "ephemeral worker exceeded per-leaf wall-clock timeout of 1s"
    }));
    assert!(
        !run.events.is_empty(),
        "streamed events before the wall-clock kill are retained"
    );
    let _ = fs::remove_dir_all(&root);
}
