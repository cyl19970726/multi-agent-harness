use super::*;

#[test]
fn busy_kimi_member_batches_mail_in_order_and_withholds_stale_handoff() {
    let home = TempHome::new("team-run-kimi-busy-mail");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let ready = home.base().join("kimi-first-prompt-ready");
    let release = home.base().join("kimi-first-prompt-release");
    let prompts = home.base().join("kimi-prompts.jsonl");
    let ready_value = ready.display().to_string();
    let release_value = release.display().to_string();
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_FIRST_PROMPT_READY", ready_value.as_str()),
            ("FAKE_KIMI_FIRST_PROMPT_RELEASE", release_value.as_str()),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Kimi safe-boundary batching",
            "members": [{"name": "kimi-busy", "role": "implementer", "provider": "kimi", "initial_work": "Exercise safe-boundary batching"}]
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
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    wait_for_file(&ready, "first Kimi prompt to enter busy state");

    let (status, first) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "BUSY_CORRECTION_ONE",
        }),
    );
    assert_eq!(status, 200, "body: {first}");
    let first_id = first["result"]["id"].as_str().unwrap().to_string();
    let correlation = first["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, second) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "BUSY_CORRECTION_TWO",
            "correlation_id": correlation,
            "causation_id": first_id,
        }),
    );
    assert_eq!(status, 200, "body: {second}");
    let second_id = second["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        first["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    assert_eq!(
        second["result"]["deliveries"][0]["status"].as_str(),
        Some("queued")
    );
    std::fs::write(&release, b"release").expect("release first Kimi prompt");

    let mut accepted = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"].as_array().unwrap();
        let delivery = |id: &str| {
            messages
                .iter()
                .find(|message| message["id"].as_str() == Some(id))
                .map(|message| &message["deliveries"][0])
        };
        let first_delivery = delivery(&first_id);
        let second_delivery = delivery(&second_id);
        let receipts_match =
            first_delivery
                .zip(second_delivery)
                .is_some_and(|(first_delivery, second_delivery)| {
                    first_delivery["status"].as_str() == Some("acknowledged")
                        && second_delivery["status"].as_str() == Some("acknowledged")
                        && first_delivery["attempt"].as_u64() == Some(1)
                        && second_delivery["attempt"].as_u64() == Some(1)
                        && first_delivery["provider_receipt_id"].as_str()
                            == second_delivery["provider_receipt_id"].as_str()
                });
        accepted = receipts_match
            && snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|action| {
                    action["member_run_id"].as_str() == Some(member_id.as_str())
                        && action["action_type"].as_str() == Some("turn_completed")
                });
        if accepted {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        accepted,
        "Kimi busy-turn messages were not batched exactly once"
    );
    let prompt_log = std::fs::read_to_string(&prompts).expect("Kimi prompt log");
    let first_position = prompt_log
        .find("BUSY_CORRECTION_ONE")
        .expect("first correction in provider prompt");
    let second_position = prompt_log
        .find("BUSY_CORRECTION_TWO")
        .expect("second correction in provider prompt");
    assert!(
        first_position < second_position,
        "safe-boundary mail order changed"
    );
}
