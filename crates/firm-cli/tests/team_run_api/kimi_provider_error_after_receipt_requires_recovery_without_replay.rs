use super::*;

#[test]
fn kimi_provider_error_after_receipt_requires_recovery_without_replay() {
    let home = TempHome::new("team-run-kimi-provider-error");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let error_once = home.base().join("kimi-prompt-error-once");
    let error_once_value = error_once.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            (
                "FAKE_KIMI_PROMPT_ERROR_ONCE_MARKER",
                error_once_value.as_str(),
            ),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Kimi provider failure parity",
            "members": [{"name": "kimi-fail", "role": "implementer", "provider": "kimi", "initial_work": "Exercise provider failure parity"}]
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

    let mut recovery_required = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let messages = snapshot["team_messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let handoffs = messages
            .iter()
            .filter(|message| {
                message["sender_runtime_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .count();
        let recovery_action = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("runtime_recovery_required")
                    && action["status"].as_str() == Some("failed")
            });
        let blocked = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("blocked")
            });
        assert_eq!(
            handoffs, 0,
            "a provider-failed turn must never fabricate a handoff"
        );
        recovery_required = recovery_action && blocked;
        if recovery_required {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovery_required,
        "a provider failure after prompt acceptance must stop at RecoveryRequired"
    );
    assert!(error_once.exists(), "the scripted provider error fired");
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let dispatches = store
        .runtime_commands(&current_space_id(&home))
        .expect("canonical RuntimeCommands")
        .into_iter()
        .filter(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
        })
        .collect::<Vec<_>>();
    assert_eq!(dispatches.len(), 1, "the failed effect must not replay");
    assert_eq!(
        dispatches[0].status,
        harness_core::agentfirm_api::RuntimeCommandStatus::Applied
    );
    assert_eq!(
        dispatches[0].effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
    );
    assert_eq!(
        dispatches[0].postcondition_status,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
        "the prompt receipt proves StartCycle independently of terminal provider failure"
    );
}
