use super::*;

#[test]
fn http_console_delegates_native_team_run_to_node_daemon() {
    let home = TempHome::new("mission-wave-console-start");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );

    // No Mission seeding: post-DEV-35 Teams are created without Mission
    // provenance, and DOC-108 retired the Mission writers entirely.
    let host = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "agent-console-host",
        "Console Host",
        "host",
        "codex",
        &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    );
    assert!(
        host.status.success(),
        "canonical host create failed: {host:?}"
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
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
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-console",
            "--name",
            "Console Team",
            "--description",
            "Flat Console Team",
            "--host-agent-id",
            "agent-console-host",
            "--node-id",
            &node_id,
            "--member",
            "agent-console-host",
        ],
    );
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Complete through the Console start endpoint",
            "agent_team_id": "team-console",
            "members": [{"name": "worker", "role": "implementer", "provider": "kimi", "initial_work": "Run the fake provider and return the requested evidence."}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member = member_run_for_work_owner(&body["result"], 0);
    let member_id = member["id"].as_str().expect("member id").to_string();
    let agent_member_id = member["agent_member_id"]
        .as_str()
        .expect("canonical AgentMember id")
        .to_string();
    let work_id = body["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();
    let work_version = body["result"]["works"][0]["version"]
        .as_u64()
        .expect("Work version");

    let daemon = run_firm_with_env(
        &home,
        home.base(),
        &["--project", &project_id, "daemon", "start"],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );
    assert!(
        daemon.status.success(),
        "start NodeDaemon failed: {}",
        String::from_utf8_lossy(&daemon.stderr)
    );
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("running"));
    assert_eq!(body["result"]["node_daemon"]["node_id"], node_id);

    // Repeated adoption is idempotent at the NodeDaemon boundary.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(
        body["result"]["node_daemon"]["daemon_response"]["reused"],
        true
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
        assert_eq!(status, 200);
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let completed_turn = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        if idle && completed_turn {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "member did not return to persistent idle: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

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
    let submitted_version = started_version + 1;
    let report_id = "report-console-result";
    let candidate = serde_json::json!({
        "kind": "content_digest",
        "value": "console-result-v1"
    });
    let candidate_fingerprint = harness_store::canonical_json_fingerprint(&candidate);
    let report_command = serde_json::json!({
        "command": "create_work_report",
        "team_id": "team-console",
        "report": {
            "id": report_id,
            "work_id": work_id,
            "work_revision": submitted_version,
            "report_revision": 1,
            "kind": "result",
            "authored_by": {"kind": "agent_member", "id": agent_member_id},
            "summary": "Host accepted the fake provider evidence",
            "base_revision": null,
            "candidate": candidate,
            "candidate_fingerprint": candidate_fingerprint,
            "finding_refs": [],
            "failure_analysis_ref": null,
            "artifact_refs": [],
            "check_refs": [],
            "evidence_refs": ["fake-provider-round"],
            "known_risks": [],
            "confidence": "high",
            "recommended_next_action": "accept",
            "created_at": "unix-ms:1"
        }
    })
    .to_string();
    run_json(
        &home,
        &project_id,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "agent_member",
            "--actor-id",
            &agent_member_id,
            "--idempotency-key",
            "console-work-report",
            "--expected-version",
            "0",
            "--json",
            &report_command,
        ],
    );
    let accept_command = serde_json::json!({
        "command": "accept_work",
        "team_id": "team-console",
        "work_id": work_id,
        "work_report_id": report_id,
        "candidate_fingerprint": candidate_fingerprint,
        "updated_at": "unix-ms:2"
    })
    .to_string();
    let accepted = run_json(
        &home,
        &project_id,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "agent_member",
            "--actor-id",
            "agent-console-host",
            "--idempotency-key",
            "console-work-accept",
            "--expected-version",
            &submitted_version.to_string(),
            "--json",
            &accept_command,
        ],
    );
    assert_eq!(accepted["projection"]["phase"].as_str(), Some("closed"));
    assert_eq!(
        accepted["projection"]["resolution"].as_str(),
        Some("accepted")
    );

    let (status, completed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition?project={project_id}"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {completed}");
    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
    assert_eq!(status, 200);
    assert!(
        snapshot["member_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["action_type"].as_str() != Some("thinking")),
        "thinking became durable: {}",
        snapshot["member_actions"]
    );
    assert!(
        !snapshot.to_string().contains("hidden reasoning"),
        "thinking leaked into snapshot"
    );

    // The legacy unscoped provider ingress is retired for every payload. Live
    // activity now enters only through the exact-AgentSession daemon bridge.
    let (status, body) = serve.post_json(
        &format!("/v1/live/member-activity?project={project_id}"),
        &serde_json::json!({
            "team_run_id": run_id,
            "member_run_id": member_id,
            "preview": "too late",
        }),
    );
    assert_eq!(status, 410, "body: {body}");
    assert_eq!(body["error"].as_str(), Some("retired_live_member_activity"));
    let stopped = run_firm(
        &home,
        home.base(),
        &["--project", &project_id, "daemon", "stop"],
    );
    assert!(
        stopped.status.success(),
        "stop NodeDaemon failed: {stopped:?}"
    );

    // Wave gate routes stay retired (ADR 0051), and the Mission Log writer
    // that once recorded closeout evidence is itself retired (DOC-108): run
    // acceptance is recorded on the Work/TeamRun path, never in a Mission.
    let (status, body) = serve.post_json(
        &format!("/v1/waves/wave-console/gate?project={project_id}"),
        &serde_json::json!({
            "status": "accepted",
            "run_id": run_id,
            "accepted_by": "console-host",
            "outcome": "deterministic provider completed",
            "artifact_refs": ["check:http-console"],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("retired"),
        "body: {body}"
    );
}
