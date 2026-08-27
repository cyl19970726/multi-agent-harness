use super::*;

#[test]
fn idle_kimi_member_consumes_late_mail_on_the_same_native_session() {
    let home = TempHome::new("team-run-kimi-late-mail");
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
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise persistent Kimi mailbox",
            "members": [{"name": "kimi-idle", "role": "implementer", "provider": "kimi", "initial_work": "Exercise persistent Kimi mailbox"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);
    let mut first_session = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        first_session = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            })
            .and_then(|member| {
                member["native_session"]["native_session_id"]
                    .as_str()
                    .map(str::to_string)
            });
        let first_round_completed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        if first_session.is_some() && first_round_completed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let first_session = first_session.expect("Kimi idle native session");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "late Kimi follow-up",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    assert_eq!(
        sent["result"]["response_intent"].as_str(),
        Some("response_required"),
        "canonical Host follow-up makes the sender-aware default explicit: {sent}"
    );
    let message_id = sent["result"]["id"].as_str().unwrap().to_string();

    let mut second_round = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivered = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(message_id.as_str()))
            .is_some_and(|message| {
                message["deliveries"][0]["status"].as_str() == Some("acknowledged")
                    && message["deliveries"][0]["attempt"].as_u64() == Some(1)
            });
        let same_session = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(first_session.as_str())
            });
        let completed_rounds = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count();
        second_round = delivered && same_session && completed_rounds >= 2;
        if second_round {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        second_round,
        "late Kimi mail was not delivered exactly once on the same native session"
    );

    // DEV-21: the fresh-start settle wrote the native Session through
    // save_member_run; the trust selector + exact-binding layers must carry it.
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    assert_trust_native_binding_synced(&store, &member_id, &first_session);
}
