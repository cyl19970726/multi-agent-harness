use super::*;

#[test]
fn interrupt_cancels_waiting_provider_message_before_kimi_prompt() {
    let home = TempHome::new("team-run-kimi-waiting-cancel");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.36.1"),
            ("FAKE_KIMI_ASK", "1"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Wait for Lead, then be interrupted",
            "members": [{"name": "kimi-waiting", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Exercise unanswered provider-question cancellation"}]
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
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut waiting_request_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let waiting = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("waiting")
            });
        waiting_request_id = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["kind"].as_str() == Some("provider_interaction_request")
                    && message["sender_runtime_id"].as_str() == Some(member_id.as_str())
            })
            .and_then(|message| message["id"].as_str().map(str::to_string));
        if waiting && waiting_request_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        waiting_request_id.is_some(),
        "Kimi never emitted a provider-interaction request message"
    );
    let waiting_request_id = waiting_request_id.expect("waiting request id");
    let (status, interrupted) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"reason": "cancel while waiting", "requested_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {interrupted}");
    let mut idle_with_cancelled_message = false;
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
        let cancelled = snapshot["canonical_message_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|delivery| {
                delivery["message_id"].as_str() == Some(waiting_request_id.as_str())
                    && delivery["status"].as_str() == Some("acknowledged")
            });
        idle_with_cancelled_message = idle && cancelled;
        if idle_with_cancelled_message {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle_with_cancelled_message,
        "interrupt did not cancel the waiting provider Message and return the Member to idle; snapshot={}",
        serve.get_json("/v1/snapshot").1
    );
}
