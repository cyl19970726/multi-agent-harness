//! Integration coverage for per-Execution-Space SSE multiplexing.
//!
//! Project Binding selection is deliberately irrelevant to stream ownership.

use std::time::Duration;

mod firm_env;
use firm_env::{
    collect_sse_data, create_canonical_agent_member, current_project_id, run_firm, ServeHandle,
    TempHome,
};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Append a native Mission to a specific project's store.
fn create_space(home: &TempHome, id: &str, project_binding: &str) {
    let out = run_firm(
        home,
        home.base(),
        &[
            "space",
            "init",
            "--id",
            id,
            "--name",
            id,
            "--project-binding",
            project_binding,
        ],
    );
    assert!(out.status.success(), "space init failed: {out:?}");
}

/// DOC-108 retired the Mission writers: Mission rows are pre-cutover history
/// seeded directly into the Space ledger. The objective lands in the row so
/// ledger-surgery fixtures can rewrite it.
fn create_mission(home: &TempHome, space_id: &str, project_id: &str, id: &str, objective: &str) {
    let _ = project_id;
    use std::io::Write as _;
    let dir = home.spaces_dir().join(space_id);
    std::fs::create_dir_all(&dir).expect("create space store dir");
    let mut ledger = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("missions.jsonl"))
        .expect("open mission ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": id,
            "title": id,
            "objective": objective,
            "status": "planned",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical mission");
}

fn create_team(home: &TempHome, space_id: &str, project_id: &str) -> String {
    let run = |args: &[&str]| {
        let mut full = vec!["--space", space_id, "--project", project_id];
        full.extend_from_slice(args);
        let output = run_firm(home, home.base(), &full);
        assert!(
            output.status.success(),
            "fixture {args:?} failed: {output:?}"
        );
        output
    };
    let node = run(&["node", "init"]);
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--execution-space-id",
        space_id,
        "--project-binding-id",
        project_id,
    ]);
    create_mission(
        home,
        space_id,
        project_id,
        "mission-sse-work",
        "Verify external Work stream convergence",
    );
    let host_id = "agent-sse-host";
    let host = create_canonical_agent_member(
        home,
        home.base(),
        project_id,
        host_id,
        "sse-host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "canonical host failed: {host:?}");
    let worker_id = "agent-sse-worker";
    let worker = create_canonical_agent_member(
        home,
        home.base(),
        project_id,
        worker_id,
        "worker",
        "builder",
        "kimi",
        &[("FIRM_SPACE", space_id)],
    );
    assert!(
        worker.status.success(),
        "canonical worker failed: {worker:?}"
    );
    let team = run(&[
        "team",
        "create",
        "--name",
        "SSE Work team",
        "--description",
        "Flat external Work stream test team",
        "--mission-id",
        "mission-sse-work",
        "--host-agent-id",
        host_id,
        "--node-id",
        node_id,
        "--member",
        host_id,
        "--member",
        worker_id,
    ]);
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    team["id"].as_str().expect("team id").to_string()
}

fn invalidation_keys(frames: &[serde_json::Value]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| {
            Some(format!(
                "{}:{}",
                frame["scope_id"].as_str()?,
                frame["ledger"].as_str()?
            ))
        })
        .collect()
}

