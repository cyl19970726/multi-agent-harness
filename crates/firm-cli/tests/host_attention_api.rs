//! Console follow-up acceptance (docs/design/console-followups.md):
//! HostAttention HTTP surface (read + ack lifecycle) and the standalone
//! member resume endpoint. Both run against a real `harness serve` with the
//! fake kimi ACP provider, so the lifecycle exercised here is the production
//! path, not a store-level unit.

use std::time::{Duration, Instant};

mod fake_provider;
mod firm_env;
use firm_env::{
    assign_work_for_member_run, create_canonical_agent_member, current_project_id,
    member_run_for_work_owner, run_firm, run_firm_with_env, ServeHandle, TempHome,
};

fn init_project(home: &TempHome, name: &str) -> (String, String, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    assert!(node.status.success(), "node init failed: {node:?}");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    let registration = run_firm(
        home,
        &root,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    assert!(
        registration.status.success(),
        "register failed: {registration:?}"
    );
    let host = create_canonical_agent_member(
        home,
        &root,
        &project_id,
        "agent-console-host",
        "console-host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host create failed: {host:?}");
    let host_id = "agent-console-host".to_string();
    (project_id, node_id, host_id)
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

fn create_mission_and_run(
    home: &TempHome,
    serve: &ServeHandle,
    project_id: &str,
    node_id: &str,
    host_id: &str,
) -> (String, String, String, u64) {
    // DOC-108 retired the Mission HTTP writer this fixture used; legacy
    // Mission provenance is seeded directly as pre-cutover history.
    firm_env::seed_historical_mission(
        home,
        project_id,
        "mission-console-followups",
        "Console follow-ups",
    );
    let team = run_firm(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "team",
            "create",
            "--id",
            "team-console-followups",
            "--name",
            "Console follow-ups team",
            "--description",
            "Flat team for HostAttention runtime coverage",
            "--mission-id",
            "mission-console-followups",
            "--host-agent-id",
            host_id,
            "--node-id",
            node_id,
            "--member",
            host_id,
        ],
    );
    assert!(team.status.success(), "team create failed: {team:?}");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs?project={project_id}"),
        &serde_json::json!({
            "agent_team_id": "team-console-followups",
            "objective": "Drive the console follow-up routes",
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
    let member_id = member_run_for_work_owner(&body["result"], 0)["id"]
        .as_str()
        .expect("Work owner MemberRun")
        .to_string();
    let work_id = body["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let work_version = body["result"]["works"][0]["version"]
        .as_u64()
        .expect("Work version");
    (run_id, member_id, work_id, work_version)
}

#[cfg(any())]
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

fn wait_for_member_runtime_ready(
    serve: &ServeHandle,
    project_id: &str,
    member_id: &str,
    within: Duration,
) {
    let deadline = Instant::now() + within;
    loop {
        let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
        assert_eq!(status, 200);
        let ready = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id)
                    && member["status"].as_str() == Some("idle")
                    && member["coordination_status"].as_str() == Some("active")
                    && member["native_session"].is_object()
            });
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "member {member_id} never published an active idle provider runtime: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn host_attentions_read_and_console_ack_lifecycle() {
    let home = TempHome::new("host-attention-console");
    let (project_id, node_id, host_id) = init_project(&home, "alpha");
    let serve = spawn_fake_kimi_serve(&home);
    let (run_id, member_id, work_id, work_version) =
        create_mission_and_run(&home, &serve, &project_id, &node_id, &host_id);

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_member_runtime_ready(&serve, &project_id, &member_id, Duration::from_secs(15));
    let bound = assign_work_for_member_run(&home, &project_id, &work_id, &member_id, true);
    assert_eq!(bound.version, work_version);
    firm_env::provider_received_work::record_provider_received_work(
        &home,
        &project_id,
        &work_id,
        "host-attention",
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
            &work_version.to_string(),
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

// Historical run-addressed Close/Resume shell. The provider effect can no
// longer be treated as stopped when its acknowledgement is uncertain;
// canonical RuntimeCommand recovery tests own the executable lifecycle
// contract and require RecoveryRequired instead of a compatibility retry.
#[cfg(any())]
#[test]
fn member_resume_route_rejects_active_and_resumes_closed_member() {
    let home = TempHome::new("member-resume-console");
    let (project_id, node_id, host_id) = init_project(&home, "alpha");
    let serve = spawn_fake_kimi_serve(&home);
    let (run_id, member_id, _work_id, _work_version) =
        create_mission_and_run(&home, &serve, &project_id, &node_id, &host_id);

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_member_runtime_ready(&serve, &project_id, &member_id, Duration::from_secs(15));

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
    assert_eq!(
        body["result"]["resume"]["via"].as_str(),
        Some("resume"),
        "body: {body}"
    );
    assert_eq!(
        body["result"]["resume"]["member_run"]["coordination_status"].as_str(),
        Some("active"),
        "resume reactivates coordination: {body}"
    );
    assert_eq!(
        body["result"]["resume"]["runtime_activation"].as_str(),
        Some("supervisor_rescan"),
        "the existing NodeDaemon child supervisor owns resumed execution: {body}"
    );
    let runtime_start = &body["result"]["runtime_start"];
    if !runtime_start.is_null() {
        assert_eq!(
            runtime_start["team_run_id"].as_str(),
            Some(run_id.as_str()),
            "resume must target the same managed TeamRun: {body}"
        );
        assert_eq!(
            runtime_start["daemon_response"]["reused"].as_bool(),
            Some(true),
            "the existing NodeDaemon child supervisor is reused, not duplicated: {body}"
        );
    }
}
