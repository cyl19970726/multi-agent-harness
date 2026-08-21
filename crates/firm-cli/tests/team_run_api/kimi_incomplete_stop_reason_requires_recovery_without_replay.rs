use super::*;

/// `max_tokens`, `refusal`, and `max_turn_requests` all stop an already-started
/// turn without proving whether provider-side effects completed. They must
/// enter RecoveryRequired, never be recorded as success or auto-replayed.
#[test]
fn kimi_incomplete_stop_reason_requires_recovery_without_replay() {
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
            let action_requires_recovery = actions.iter().any(|action| {
                action["action_type"].as_str() == Some("runtime_recovery_required")
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
            recovery_required = action_requires_recovery && blocked;
            if recovery_required {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            recovery_required,
            "stopReason {stop_reason} must stop at RecoveryRequired"
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
            dispatches[0].status,
            harness_core::agentfirm_api::RuntimeCommandStatus::Applied
        );
        assert_eq!(
            dispatches[0].postcondition_status,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
            "the accepted StartCycle is distinct from the incomplete terminal outcome"
        );
    }
}
