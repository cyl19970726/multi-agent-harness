use super::*;

// Provider-neutral Codex Close/Reopen journey. Close is acknowledged only
// after the active turn is terminal, the owned runtime is reaped, and the
// retained native thread locator is durably visible. Reopen must bind a
// higher runtime generation to that exact thread.
#[test]
fn host_can_explicitly_close_a_live_codex_member() {
    let home = TempHome::new("team-run-codex-close");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-close"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let resume_marker = home.base().join("codex-close-resume.log");
    let resume_marker_value = resume_marker.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_RESUME_MARKER", resume_marker_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise explicit Host close",
            "members": [{"name": "codex-close", "role": "observer", "provider": "codex", "initial_work": "Exercise Codex close"}]
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
    let mut native_session_id = None;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        running = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                native_session_id = member["native_session"]["native_session_id"]
                    .as_str()
                    .map(str::to_string);
                member["status"].as_str() == Some("running") && native_session_id.is_some()
            });
        if running {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(running, "Codex member never became live");
    let native_session_id = native_session_id.expect("Codex native session before close");

    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "lane accepted"}),
    );
    assert_eq!(status, 200, "body: {result}");
    assert_eq!(result["result"]["status"].as_str(), Some("closed"));
    assert_eq!(
        result["result"]["provider_terminal_evidence"]["member_runtime_close"]
            ["control_acknowledged"]
            .as_str(),
        Some("satisfied"),
        "Close must expose the independent Codex runtime receipt: {result}"
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
                    && member["coordination_status"].as_str() == Some("closed")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, close_diagnostic) = serve.get_json("/v1/snapshot");
    assert!(
        stopped,
        "Codex member did not terminate after Host close; snapshot: {close_diagnostic}"
    );

    let (status, reopened) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({"reopened_by": "host", "reason": "continue same conversation"}),
    );
    assert_eq!(status, 202, "body: {reopened}");
    assert_eq!(
        reopened["result"]["reopen"]["member_run"]["id"].as_str(),
        Some(member_id.as_str())
    );
    assert_eq!(
        reopened["result"]["reopen"]["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["result"]["reopen"]["member_run"]["native_session"]["native_session_id"].as_str(),
        Some(native_session_id.as_str())
    );

    let mut resumed = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        resumed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                matches!(member["status"].as_str(), Some("running" | "idle"))
                    && member["coordination_status"].as_str() == Some("active")
                    && member["runtime_generation"].as_u64() == Some(2)
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(native_session_id.as_str())
            });
        if resumed && resume_marker.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(resumed, "reopened Codex member did not run generation 2");
    let resume_log = std::fs::read_to_string(&resume_marker).expect("Codex resume marker");
    assert!(
        resume_log.contains(&native_session_id),
        "reopen did not call thread/resume with the preserved session: {resume_log}"
    );

    let (status, result) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "reopen acceptance complete"}),
    );
    assert_eq!(status, 200, "body: {result}");
}
