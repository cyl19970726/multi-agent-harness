use super::*;

#[test]
fn post_team_run_message_and_start_async() {
    let home = TempHome::new("team-run-msg");
    let _project_id = init_project(&home, "alpha");
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

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Route mail",
            "members": [
                {"name": "lead", "role": "coordinator", "provider": "kimi",
                 "initial_work": "Coordinate delivery"},
                {"name": "worker-1", "role": "implementer", "provider": "kimi",
                 "initial_work": "Implement the requested change"},
            ],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_ids: Vec<String> = body["result"]["member_runs"]
        .as_array()
        .expect("member runs")
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(member_ids.len(), 3);
    // Route a handoff from the worker to the lead.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": member_ids[1],
            "recipient_runtime_ids": [member_ids[0]],
            "kind": "message",
            "body": "take over the review",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");
    assert_eq!(body["result"]["kind"].as_str(), Some("message"));
    assert!(body["result"]["correlation_id"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(body["result"]["causation_id"].is_null());
    assert_eq!(
        body["result"]["team_run_id"].as_str(),
        Some(run_id.as_str())
    );
    assert_eq!(
        body["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    // Work ownership is separate from the one explicit conversation message.
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["team_messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["team_run_events"].as_array().map(Vec::len),
        Some(6)
    );

    // Unknown run id is rejected by the canonical trust contract, with no append.
    let (status, body) = serve.post_json(
        "/v1/team-runs/team-run-nope/messages",
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_ids[0]],
            "kind": "control",
            "body": "ping",
        }),
    );
    assert_eq!(status, 409, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(false), "body: {body}");

    // HTTP start claims planning -> running synchronously, then drives the
    // provider work in the background.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(true), "body: {body}");
    assert_eq!(
        body["result"]["id"].as_str(),
        Some(run_id.as_str()),
        "body: {body}"
    );
    assert_eq!(body["result"]["status"].as_str(), Some("running"));

    let (status, host_notice) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": member_ids[1],
            "recipient_runtime_ids": [member_ids[0].clone()],
            "kind": "message",
            "body": "The Work is ready for Host review",
        }),
    );
    assert_eq!(status, 200, "body: {host_notice}");
    let host_handoff_id = host_notice["result"]["id"]
        .as_str()
        .expect("Host notice id")
        .to_string();

    // The URL TeamRun fence still rejects cross-run acknowledgement. Canonical
    // member delivery itself is acknowledged by the NodeDaemon after the
    // provider receipt; there is no manual Host ACK shadow ledger.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/wrong-run/messages/{host_handoff_id}/ack"),
        &serde_json::json!({"member_id": "host"}),
    );
    assert_eq!(status, 410, "retired manual ACK writer: {body}");
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let delivery = snapshot["team_messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| message["id"].as_str() == Some(host_handoff_id.as_str()))
        .map(|message| message["deliveries"][0].clone())
        .expect("canonical informational delivery");
    assert_eq!(delivery["status"].as_str(), Some("queued"));
}
