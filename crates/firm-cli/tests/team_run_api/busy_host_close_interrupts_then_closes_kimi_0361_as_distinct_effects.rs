use super::*;

#[test]
fn busy_host_close_interrupts_then_closes_kimi_0361_as_distinct_effects() {
    let home = TempHome::new("team-run-kimi-0361-close");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let cancel_marker = home.base().join("kimi-close-cancel-marker.log");
    let cancel_marker_value = cancel_marker.display().to_string();
    let close_marker = home.base().join("kimi-close-runtime-marker.log");
    let close_marker_value = close_marker.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.36.1"),
            ("FAKE_KIMI_WAIT", "1"),
            ("FAKE_KIMI_CANCEL_MARKER", cancel_marker_value.as_str()),
            ("FAKE_KIMI_CLOSE_MARKER", close_marker_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Interrupt one active Kimi cycle, then close its runtime",
            "members": [{"name": "kimi-close", "role": "observer", "provider": "kimi", "model": "k2.5", "initial_work": "Exercise Kimi close"}]
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
    let mut running = false;
    for _ in 0..500 {
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
    assert!(running, "Kimi 0.36.1 member never became live");

    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "lane accepted"}),
    );
    let (_, close_snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200, "body: {closed}; snapshot: {close_snapshot}");
    assert_eq!(closed["result"]["status"].as_str(), Some("closed"));
    assert_eq!(
        closed["result"]["provider_terminal_evidence"]["provider_terminal_event"].as_str(),
        Some("agent_settled"),
        "Close must expose the correlated current-cycle terminal evidence: {closed}"
    );
    assert!(
        closed["result"]["provider_terminal_evidence"]["member_runtime_close"]
            ["control_acknowledged"]
            .as_str()
            == Some("satisfied"),
        "Close must expose the independent provider runtime receipt: {closed}"
    );
    let mut stopped = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "Host close did not stop Kimi 0.36.1 runtime");
    assert!(
        cancel_marker.exists(),
        "busy Close must first interrupt the active Kimi cycle"
    );
    assert!(
        close_marker.exists(),
        "busy Close must then obtain a distinct Kimi session/close receipt"
    );
}