fn json_output(output: &std::process::Output, context: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{context} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} returned non-JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn sse_streams_are_isolated_per_execution_space() {
    let home = TempHome::new("sse-iso");
    let id_a = init_project(&home, "alpha");
    let id_b = init_project(&home, "beta");
    create_space(&home, "space-alpha", &id_a);
    create_space(&home, "space-beta", &id_b);

    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Open one SSE stream per project (drained past the initial snapshot).
    let mut sse_a = serve.open_sse(&format!("?space=space-alpha&project={id_a}"));
    let mut sse_b = serve.open_sse(&format!("?space=space-beta&project={id_b}"));

    // Append a row to EACH project's store after the streams are live.
    create_mission(&home, "space-alpha", &id_a, "mission-alpha", "hello alpha");
    create_mission(&home, "space-beta", &id_b, "mission-beta", "hello beta");

    // Collect a few frames from each (watcher poll is ~150ms).
    let frames_a = collect_sse_data(&mut sse_a, Duration::from_secs(4), 1);
    let frames_b = collect_sse_data(&mut sse_b, Duration::from_secs(4), 1);

    let ids_a = invalidation_keys(&frames_a);
    let ids_b = invalidation_keys(&frames_b);

    assert!(
        ids_a.contains(&"space-alpha:missions.jsonl".to_string()),
        "stream A missing its own frame: {ids_a:?}"
    );
    assert!(
        !ids_a.contains(&"space-beta:missions.jsonl".to_string()),
        "stream A LEAKED project B's frame: {ids_a:?}"
    );
    assert!(
        ids_b.contains(&"space-beta:missions.jsonl".to_string()),
        "stream B missing its own frame: {ids_b:?}"
    );
    assert!(
        !ids_b.contains(&"space-alpha:missions.jsonl".to_string()),
        "stream B LEAKED project A's frame: {ids_b:?}"
    );
}

/// A project registered AFTER serve started must still get a live `/v1/events`
/// channel: the watcher re-scans the registry each poll, discovers the new project,
/// and broadcasts a freshly-appended row to a client subscribed to it — no serve
/// restart required (goal-multi-project #147 follow-up). With the old startup-only
/// `watch_map`, this stream would receive ZERO frames.
#[test]
fn newly_registered_space_gets_live_sse_without_restart() {
    let home = TempHome::new("sse-new-project");
    let id_a = init_project(&home, "alpha");
    create_space(&home, "space-alpha", &id_a);

    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Register a NEW project after serve is already running. It is not in the
    // startup watch_map, so it only becomes watchable if serve re-scans the registry.
    let id_new = init_project(&home, "gamma");
    create_space(&home, "space-gamma", &id_new);
    assert_ne!(
        id_new, id_a,
        "gamma must be a distinct, post-startup project"
    );

    // Subscribe to the new project's stream, then append a row to its store.
    let mut sse_new = serve.open_sse(&format!("?space=space-gamma&project={id_new}"));
    create_mission(
        &home,
        "space-gamma",
        &id_new,
        "mission-gamma",
        "hello gamma",
    );

    let frames = collect_sse_data(&mut sse_new, Duration::from_secs(6), 1);
    let ids = invalidation_keys(&frames);
    assert!(
        ids.contains(&"space-gamma:missions.jsonl".to_string()),
        "newly-registered project's SSE stream did not receive its live frame \
         (watcher likely did not re-scan the registry): {ids:?}"
    );
}

#[test]
fn events_without_space_uses_active_default_stream() {
    let home = TempHome::new("sse-default");
    let _id_a = init_project(&home, "alpha");
    let id_b = init_project(&home, "beta"); // beta active
    create_space(&home, "space-beta", &id_b);

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // No ?project → the active project (beta).
    let mut sse = serve.open_sse("");
    create_mission(&home, "space-beta", &id_b, "mission-default", "to active");

    let frames = collect_sse_data(&mut sse, Duration::from_secs(4), 1);
    let ids = invalidation_keys(&frames);
    assert!(
        ids.contains(&"space-beta:missions.jsonl".to_string()),
        "default stream did not receive active project's frame: {ids:?}"
    );
}

#[test]
fn invalidation_is_queued_across_snapshot_marker_to_get_boundary() {
    let home = TempHome::new("sse-snapshot-get-boundary");
    let project_id = init_project(&home, "alpha");
    create_space(&home, "space-alpha", &project_id);

    // Keep the server handler parked immediately after the client sees the
    // initial marker. A subscriber registered after the marker would miss the
    // write below; a subscriber registered first queues it until streaming
    // resumes.
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("FIRM_TEST_SSE_POST_SNAPSHOT_PAUSE_MS", "1200")],
    );
    let query = format!("?space=space-alpha&project={project_id}");
    let mut sse = serve.open_sse(&query);

    create_mission(
        &home,
        "space-alpha",
        &project_id,
        "mission-crossing",
        "written after marker before authoritative GET",
    );

    let frames = collect_sse_data(&mut sse, Duration::from_secs(5), 1);
    assert!(
        invalidation_keys(&frames).contains(&"space-alpha:missions.jsonl".to_string()),
        "write crossing initial marker boundary was not queued: {frames:?}"
    );
}

