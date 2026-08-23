use super::*;

#[test]
fn codex_app_server_question_routes_to_lead_and_resumes_same_turn() {
    let home = TempHome::new("team-run-codex-question");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-question"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let credentials = serde_json::json!([
        {
            "token": "host-answer-token",
            "actor": {"kind": "agent_member", "id": FIXTURE_HOST_ID},
            "authority_actors": []
        },
        {
            "token": "impostor-answer-token",
            "actor": {"kind": "service", "id": "not-the-team-host"},
            "authority_actors": []
        }
    ])
    .to_string();
    let thread_marker = home.base().join("codex-question-thread.jsonl");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_ASK", "1"),
            (
                "FAKE_CODEX_THREAD_MARKER",
                thread_marker.to_str().expect("thread marker path"),
            ),
            ("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Codex reverse input",
            "members": [{"name": "codex-question", "role": "implementer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise provider question routing"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let host_member_run_id = created["result"]["member_runs"]
        .as_array()
        .expect("created MemberRuns")
        .iter()
        .find(|member| member["agent_member_id"].as_str() == Some(FIXTURE_HOST_ID))
        .and_then(|member| member["id"].as_str())
        .expect("exact Host MemberRun")
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut interaction_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        interaction_id = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["sender_runtime_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("provider_interaction_request")
            })
            .and_then(|message| message["id"].as_str().map(str::to_string));
        if interaction_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let interaction_id = interaction_id.unwrap_or_else(|| {
        panic!(
            "Codex provider interaction request message; snapshot={}",
            serve.get_json("/v1/snapshot").1
        )
    });
    let opened = std::fs::read_to_string(&thread_marker).expect("Codex thread/start marker");
    assert!(
        opened.contains("\"sandbox\":\"danger-full-access\"")
            && opened.contains("\"approvalPolicy\":\"never\""),
        "Team provider launch must consume the frozen FullAccess mapping: {opened}"
    );
    let answer_path = format!("/v1/team-runs/{run_id}/messages/{interaction_id}/answer");
    let (status, unauthenticated) = serve.post_json(
        &answer_path,
        &serde_json::json!({"option_id": "implementation::0"}),
    );
    assert_eq!(status, 401, "body: {unauthenticated}");
    let (status, impersonation) = serve.post_json_with_headers(
        &answer_path,
        &serde_json::json!({"option_id": "implementation::0"}),
        &[
            ("X-AgentFirm-Token", "impostor-answer-token"),
            ("Idempotency-Key", "impostor-answer"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(status, 403, "body: {impersonation}");
    let (status, caller_selected_identity) = serve.post_json_with_headers(
        &answer_path,
        &serde_json::json!({"option_id": "implementation::0", "resolved_by": "host"}),
        &[
            ("X-AgentFirm-Token", "host-answer-token"),
            ("Idempotency-Key", "caller-selected-answer-identity"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(status, 409, "body: {caller_selected_identity}");
    let (status, invalid_option) = serve.post_json_with_headers(
        &answer_path,
        &serde_json::json!({"option_id": "not-exposed"}),
        &[
            ("X-AgentFirm-Token", "host-answer-token"),
            ("Idempotency-Key", "invalid-option-answer"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(status, 409, "body: {invalid_option}");
    let (_, before_valid) = serve.get_json("/v1/snapshot");
    assert!(
        before_valid["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|message| message["kind"].as_str() != Some("provider_interaction_response")),
        "rejected answers must have zero response side effects: {before_valid}"
    );
    let (status, resolved) = serve.post_json_with_headers(
        &answer_path,
        &serde_json::json!({"option_id": "implementation::0"}),
        &[
            ("X-AgentFirm-Token", "host-answer-token"),
            ("Idempotency-Key", "valid-host-answer"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(status, 200, "body: {resolved}");
    assert_eq!(
        resolved["result"]["kind"].as_str(),
        Some("provider_interaction_response")
    );
    let (retry_status, retried) = serve.post_json_with_headers(
        &answer_path,
        &serde_json::json!({"option_id": "implementation::0"}),
        &[
            ("X-AgentFirm-Token", "host-answer-token"),
            ("Idempotency-Key", "valid-host-answer-retry"),
            ("If-Match", "0"),
        ],
    );
    assert_eq!(retry_status, 200, "body: {retried}");
    assert_eq!(retried["result"]["id"], resolved["result"]["id"]);
    let mut idle_with_delivered_response = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let request_acknowledged = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(interaction_id.as_str()))
            .and_then(|message| message["deliveries"].as_array())
            .into_iter()
            .flatten()
            .any(|delivery| {
                delivery["member_id"].as_str() == Some(host_member_run_id.as_str())
                    && delivery["status"].as_str() == Some("acknowledged")
            });
        let response_delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| {
                message["kind"].as_str() == Some("provider_interaction_response")
                    && message["causation_id"].as_str() == Some(interaction_id.as_str())
            })
            .flat_map(|message| message["deliveries"].as_array().into_iter().flatten())
            .any(|delivery| {
                delivery["member_id"].as_str() == Some(member_id.as_str())
                    && delivery["status"].as_str() == Some("acknowledged")
                    && delivery["provider_receipt_id"].as_str().is_some()
            });
        idle_with_delivered_response = idle && request_acknowledged && response_delivered;
        if idle_with_delivered_response {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, diagnostic_snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        idle_with_delivered_response,
        "Codex did not consume the canonical interaction response and return idle; snapshot: {diagnostic_snapshot}"
    );
}
