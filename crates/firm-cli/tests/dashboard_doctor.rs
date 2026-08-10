//! Integration coverage for `harness dashboard doctor` (issue #307, item 3):
//! a read-only operator check that compares `GET /v1/meta` + a team-run's
//! `GET /v1/team-runs/{id}/snapshot` (exactly what the Workbench itself
//! fetches) against direct store reads and this binary's own build rev,
//! printing a pass/fail table and exiting non-zero on any mismatch.

mod firm_env;
use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};

/// Initialize the project plus the flat AgentTeam required by TeamRun creation.
fn init_project(home: &TempHome, name: &str) -> (String, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    let registration = run_firm(
        home,
        &root,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registration.status.success(),
        "node registration failed: {registration:?}"
    );
    let mission = run_firm(
        home,
        &root,
        &[
            "mission",
            "create",
            "--title",
            "Dashboard doctor mission",
            "--objective",
            "Verify read-only dashboard/store convergence",
        ],
    );
    assert!(
        mission.status.success(),
        "mission create failed: {mission:?}"
    );
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();
    let host = run_firm(
        home,
        &root,
        &[
            "agent",
            "create",
            "--name",
            "doctor-host",
            "--role",
            "host",
            "--provider",
            "codex",
        ],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let host: serde_json::Value = serde_json::from_slice(&host.stdout).expect("host JSON");
    let host_id = host["id"].as_str().expect("host id");
    let team = run_firm(
        home,
        &root,
        &[
            "team",
            "create",
            "--name",
            "Dashboard doctor team",
            "--description",
            "Flat dashboard test team",
            "--mission-id",
            &mission_id,
            "--host-agent-id",
            host_id,
            "--node-id",
            node_id,
            "--member",
            host_id,
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    (
        project_id,
        team["id"].as_str().expect("team id").to_string(),
    )
}

/// Create a TeamRun with one member (one Work via `initial_work`), then send it
/// one message — the minimum fixture doctor's three count checks need.
fn seed_team_run(serve: &ServeHandle, team_id: &str) -> (String, String) {
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id": team_id,
            "objective": "Exercise dashboard doctor",
            "members": [
                {"name": "lead", "role": "integrator", "provider": "codex",
                 "initial_work": "Ship the provenance surface"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let team_run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();

    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{team_run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "checking in",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");

    (team_run_id, member_id)
}

#[test]
fn doctor_passes_when_api_and_store_agree() {
    let home = TempHome::new("doctor-pass");
    let (_project_id, team_id) = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (team_run_id, _member_id) = seed_team_run(&serve, &team_id);
    let api = format!("http://127.0.0.1:{}", serve.port());

    let out = run_firm(
        &home,
        home.base(),
        &[
            "dashboard",
            "doctor",
            "--team-run-id",
            &team_run_id,
            "--api",
            &api,
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "doctor should exit 0 when everything agrees\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("PASS"), "stdout: {stdout}");
    assert!(!stdout.contains("FAIL"), "stdout: {stdout}");
    // Every row: works=1, members=1, messages=1, and (same binary talking to
    // itself) git_rev must trivially agree with itself.
    assert!(stdout.contains("works count"), "stdout: {stdout}");
    assert!(stdout.contains("members count"), "stdout: {stdout}");
    assert!(stdout.contains("messages count"), "stdout: {stdout}");
    assert!(stdout.contains("git_rev"), "stdout: {stdout}");
}

#[test]
fn doctor_fails_non_zero_on_unknown_team_run() {
    let home = TempHome::new("doctor-unknown-run");
    let (_project_id, _team_id) = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let api = format!("http://127.0.0.1:{}", serve.port());

    let out = run_firm(
        &home,
        home.base(),
        &[
            "dashboard",
            "doctor",
            "--team-run-id",
            "team-run-does-not-exist",
            "--api",
            &api,
        ],
    );
    assert!(
        !out.status.success(),
        "doctor must fail non-zero for an unknown TeamRun"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("404")
            || stderr.to_lowercase().contains("not_found")
            || stderr.to_lowercase().contains("not found"),
        "stderr should explain the 404: {stderr}"
    );
}

#[test]
fn doctor_fails_non_zero_on_expected_git_rev_mismatch_but_counts_still_pass() {
    let home = TempHome::new("doctor-rev-mismatch");
    let (_project_id, team_id) = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (team_run_id, _member_id) = seed_team_run(&serve, &team_id);
    let api = format!("http://127.0.0.1:{}", serve.port());

    let out = run_firm(
        &home,
        home.base(),
        &[
            "dashboard",
            "doctor",
            "--team-run-id",
            &team_run_id,
            "--api",
            &api,
            "--expected-git-rev",
            "0000000000000000000000000000000000000000",
        ],
    );
    assert!(
        !out.status.success(),
        "doctor must fail non-zero on a git_rev mismatch"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FAIL"), "stdout: {stdout}");
    // The count rows must still show PASS — only the rev row disagrees.
    assert!(
        stdout.contains("works count (store vs API)") && stdout.contains("PASS"),
        "counts should still pass alongside the rev failure: {stdout}"
    );
}

#[test]
fn doctor_requires_team_run_id_and_api_flags() {
    let home = TempHome::new("doctor-usage");
    let (_project_id, _team_id) = init_project(&home, "alpha");

    let missing_team_run = run_firm(
        &home,
        home.base(),
        &["dashboard", "doctor", "--api", "http://127.0.0.1:1"],
    );
    assert!(!missing_team_run.status.success());
    assert!(String::from_utf8_lossy(&missing_team_run.stderr).contains("--team-run-id"));

    let missing_api = run_firm(
        &home,
        home.base(),
        &["dashboard", "doctor", "--team-run-id", "team-run-x"],
    );
    assert!(!missing_api.status.success());
    assert!(String::from_utf8_lossy(&missing_api.stderr).contains("--api"));
}

#[test]
fn doctor_is_read_only_and_never_mutates_the_store() {
    let home = TempHome::new("doctor-read-only");
    let (project_id, team_id) = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (team_run_id, _member_id) = seed_team_run(&serve, &team_id);
    let api = format!("http://127.0.0.1:{}", serve.port());

    let store_path = home
        .spaces_dir()
        .join(&project_id)
        .join("work_operations.jsonl");
    let before = std::fs::read_to_string(&store_path).expect("work_operations before");

    let out = run_firm(
        &home,
        home.base(),
        &[
            "dashboard",
            "doctor",
            "--team-run-id",
            &team_run_id,
            "--api",
            &api,
        ],
    );
    assert!(out.status.success(), "{out:?}");

    let after = std::fs::read_to_string(&store_path).expect("work_operations after");
    assert_eq!(
        before, after,
        "dashboard doctor must not write to the store"
    );
}