#[test]
fn external_work_write_invalidates_and_current_snapshot_converges() {
    let home = TempHome::new("sse-external-work");
    let project_id = init_project(&home, "alpha");
    create_space(&home, "space-alpha", &project_id);
    let team_id = create_team(&home, "space-alpha", &project_id);

    let created_run = json_output(
        &run_firm(
            &home,
            home.base(),
            &[
                "--space",
                "space-alpha",
                "--project",
                &project_id,
                "team-run",
                "create",
                "--agent-team-id",
                &team_id,
                "--objective",
                "Exercise external Work projection",
                "--host-runtime-mode",
                "external_interactive",
                "--member",
                "agent-sse-host:host:codex/external_interactive",
                "--member",
                "agent-sse-worker:builder:kimi#Own external Work",
                "--json",
            ],
        ),
        "team-run create",
    );
    let team_run_id = created_run["team_run"]["id"]
        .as_str()
        .expect("team run id")
        .to_string();

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let query = format!("?space=space-alpha&project={project_id}");
    let mut sse = serve.open_sse(&query);

    // This is a separate real CLI process writing after the EventSource is
    // already healthy. Before this regression fix it produced no SSE delta.
    let created_work = json_output(
        &run_firm(
            &home,
            home.base(),
            &[
                "--space",
                "space-alpha",
                "--project",
                &project_id,
                "team-run",
                "work",
                "create",
                "--team-run-id",
                &team_run_id,
                "--title",
                "work-after-stream-open",
                "--completion-criteria",
                "Dashboard converges without reload",
            ],
        ),
        "external Work create",
    );
    let work_id = created_work["id"].as_str().expect("Work id");
    let invalidations = collect_sse_data(&mut sse, Duration::from_secs(6), 1);
    let work_invalidation = invalidations
        .iter()
        .find(|frame| frame["ledger"] == "work_operations.jsonl")
        .unwrap_or_else(|| panic!("healthy SSE missed external Work append: {invalidations:?}"));
    assert_eq!(work_invalidation["scope"], "execution_space");
    assert_eq!(work_invalidation["scope_id"], "space-alpha");
    assert_eq!(work_invalidation["reason"], "append");
    assert!(work_invalidation["stream_epoch"].as_str().is_some());

    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot{query}"));
    assert_eq!(status, 200);
    assert!(
        snapshot["works"]
            .as_array()
            .expect("snapshot Works")
            .iter()
            .any(|work| work["id"] == work_id),
        "authoritative snapshot did not converge after invalidation: {snapshot}"
    );

    assert!(
        snapshot["work_deliveries"]
            .as_array()
            .expect("snapshot deliveries")
            .iter()
            .all(|candidate| candidate["work_id"] != work_id),
        "creating Work alone must not invent provider delivery: {snapshot}"
    );
}

