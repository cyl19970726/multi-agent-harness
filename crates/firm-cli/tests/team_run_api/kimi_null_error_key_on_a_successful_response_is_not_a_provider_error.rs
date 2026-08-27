use super::*;

/// JSON-RPC servers that serialize every field return `"error": null` on
/// success. `frame.get("error").is_some()` is true for that key, so a naive
/// check turns every successful round into a provider failure and loses the
/// member's entire output.
#[test]
fn kimi_null_error_key_on_a_successful_response_is_not_a_provider_error() {
    let home = TempHome::new("team-run-kimi-null-error-key");
    let _project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_NULL_ERROR_KEY", "1"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Null error key is still a successful round",
            "members": [{"name": "kimi-null-error", "role": "implementer", "provider": "kimi", "initial_work": "Exercise null error response"}]
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
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut completed_action = None;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        assert!(
            !snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|action| {
                    action["member_run_id"].as_str() == Some(member_id.as_str())
                        && action["action_type"].as_str() == Some("provider_error")
                }),
            "`error: null` is an empty key, not a provider failure"
        );
        completed_action = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
                    && action["status"].as_str() == Some("succeeded")
            })
            .cloned();
        if completed_action.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let completed_action = completed_action
        .expect("a successful round carrying `error: null` must record a successful provider turn");
    let durable_summary = completed_action["summary"].as_str().unwrap_or_default();
    assert!(
        durable_summary.contains("transcript remains provider-native"),
        "the durable action must be a coordination fact: {completed_action}"
    );
    assert!(
        !durable_summary.contains("fake member finished round"),
        "provider-authored response text must not be copied into MemberAction: {completed_action}"
    );
    assert_eq!(
        completed_action["evidence_refs"],
        serde_json::json!([]),
        "provider output must not fabricate Harness Evidence refs"
    );
}
