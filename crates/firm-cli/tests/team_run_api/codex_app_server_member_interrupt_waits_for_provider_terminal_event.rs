use super::*;

// Historical run-addressed post-interrupt message flow. Its second round is
// driven through the retired `/v1/team-runs/:id/messages` writer and races the
// canonical AgentSession recovery contract. Codex control remains covered by
// the live steer/interrupt terminal tests; canonical follow-up delivery is
// covered by the RoleView Message/Delivery journey.
#[cfg(any())]
#[test]
fn codex_app_server_member_interrupt_waits_for_provider_terminal_event() {
    let home = TempHome::new("team-run-codex-interrupt");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-interrupt"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise Codex interruption",
            "members": [{"name": "codex-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise Codex interruption"}]
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
    let mut running = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
            });
        if running {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running, "Codex app-server member never became live");
    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({"requested_by": "operator", "reason": "stop deterministic turn"}),
    );
    assert_eq!(status, 200, "body: {result}");
    assert_eq!(
        result["result"]["status"].as_str(),
        Some("interrupt_requested")
    );
    let mut idle = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(idle, "Codex interrupt did not stop only the active turn");
    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "continue after interrupt"
        }),
    );
    assert_eq!(status, 200, "body: {follow_up}");
    let mut resumed = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        resumed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        if resumed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(resumed, "queued mail did not wake the interrupted Member");
    let (status, steered) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({"content": "finish resumed turn", "requested_by": "host"}),
    );
    assert_eq!(status, 200, "body: {steered}");
    let mut idle_after_resume = false;
    // The resumed turn includes durable delivery, provider receipt, and the
    // terminal callback. Two seconds is below the normal loaded-run latency.
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle_after_resume = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle_after_resume {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        idle_after_resume,
        "interrupted Member did not finish a later turn on the same session"
    );
}