#[test]
fn typed_ledger_replace_delete_and_reconnect_recover_only_the_selected_scope() {
    let home = TempHome::new("sse-typed-replace-delete");
    let project_a = init_project(&home, "alpha");
    let project_b = init_project(&home, "beta");
    create_space(&home, "space-alpha", &project_a);
    create_space(&home, "space-beta", &project_b);
    create_mission(
        &home,
        "space-alpha",
        &project_a,
        "mission-recovery",
        "before",
    );

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let query_a = format!("?space=space-alpha&project={project_a}");
    let query_b = format!("?space=space-beta&project={project_b}");
    let mut sse_a = serve.open_sse(&query_a);
    let mut sse_b = serve.open_sse(&query_b);
    let mission_path = home.spaces_dir().join("space-alpha").join("missions.jsonl");

    // Atomic replacement preserves byte length but changes the typed ledger's
    // identity. Incremental offsets alone cannot observe it, so Runtime must
    // invalidate only space-alpha and the authoritative snapshot must converge.
    let original = std::fs::read_to_string(&mission_path).expect("read mission ledger");
    let replaced = original.replace("before", "after!");
    assert_eq!(
        replaced.len(),
        original.len(),
        "fixture must stay same-size"
    );
    let replacement_path = mission_path.with_extension("jsonl.replace");
    std::fs::write(&replacement_path, &replaced).expect("write replacement ledger");
    std::fs::rename(&replacement_path, &mission_path).expect("atomic replace ledger");

    let replaced_frames = collect_sse_data(&mut sse_a, Duration::from_secs(6), 1);
    let replacement = replaced_frames
        .iter()
        .find(|frame| frame["ledger"] == "missions.jsonl")
        .unwrap_or_else(|| panic!("same-size typed replace was missed: {replaced_frames:?}"));
    assert_eq!(replacement["scope"], "execution_space");
    assert_eq!(replacement["scope_id"], "space-alpha");
    assert_eq!(replacement["reason"], "replace");
    assert!(
        collect_sse_data(&mut sse_b, Duration::from_millis(700), 1).is_empty(),
        "space-alpha replacement leaked to space-beta"
    );
    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot{query_a}"));
    assert_eq!(status, 200);
    assert!(snapshot["missions"]
        .as_array()
        .expect("missions")
        .iter()
        .any(|mission| mission["id"] == "mission-recovery" && mission["objective"] == "after!"));

    // Direct deletion is a real source transition, not an absence the watcher
    // may silently ignore. The selected snapshot must immediately stop showing
    // the deleted typed ledger's rows.
    std::fs::remove_file(&mission_path).expect("delete mission ledger");
    let deleted_frames = collect_sse_data(&mut sse_a, Duration::from_secs(6), 1);
    let deletion = deleted_frames
        .iter()
        .find(|frame| frame["ledger"] == "missions.jsonl")
        .unwrap_or_else(|| panic!("typed ledger deletion was missed: {deleted_frames:?}"));
    assert_eq!(deletion["scope_id"], "space-alpha");
    assert_eq!(deletion["reason"], "delete");
    let (status, empty_snapshot) = serve.get_json(&format!("/v1/snapshot{query_a}"));
    assert_eq!(status, 200);
    assert!(empty_snapshot["missions"]
        .as_array()
        .expect("missions")
        .iter()
        .all(|mission| mission["id"] != "mission-recovery"));

    // Reconnect carries no durable cursor. A fresh stream gets a new bounded
    // snapshot marker, then ledger recreation emits another scoped invalidation
    // and an authoritative refetch restores the projection.
    drop(sse_a);
    let mut reconnected = serve.open_sse(&query_a);
    std::fs::write(&mission_path, original).expect("recreate mission ledger");
    let recreated_frames = collect_sse_data(&mut reconnected, Duration::from_secs(6), 1);
    let recreation = recreated_frames
        .iter()
        .find(|frame| frame["ledger"] == "missions.jsonl")
        .unwrap_or_else(|| {
            panic!("recreated ledger was missed after reconnect: {recreated_frames:?}")
        });
    assert_eq!(recreation["reason"], "replace");
    assert_eq!(recreation["scope_id"], "space-alpha");
    let (status, recovered) = serve.get_json(&format!("/v1/snapshot{query_a}"));
    assert_eq!(status, 200);
    assert!(recovered["missions"]
        .as_array()
        .expect("missions")
        .iter()
        .any(|mission| mission["id"] == "mission-recovery" && mission["objective"] == "before"));
}

#[test]
fn unknown_execution_space_fails_closed_for_snapshot_and_events() {
    let home = TempHome::new("sse-unknown-space");
    let project_id = init_project(&home, "alpha");
    create_space(&home, "space-alpha", &project_id);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    for path in [
        "/v1/snapshot?space=typo-space",
        "/v1/events?space=typo-space",
    ] {
        let (status, body) = serve.get_json(path);
        assert_eq!(status, 404, "{path} must fail closed: {body}");
        assert_eq!(body["error"], "execution_space_not_found");
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("typo-space")),
            "error must name the rejected selector: {body}"
        );
    }
}
