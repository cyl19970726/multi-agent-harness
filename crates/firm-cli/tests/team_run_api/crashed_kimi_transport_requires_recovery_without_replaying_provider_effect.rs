use super::*;

#[test]
fn crashed_kimi_transport_requires_recovery_without_replaying_provider_effect() {
    let home = TempHome::new("team-run-kimi-crash-recovery");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let crash_once = home.base().join("kimi-crashed-once");
    let attach = home.base().join("kimi-attach.log");
    let prompts = home.base().join("kimi-recovery-prompts.jsonl");
    let crash_value = crash_once.display().to_string();
    let attach_value = attach.display().to_string();
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.36.1"),
            ("FAKE_KIMI_RESULT", "done"),
            ("FAKE_KIMI_CRASH_ONCE_MARKER", crash_value.as_str()),
            ("FAKE_KIMI_ATTACH_MARKER", attach_value.as_str()),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Recover a Kimi member after provider transport loss",
            "members": [{"name": "kimi-recover", "role": "implementer", "provider": "kimi", "initial_work": "Exercise Kimi recovery"}]
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
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut recovery_required = false;
    for _ in 0..400 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let member_blocked = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("blocked")
            });
        let recovery_action = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("runtime_recovery_required")
            });
        recovery_required = member_blocked && recovery_action;
        if recovery_required {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovery_required,
        "Kimi transport loss after provider receipt must stop at RecoveryRequired; snapshot={}; attach={:?}; prompts={:?}",
        serve.get_json("/v1/snapshot").1,
        std::fs::read_to_string(&attach),
        std::fs::read_to_string(&prompts),
    );
    let attach_log = std::fs::read_to_string(&attach).unwrap_or_default();
    assert!(
        !attach_log.lines().any(|line| line.starts_with("resume ")),
        "an uncertain provider effect must not auto-resume: {attach_log}"
    );
    assert!(
        !attach_log.lines().any(|line| line.starts_with("load ")),
        "an uncertain provider effect must not auto-load native history"
    );
    let prompt_log = std::fs::read_to_string(&prompts).expect("prompt log");
    assert_eq!(
        prompt_log.lines().count(),
        1,
        "the provider prompt must execute exactly once"
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let commands = store
        .runtime_commands(&current_space_id(&home))
        .expect("canonical RuntimeCommands");
    let dispatches = commands
        .iter()
        .filter(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
        })
        .collect::<Vec<_>>();
    assert_eq!(dispatches.len(), 1, "no provider replay is permitted");
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
        "the prompt receipt proves StartCycle even though the later cycle outcome needs recovery"
    );
    let delivery = store
        .fabric_work_deliveries(&current_space_id(&home))
        .expect("canonical WorkDelivery")
        .into_iter()
        .find(|delivery| delivery.work_id == work_id)
        .expect("accepted WorkDelivery");
    assert_eq!(
        delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
}
