use super::*;

#[test]
fn kimi_model_switch_uses_only_the_new_models_advertised_effort_controls() {
    let home = TempHome::new("team-run-kimi-qwen-controls");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let controls = home.base().join("kimi-qwen-controls");
    let controls_value = controls.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_CONTROL_MARKER", controls_value.as_str()),
            ("FAKE_KIMI_MODEL_SWITCH_NO_REFRESH", "1"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "20"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Switch from the fake K3-shaped default to Qwen",
            "members": [{"name": "qwen", "role": "implementer", "provider": "kimi", "model": "qwen/qwen3.8-max", "initial_work": "Run without a K3-only effort override"}]
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

    // Reproduce a pre-existing/resumed durable row whose old K3 model had
    // already made `max` effective. The Qwen model switch below intentionally
    // returns no refreshed thinking options, so every old-model projection
    // field must be cleared rather than surviving by omission.
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let initial_member = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("created member row");
    let mut stale_member = initial_member.clone();
    stale_member.provider_controls.reasoning_effort.effective = Some("max".to_string());
    stale_member.provider_controls.reasoning_effort.status =
        harness_core::ProviderControlStatus::Effective;
    stale_member.provider_controls.reasoning_effort.note =
        Some("acknowledged by the previous K3 model".to_string());
    store
        .compare_and_append_member_run(&initial_member, &stale_member)
        .expect("seed stale old-model control projection");

    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    let mut controlled = None;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        controlled = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["provider_controls"]["model"]["effective"].as_str()
                        == Some("qwen/qwen3.8-max")
            })
            .cloned();
        if controlled.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let controlled = controlled.expect("Qwen controls must become effective");
    assert!(controlled["provider_controls"]["reasoning_effort"]["requested"].is_null());
    assert!(
        controlled["provider_controls"]["reasoning_effort"]["effective"].is_null(),
        "without refreshed Qwen options, the old model's default is not evidence: {controlled}"
    );
    assert_eq!(
        controlled["provider_controls"]["reasoning_effort"]["status"].as_str(),
        Some("not_requested")
    );
    assert!(
        controlled["provider_controls"]["reasoning_effort"]["note"].is_null(),
        "the old model's receipt note must be cleared: {controlled}"
    );
    let calls = std::fs::read_to_string(&controls).expect("control marker");
    assert!(calls.contains("qwen/qwen3.8-max"), "{calls}");
    assert!(
        !calls.contains("\"configId\":\"thinking\""),
        "an omitted effort must not send the old model's override: {calls}"
    );

    // An explicitly requested K3-only value is not silently carried into the
    // Qwen turn either: refreshed model-specific options reject it before any
    // prompt, leaving an actionable failed ProviderRuntimeProjection.
    let rejected_home = TempHome::new("team-run-kimi-qwen-reject-k3-effort");
    let _project_id = init_project(&rejected_home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(rejected_home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = rejected_home.base().join("qwen-rejected-prompts");
    let prompts_value = prompts.display().to_string();
    let rejected = ServeHandle::spawn_with_env(
        &rejected_home,
        rejected_home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
        ],
    );
    let (_, created) = rejected.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Reject K3-only effort on Qwen",
            "members": [{"name": "qwen-bad-effort", "role": "implementer", "provider": "kimi", "model": "qwen/qwen3.8-max", "effort": "max", "initial_work": "Must fail before provider execution"}]
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
    let (status, started) = rejected.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");
    let mut failed = false;
    for _ in 0..300 {
        let (_, snapshot) = rejected.get_json("/v1/snapshot");
        failed = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("error")
                    && action["summary"].as_str().is_some_and(|summary| {
                        summary.contains("does not advertise requested reasoning effort `max`")
                    })
            });
        if failed {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(failed, "unsupported Qwen effort must fail before prompting");
    assert!(
        !prompts.exists(),
        "the invalid control set must not reach session/prompt"
    );
}
