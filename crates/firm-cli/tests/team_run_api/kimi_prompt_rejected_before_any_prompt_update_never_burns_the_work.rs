use super::*;

/// A prompt the provider rejects before any prompt-scoped update was never
/// accepted. A late session-level command-catalog notification must still be
/// processed, but publishing a receipt for it would burn the Work before the
/// provider accepted responsibility.
#[test]
fn kimi_prompt_rejected_before_any_prompt_update_never_burns_the_work() {
    let home = TempHome::new("team-run-kimi-reject-before-update");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let reject_once = home.base().join("kimi-reject-before-update-once");
    let reject_once_value = reject_once.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
            (
                "FAKE_KIMI_REJECT_BEFORE_UPDATE_MARKER",
                reject_once_value.as_str(),
            ),
            ("FAKE_KIMI_LATE_AVAILABLE_BEFORE_REJECT", "1"),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Kimi immediate rejection must not burn the Work",
            "members": [{"name": "kimi-reject", "role": "implementer", "provider": "kimi", "initial_work": "Exercise immediate rejection recovery"}]
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

    let mut rejected = false;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let provider_error = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("provider_error")
                    && action["status"].as_str() == Some("failed")
            });
        let handoffs = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| {
                message["sender_runtime_id"].as_str() == Some(member_id.as_str())
                    && message["kind"].as_str() == Some("handoff")
            })
            .count();
        assert_eq!(
            handoffs, 0,
            "a rejected prompt must never fabricate a handoff"
        );
        rejected = provider_error;
        if rejected {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        rejected,
        "an immediately rejected Kimi prompt must record a failed provider_error round"
    );
    assert!(reject_once.exists(), "the scripted rejection fired");

    // The core contract: no receipt was published for a turn the provider
    // never accepted. The canonical claim may be visible before provider
    // cleanup settles its negative acknowledgement, so wait for that durable
    // transition in the canonical delivery projection.
    let mut settled = None;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let delivery = snapshot["work_deliveries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|delivery| delivery["work_id"].as_str() == Some(work_id.as_str()))
            .cloned();
        if delivery
            .as_ref()
            .is_some_and(|delivery| delivery["status"].as_str() == Some("failed"))
        {
            settled = delivery;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let work_delivery = settled.expect("canonical Work delivery negative acknowledgement");
    assert_eq!(
        work_delivery["status"].as_str(),
        Some("failed"),
        "provider rejection must settle the exact canonical claim: {work_delivery}"
    );
    assert_eq!(
        work_delivery["attempt"].as_u64(),
        Some(1),
        "canonical delivery attempt identity must remain stable: {work_delivery}"
    );
    assert!(
        work_delivery["provider_receipt_id"].is_null(),
        "a rejected prompt must publish no provider receipt: {work_delivery}"
    );
    assert_eq!(work_delivery["authority"].as_str(), Some("canonical_trust"));
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let commands = store
        .runtime_commands(&current_space_id(&home))
        .expect("canonical RuntimeCommands");
    let dispatch = commands
        .iter()
        .find(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
                && command
                    .source_record_id
                    .as_deref()
                    .is_some_and(|source| source.contains(":turn:1"))
        })
        .expect("failed canonical provider dispatch");
    assert_eq!(
        dispatch.status,
        harness_core::agentfirm_api::RuntimeCommandStatus::Failed
    );
    assert_eq!(
        dispatch.effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::NotApplied
    );
    assert!(
        !snapshot["team_run_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|event| {
                event["entity_id"].as_str() == Some(work_id.as_str())
                    && event["summary"].as_str().is_some_and(|summary| {
                        summary.contains("accepted by provider")
                            || summary.contains("provider_received")
                    })
            }),
        "a rejected prompt must not journal accepted/provider_received: {}",
        snapshot["team_run_events"]
    );
}
