use super::*;

/// Known accepted stop reasons fail the semantic round, while their exact
/// terminal response leaves the runtime idle. Input acceptance is immutable.
#[test]
fn kimi_known_incomplete_stop_reason_is_failed_idle_without_replay() {
    for stop_reason in ["max_tokens", "refusal", "max_turn_requests"] {
        let home = TempHome::new(&format!("team-run-kimi-stop-{stop_reason}"));
        let project_id = init_project(&home, "alpha");
        let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
        let fake_kimi = fake_bin.join("kimi").display().to_string();
        let serve = ServeHandle::spawn_with_env(
            &home,
            home.base(),
            &[],
            &[
                ("KIMI_CODE_BIN", fake_kimi.as_str()),
                ("FAKE_KIMI_RESULT", "done"),
                ("FAKE_KIMI_STOP_REASON", stop_reason),
                ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "30000"),
            ],
        );
        let (_, created) = serve.post_json(
            "/v1/team-runs",
            &serde_json::json!({
                "objective": format!("Kimi {stop_reason} must not read as success"),
                "members": [{"name": "kimi-stop", "role": "implementer", "provider": "kimi", "initial_work": "Exercise incomplete stop reason"}]
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

        let mut failed_idle = false;
        for _ in 0..300 {
            let (_, snapshot) = serve.get_json("/v1/snapshot");
            let actions: Vec<&serde_json::Value> = snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|action| action["member_run_id"].as_str() == Some(member_id.as_str()))
                .collect();
            assert!(
                !actions.iter().any(|action| {
                    action["action_type"].as_str() == Some("turn_completed")
                        && action["status"].as_str() == Some("succeeded")
                }),
                "{stop_reason} must never be recorded as a succeeded completion"
            );
            let handoffs = snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| {
                    message["sender_runtime_id"].as_str() == Some(member_id.as_str())
                        && message["kind"].as_str() == Some("handoff")
                })
                .count();
            assert_eq!(handoffs, 0, "{stop_reason} must never fabricate a handoff");
            let failed_round = actions.iter().any(|action| {
                action["action_type"].as_str() == Some("provider_error")
                    && action["status"].as_str() == Some("failed")
                    && action["provider_status"]
                        .as_str()
                        .and_then(harness_runtime_contract::ProviderTerminalFailure::parse)
                        .is_some_and(|failure| failure.reason == stop_reason)
            });
            let idle = snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("idle")
                });
            failed_idle = failed_round && idle;
            if failed_idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            failed_idle,
            "stopReason {stop_reason} must remain failed and Idle"
        );
        let store = HarnessStore::new(home.spaces_dir().join(&project_id));
        let dispatches = store
            .runtime_commands(&current_space_id(&home))
            .expect("canonical RuntimeCommands")
            .into_iter()
            .filter(|command| {
                command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
            })
            .collect::<Vec<_>>();
        assert_eq!(dispatches.len(), 1, "{stop_reason} must not replay");
        assert_eq!(
            dispatches[0].phase,
            harness_core::agentfirm_api::RuntimeCommandPhase::Settled
        );
        assert_eq!(
            dispatches[0].postcondition_status,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
            "the accepted StartCycle is distinct from the incomplete terminal outcome"
        );
    }
}
