use super::*;

#[test]
fn codex_provider_reported_interruption_is_not_attributed_to_harness() {
    let home = TempHome::new("team-run-codex-provider-interrupt");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(
        &home.base().join("fakebin-codex-provider-interrupt"),
    );
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_INTERRUPT_WITHOUT_REQUEST", "1"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise provider-reported interruption",
            "members": [{"name": "codex-provider-stop", "role": "observer", "provider": "codex", "execution_mode": "codex_app_server", "initial_work": "Exercise provider interruption"}]
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

    let mut final_snapshot = serde_json::Value::Null;
    let mut interruption_summary = String::new();
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
        interruption_summary = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
            .filter_map(|action| action["summary"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if idle && interruption_summary.contains("without a Harness control request") {
            final_snapshot = snapshot;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(
        final_snapshot,
        serde_json::Value::Null,
        "provider-reported interruption did not return the member to idle with honest attribution"
    );
    assert!(
        interruption_summary.contains("without a Harness control request"),
        "missing honest provider interruption attribution: {interruption_summary}"
    );
    assert!(
        !interruption_summary.contains("operator or Lead interrupted"),
        "provider interruption was falsely attributed to Harness: {interruption_summary}"
    );
}
