use super::*;

#[test]
fn codex_app_server_multi_question_fails_closed_without_interaction_rows() {
    let home = TempHome::new("team-run-codex-multi-question");
    let _project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-multi-question"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("PATH", path.as_str()), ("FAKE_CODEX_ASK", "multiple")],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Reject unsupported Codex multi-question input",
            "members": [{"name": "codex-multi-question", "role": "implementer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Emit two provider questions"}]
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

    let mut terminal = false;
    for _ in 0..150 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        terminal = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| {
                matches!(
                    member["status"].as_str(),
                    Some("idle" | "failed" | "stopped" | "completed")
                )
            });
        if terminal {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        terminal,
        "unsupported multi-question request did not quiesce"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|message| {
                message["kind"].as_str() != Some("provider_interaction_request")
                    || message["sender_runtime_id"].as_str() != Some(member_id.as_str())
            }),
        "unsupported multi-question request became a TeamMessageProjection"
    );
    assert!(
        snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|action| {
                action["member_run_id"].as_str() != Some(member_id.as_str())
                    || action["action_type"].as_str() != Some("provider_control")
            }),
        "unsupported multi-question request wrote a provider-control receipt"
    );
}
