//! Console follow-up acceptance (docs/design/console-followups.md):
//! HostAttention HTTP surface (read + ack lifecycle) and the standalone
//! member resume endpoint. Both run against a real `harness serve` with the
//! fake kimi ACP provider, so the lifecycle exercised here is the production
//! path, not a store-level unit.

use std::time::{Duration, Instant};

mod fake_provider;
mod firm_env;
use firm_env::{current_project_id, run_firm, run_firm_with_env, ServeHandle, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

fn run_member_json(
    home: &TempHome,
    project_id: &str,
    team_run_id: &str,
    member_run_id: &str,
    args: &[&str],
) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(
        home,
        home.base(),
        &full,
        &[
            ("FIRM_TEAM_RUN_ID", team_run_id),
            ("FIRM_MEMBER_RUN_ID", member_run_id),
        ],
    );
    assert!(
        out.status.success(),
        "member harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("member harness {args:?} stdout was not JSON ({error})"))
}

fn spawn_fake_kimi_serve(home: &TempHome) -> ServeHandle {
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    // Leave enough time after observing `idle` for a separate CLI process to
    // start the Work on slower CI hosts before the test-only idle retirement.
    ServeHandle::spawn_with_env(
        home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    )
}

fn create_mission_and_run(serve: &ServeHandle, project_id: &str) -> (String, String, String) {
    let (status, body) = serve.post_json(
        &format!("/v1/missions?project={project_id}"),
        &serde_json::json!({
            "id": "mission-console-followups",
            "title": "Console follow-ups",
            "objective": "Exercise HostAttention and resume routes",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs?project={project_id}"),
        &serde_json::json!({
            "objective": "Drive the console follow-up routes",
            "mission_id": "mission-console-followups",
            "members": [{
                "name": "worker",
                "role": "implementer",
                "provider": "kimi",
                "initial_work": "Produce the evidence this console route needs.",
            }],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let work_id = body["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    (run_id, member_id, work_id)
}

fn wait_for_member_status(
    serve: &ServeHandle,
    project_id: &str,
    member_id: &str,
    expected: &str,
    within: Duration,
) {
    let deadline = Instant::now() + within;
    loop {
        let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
        assert_eq!(status, 200);
        let matches = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id)
                    && member["status"].as_str() == Some(expected)
            });
        if matches {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "member {member_id} never reached {expected}: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn host_attentions_read_and_console_ack_lifecycle() {
    let home = TempHome::new("host-attention-console");
    let project_id = init_project(&home, "alpha");
    let serve = spawn_fake_kimi_serve(&home);
    let (run_id, member_id, work_id) = create_mission_and_run(&serve, &project_id);

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_member_status(
        &serve,
        &project_id,
        &member_id,
        "idle",
        Duration::from_secs(15),
    );

    // Submitting the initial Work derives a WorkReviewRequested HostAttention.
    let started = run_member_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "1",
            "--member-run-id",
            &member_id,
            "--json",
        ],
    );
    let started_version = started["version"].as_u64().expect("started version");
    run_member_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            &started_version.to_string(),
            "--member-run-id",
            &member_id,
            "--result",
            "evidence for the console route",
            "--json",
        ],
    );

    let (status, body) = serve.get_json(&format!(
        "/v1/host-attentions?team_run_id={run_id}&project={project_id}"
    ));
    assert_eq!(status, 200, "body: {body}");
    let attention = body["attentions"]
        .as_array()
        .expect("attentions array")
        .iter()
        .find(|row| row["kind"].as_str() == Some("work_review_requested"))
        .expect("work review attention")
        .clone();
    let attention_id = attention["id"].as_str().expect("attention id").to_string();
    assert_eq!(attention["status"].as_str(), Some("actionable"));
    assert_eq!(attention["work_id"].as_str(), Some(work_id.as_str()));

    // Unknown runs are 404; unknown attentions are errors, not silent success.
    let (status, _) = serve.get_json(&format!(
        "/v1/host-attentions?team_run_id=nope&project={project_id}"
    ));
    assert_eq!(status, 404);

    // Console ack walks Actionable -> Claimed -> Delivered -> Acknowledged and
    // binds unbound http runs to the console host surface.
    let (status, body) = serve.post_json(
        &format!("/v1/host-attentions/{attention_id}/ack?project={project_id}"),
        &serde_json::json!({"acknowledged_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["attention"]["status"].as_str(),
        Some("acknowledged")
    );
    assert_eq!(body["result"]["idempotent"].as_bool(), Some(false));

    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
    assert_eq!(status, 200);
    let host_thread = snapshot["team_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|run| run["id"].as_str() == Some(run_id.as_str()))
        .and_then(|run| run["host_thread_id"].as_str().map(str::to_string));
    assert_eq!(
        host_thread.as_deref(),
        Some("console"),
        "console ack binds unbound runs"
    );

    let (status, body) = serve.post_json(
        &format!("/v1/host-attentions/{attention_id}/ack?project={project_id}"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["idempotent"].as_bool(), Some(true));
}

#[test]
fn member_resume_route_rejects_active_and_resumes_closed_member() {
    let home = TempHome::new("member-resume-console");
    let project_id = init_project(&home, "alpha");
    let serve = spawn_fake_kimi_serve(&home);
    let (run_id, member_id, _work_id) = create_mission_and_run(&serve, &project_id);

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_member_status(
        &serve,
        &project_id,
        &member_id,
        "idle",
        Duration::from_secs(15),
    );

    // An active member is continued by message/steer, never by resume.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/resume?project={project_id}"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("active"),
        "honest active-member refusal: {body}"
    );

    // Close the member, then resume the recorded native session.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close?project={project_id}"),
        &serde_json::json!({}),
    );
    assert!(status == 200 || status == 202, "body: {body}");
    wait_for_member_status(
        &serve,
        &project_id,
        &member_id,
        "stopped",
        Duration::from_secs(15),
    );

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/resume?project={project_id}"),
        &serde_json::json!({"resumed_by": "operator"}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["result"]["via"].as_str(), Some("resume"));
    assert_eq!(
        body["result"]["member_run"]["coordination_status"].as_str(),
        Some("active"),
        "resume reactivates coordination: {body}"
    );
}
