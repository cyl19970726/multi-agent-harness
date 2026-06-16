//! Integration coverage for per-project SSE multiplexing
//! (goal-multi-project P6, sse-multiplex task).
//!
//! Two registered projects, one serve. A client subscribed to project A's
//! `/v1/events?project=<A>` must receive frames appended to A's store and NEVER
//! frames appended to B's store, and vice versa. This proves the watcher's
//! per-(project,filename) offsets and per-project broadcast channels keep the two
//! streams isolated even though every project has an identically-named
//! `messages.jsonl`.

use std::time::Duration;

mod harness_env;
use harness_env::{collect_sse_data, current_project_id, run_harness, ServeHandle, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Append a real message to a specific project's store (drives the watcher's
/// `messages.jsonl` tail).
fn create_message(home: &TempHome, project_id: &str, msg_id: &str, content: &str) {
    let out = run_harness(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "message",
            "send",
            "--id",
            msg_id,
            "--from",
            "tester",
            "--content",
            content,
        ],
    );
    assert!(out.status.success(), "message create failed: {out:?}");
}

fn message_ids(frames: &[serde_json::Value]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| f["id"].as_str().map(|s| s.to_string()))
        .collect()
}

#[test]
fn sse_streams_are_isolated_per_project() {
    let home = TempHome::new("sse-iso");
    let id_a = init_project(&home, "alpha");
    let id_b = init_project(&home, "beta");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Open one SSE stream per project (drained past the initial snapshot).
    let mut sse_a = serve.open_sse(&format!("?project={id_a}"));
    let mut sse_b = serve.open_sse(&format!("?project={id_b}"));

    // Append a row to EACH project's store after the streams are live.
    create_message(&home, &id_a, "msg-alpha", "hello alpha");
    create_message(&home, &id_b, "msg-beta", "hello beta");

    // Collect a few frames from each (watcher poll is ~150ms).
    let frames_a = collect_sse_data(&mut sse_a, Duration::from_secs(4), 1);
    let frames_b = collect_sse_data(&mut sse_b, Duration::from_secs(4), 1);

    let ids_a = message_ids(&frames_a);
    let ids_b = message_ids(&frames_b);

    assert!(
        ids_a.contains(&"msg-alpha".to_string()),
        "stream A missing its own frame: {ids_a:?}"
    );
    assert!(
        !ids_a.contains(&"msg-beta".to_string()),
        "stream A LEAKED project B's frame: {ids_a:?}"
    );
    assert!(
        ids_b.contains(&"msg-beta".to_string()),
        "stream B missing its own frame: {ids_b:?}"
    );
    assert!(
        !ids_b.contains(&"msg-alpha".to_string()),
        "stream B LEAKED project A's frame: {ids_b:?}"
    );
}

#[test]
fn events_without_project_uses_active_default_stream() {
    let home = TempHome::new("sse-default");
    let _id_a = init_project(&home, "alpha");
    let id_b = init_project(&home, "beta"); // beta active

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // No ?project → the active project (beta).
    let mut sse = serve.open_sse("");
    create_message(&home, &id_b, "msg-default", "to active");

    let frames = collect_sse_data(&mut sse, Duration::from_secs(4), 1);
    let ids = message_ids(&frames);
    assert!(
        ids.contains(&"msg-default".to_string()),
        "default stream did not receive active project's frame: {ids:?}"
    );
}
