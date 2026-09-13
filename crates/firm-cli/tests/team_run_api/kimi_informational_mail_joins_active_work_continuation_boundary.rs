use super::*;

#[test]
fn kimi_informational_mail_joins_active_work_continuation_boundary() {
    let home = TempHome::new("kimi-continuation-message-boundary");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let first_ready = home.base().join("first-prompt-ready");
    let first_release = home.base().join("first-prompt-release");
    let prompts = home.base().join("kimi-prompts.jsonl");
    let first_ready_value = first_ready.display().to_string();
    let first_release_value = first_release.display().to_string();
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_KEEP_WORK_ACTIVE", "1"),
            ("FAKE_KIMI_EMPTY_TERMINAL", "1"),
            ("FAKE_KIMI_REAL_ON_PROMPT", "2"),
            ("FAKE_KIMI_FIRST_PROMPT_READY", first_ready_value.as_str()),
            (
                "FAKE_KIMI_FIRST_PROMPT_RELEASE",
                first_release_value.as_str(),
            ),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            // Production continuation is intentionally disabled when the
            // one-turn test retirement hook is set to any duration.
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", ""),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Continue active Work with queued conversation context",
            "members": [{
                "name": "kimi-continuation",
                "role": "implementer",
                "provider": "kimi",
                "initial_work": "Start this Work, then continue its newer version"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("MemberRun id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    wait_for_file(&first_ready, "first Kimi prompt boundary");

    // The first cycle input is already assembled. This informational message
    // must stay queued until Work start advances the version and independently
    // selects ActiveWorkContinuation.
    let (status, authored) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "response_intent": "informational",
            "body": "CONTINUATION_BOUNDARY_CONSTRAINT",
        }),
    );
    assert_eq!(status, 200, "body: {authored}");
    let message_id = authored["result"]["id"]
        .as_str()
        .expect("Message id")
        .to_string();
    assert_eq!(
        authored["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    std::fs::write(&first_release, b"release").expect("release first Kimi cycle");

    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let (delivery, prompt_log) = loop {
        let prompt_log = std::fs::read_to_string(&prompts).unwrap_or_default();
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivery = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(message_id.as_str()))
            .map(|message| message["deliveries"][0].clone());
        let two_inputs = prompt_log.lines().count() >= 2;
        let acknowledged = delivery
            .as_ref()
            .is_some_and(|delivery| delivery["status"].as_str() == Some("acknowledged"));
        if two_inputs && acknowledged {
            break (delivery.expect("acknowledged delivery"), prompt_log);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "continuation boundary did not converge; prompts={prompt_log}; snapshot={snapshot}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    let prompts = prompt_log.lines().collect::<Vec<_>>();
    assert_eq!(
        prompts.len(),
        2,
        "informational mail must not create a standalone provider turn: {prompt_log}"
    );
    assert!(
        !prompts[0].contains("CONTINUATION_BOUNDARY_CONSTRAINT"),
        "message arrived after the first input boundary: {}",
        prompts[0]
    );
    assert!(prompts[1].contains("ACTIVE WORK CONTINUATION"));
    assert!(prompts[1].contains("CONTINUATION_BOUNDARY_CONSTRAINT"));
    assert!(
        delivery["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")),
        "message ACK requires the continuation cycle's provider receipt: {delivery}"
    );
}
