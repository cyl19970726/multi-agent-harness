use super::*;

#[test]
fn close_cancels_kimi_provider_request_without_resuming_member() {
    let home = TempHome::new("team-run-kimi-waiting-close");
    let project_id = init_project(&home, "alpha");
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
            "objective": "Close a member blocked on provider input",
            "members": [{"name": "kimi-close-waiting", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Wait for provider input, then close"}]
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

    let mut request_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        request_id = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| {
                message["kind"].as_str() == Some("provider_interaction_request")
                    && message["sender_runtime_id"].as_str() == Some(member_id.as_str())
            })
            .and_then(|message| message["id"].as_str().map(str::to_string));
        let waiting = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("waiting")
            });
        if waiting && request_id.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let request_id = request_id.unwrap_or_else(|| {
        panic!(
            "Kimi provider request before close; snapshot={}",
            serve.get_json("/v1/snapshot").1
        )
    });

    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"reason": "close while waiting", "requested_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {closed}");

    let mut terminal_acknowledged = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let terminal = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                member["coordination_status"].as_str() == Some("closed")
                    && member["status"].as_str() == Some("stopped")
            });
        let acknowledged = snapshot["canonical_message_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|delivery| {
                delivery["message_id"].as_str() == Some(request_id.as_str())
                    && delivery["status"].as_str() == Some("acknowledged")
            });
        terminal_acknowledged = terminal && acknowledged;
        if terminal_acknowledged {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        terminal_acknowledged,
        "close did not wake the reverse request and terminate the member; snapshot={}",
        serve.get_json("/v1/snapshot").1
    );
    // Let a delayed provider callback run if one still exists; it must not
    // write Running/Idle over the terminal close.
    std::thread::sleep(Duration::from_millis(100));
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let latest = snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .expect("closed member remains visible");
    assert_eq!(latest["coordination_status"].as_str(), Some("closed"));
    assert_eq!(latest["status"].as_str(), Some("stopped"));
    let agent_member_id = latest["agent_member_id"]
        .as_str()
        .expect("member carries canonical AgentIdentity")
        .to_string();
    let session = snapshot["agent_sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|session| session["agent_member_id"].as_str() == Some(agent_member_id.as_str()))
        .expect("Team close preserves the machine-owned AgentSession");
    assert_eq!(
        session["lifecycle"].as_str(),
        Some("idle"),
        "Team close quiesces only its provider turn"
    );
    let released_binding = snapshot["work_execution_bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|binding| {
            binding["agent_member_id"].as_str() == Some(agent_member_id.as_str())
                && binding["status"].as_str() == Some("released")
        })
        .expect("exact applied Close releases the old-generation WorkExecutionBinding");
    let released_binding_id = released_binding["id"]
        .as_str()
        .expect("released binding has canonical identity");
    assert!(
        snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|delivery| {
                delivery["work_execution_binding_id"].as_str() == Some(released_binding_id)
                    && delivery["status"].as_str() == Some("provider_received")
            }),
        "Close preserves the immutable ProviderReceived delivery evidence"
    );
    let close_requests = snapshot["team_member_close_requests"]
        .as_array()
        .expect("close requests");
    assert_eq!(
        close_requests
            .iter()
            .filter(|request| request["member_run_id"].as_str() == Some(member_id.as_str()))
            .count(),
        1,
        "the close has one latest durable request"
    );
    assert_eq!(
        close_requests
            .iter()
            .find(|request| request["member_run_id"].as_str() == Some(member_id.as_str()))
            .and_then(|request| request["status"].as_str()),
        Some("applied")
    );

    let store_root = home.firm_home().join("execution-spaces").join(project_id);
    let ledgers = [
        "team_member_close_requests.jsonl",
        "team_messages.jsonl",
        "member_runs.jsonl",
        "member_actions.jsonl",
        "team_run_events.jsonl",
        "canonical_operations.jsonl",
    ];
    let ledger_bytes = |name: &str| std::fs::read(store_root.join(name)).unwrap_or_default();
    let before_replay = ledgers
        .iter()
        .map(|name| ledger_bytes(name))
        .collect::<Vec<_>>();
    let (replay_status, replay) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"reason": "close while waiting", "requested_by": "operator"}),
    );
    assert_eq!(replay_status, 200, "body: {replay}");
    assert_eq!(replay["result"]["idempotent"].as_bool(), Some(true));
    let after_replay = ledgers
        .iter()
        .map(|name| ledger_bytes(name))
        .collect::<Vec<_>>();
    assert_eq!(
        after_replay, before_replay,
        "close replay must not repeat cancellation, lifecycle, event, or receipt effects"
    );
}
