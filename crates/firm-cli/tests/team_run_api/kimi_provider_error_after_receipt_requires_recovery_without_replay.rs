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
            // Keep the test-only Supervisor alive across slow, cold CI
            // runners. ServeHandle still terminates both child processes on
            // drop, so this does not extend test teardown or production TTLs.
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "180000"),
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
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let initial_supervisor_deadline = std::time::Instant::now() + Duration::from_secs(10);
    let initial_supervisor = loop {
        if let Some(lease) = store
            .latest_team_supervisor_lease(&run_id)
            .expect("initial Supervisor lease")
            .filter(|lease| {
                lease.status == harness_core::TeamSupervisorLeaseStatus::Active
                    && lease.expires_unix_ms > current_unix_ms()
            })
        {
            break lease;
        }
        assert!(
            std::time::Instant::now() < initial_supervisor_deadline,
            "initial Supervisor authority did not become current"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

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

    let (_, before_recovery) = serve.get_json("/v1/snapshot");
    let blocked_member = before_recovery["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .expect("blocked member projection");
    let native_session_id = blocked_member["native_session"]["native_session_id"]
        .as_str()
        .expect("provider-native session id")
        .to_string();
    let initial_runtime_generation = blocked_member["runtime_generation"]
        .as_u64()
        .expect("runtime generation");
    let work_id = before_recovery["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["active_member_run_id"].as_str() == Some(member_id.as_str()))
        .and_then(|work| work["id"].as_str())
        .expect("member Work")
        .to_string();

    // Reproduce the exact probation edge: generic idle wake would Continue
    // active Work from a nonzero streak unless recovery atomically consumes
    // that continuation authority together with the provider receipt fence.
    let blocked_row = store
        .member_runs()
        .expect("member rows before recovery")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("blocked member row");
    let mut probation_blocked = blocked_row.clone();
    probation_blocked.zero_output_streak = 2;
    probation_blocked.last_event_at = Some("unix-ms:recovery-probation".into());
    store
        .compare_and_append_member_run(&blocked_row, &probation_blocked)
        .expect("seed nonzero probation continuation streak");
    let blocked_sessions = store
        .fabric_agent_sessions(&current_space_id(&home))
        .expect("blocked Member AgentSessions")
        .into_iter()
        .filter(|session| {
            session.agent_member_id == blocked_row.agent_member_id
                && session
                    .native_session_ref
                    .as_ref()
                    .is_some_and(|native| native.native_session_id == native_session_id)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        blocked_sessions.len(),
        1,
        "the blocked runtime must resolve to one exact AgentSession"
    );
    let blocked_session_id = blocked_sessions[0].id.clone();

    let exact_current_session_bound = |expected_residency| {
        let Some(lease) = store
            .latest_team_supervisor_lease(&run_id)
            .expect("current Supervisor lease during recovery")
        else {
            return false;
        };
        lease.status == harness_core::TeamSupervisorLeaseStatus::Active
            && lease.expires_unix_ms > current_unix_ms().saturating_add(1_000)
            && (lease.supervisor_id != initial_supervisor.supervisor_id
                || lease.generation != initial_supervisor.generation)
            && store
                .fabric_agent_sessions(&current_space_id(&home))
                .expect("AgentSessions during recovery")
                .into_iter()
                .any(|session| {
                    session.id == blocked_session_id
                        && session.agent_member_id == blocked_row.agent_member_id
                        && session.execution_space_id == lease.execution_space_id
                        && session.node_id == lease.node_id
                        && session.node_daemon_id == lease.node_daemon_id
                        && session.node_daemon_generation == lease.node_daemon_generation
                        && session.control_state.runtime_residency == expected_residency
                        && session
                            .native_session_ref
                            .as_ref()
                            .is_some_and(|native| native.native_session_id == native_session_id)
                        && matches!(
                            &session.control_state.driver_ref,
                            harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
                                team_run_id,
                                team_supervisor_id,
                                team_supervisor_generation,
                            } if team_run_id == &run_id
                                && team_supervisor_id == &lease.supervisor_id
                                && *team_supervisor_generation == lease.generation
                        )
                })
    };
    let close_deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if exact_current_session_bound(harness_core::agentfirm_api::RuntimeResidency::Detached) {
            break;
        }
        assert!(
            std::time::Instant::now() < close_deadline,
            "NodeDaemon did not bind the detached blocked Member to the exact current Supervisor before recovery Close"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({
            "requested_by": "host",
            "reason": "explicitly close the detached failed runtime generation"
        }),
    );
    assert_eq!(status, 200, "detached recovery Close: {closed}");
    assert_eq!(
        closed["result"]["runtime_effect"], "already_detached",
        "recovery Close must not fabricate a provider Close receipt: {closed}"
    );
    assert_eq!(closed["result"]["provider_close_receipt"], "not_fabricated");
    let closed_row = store
        .member_runs()
        .expect("member rows after recovery Close")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("closed member row");
    assert_eq!(closed_row.zero_output_streak, 0);

    let (status, reopened) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({
            "reopened_by": "host",
            "reason": "resume the same native session after explicit recovery"
        }),
    );
    assert_eq!(status, 202, "same-session Reopen: {reopened}");

    // Reopen itself is not new provider input. Wait only for the durable new
    // MemberRun generation while continuously proving that the provider-
    // received Work is not injected again. The explicit Host Message below is
    // what may start a new Supervisor and attach the same native Session.
    let reopen_deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let start_cycles = store
            .runtime_commands(&current_space_id(&home))
            .expect("RuntimeCommands while awaiting Reopen authority")
            .into_iter()
            .filter(|command| {
                command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
            })
            .count();
        assert_eq!(
            start_cycles, 1,
            "Reopen must not replay the provider-received Work"
        );
        let reopened_generation = store
            .member_runs()
            .expect("MemberRuns while awaiting Reopen authority")
            .into_iter()
            .rev()
            .find(|member| member.id == member_id)
            .is_some_and(|member| {
                member.runtime_generation == initial_runtime_generation + 1
                    && member
                        .native_session
                        .as_ref()
                        .is_some_and(|native| native.native_session_id == native_session_id)
            });
        if reopened_generation {
            break;
        }
        assert!(
            std::time::Instant::now() < reopen_deadline,
            "Reopen did not preserve the same native Session on the new MemberRun generation"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let commands_after_reopen = store
        .runtime_commands(&current_space_id(&home))
        .expect("RuntimeCommands after Reopen");
    assert_eq!(
        commands_after_reopen
            .iter()
            .filter(|command| {
                command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
            })
            .count(),
        1,
        "Reopen must not replay the provider-received Work"
    );
    let deliveries_after_reopen = store
        .fabric_work_deliveries(&current_space_id(&home))
        .expect("WorkDeliveries after Reopen");
    assert_eq!(deliveries_after_reopen.len(), 1);
    assert_eq!(
        deliveries_after_reopen[0].status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );

    let (_, reopened_snapshot) = serve.get_json("/v1/snapshot");
    let reopened_member = reopened_snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .expect("reopened member projection");
    assert_eq!(
        reopened_member["runtime_generation"].as_u64(),
        Some(initial_runtime_generation + 1)
    );
    assert_eq!(
        reopened_member["native_session"]["native_session_id"].as_str(),
        Some(native_session_id.as_str())
    );

    let (status, follow_up) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "work_id": work_id,
            "body": "new explicit recovery-cycle input"
        }),
    );
    assert_eq!(status, 200, "new Host input: {follow_up}");
    let follow_up_id = follow_up["result"]["id"]
        .as_str()
        .expect("follow-up message id")
        .to_string();

    let mut resumed_once = false;
    let follow_up_deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < follow_up_deadline {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let acknowledged = snapshot["team_messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|message| message["id"].as_str() == Some(follow_up_id.as_str()))
            .is_some_and(|message| {
                message["deliveries"][0]["status"].as_str() == Some("acknowledged")
            });
        let same_session_idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(native_session_id.as_str())
                    && member["runtime_generation"].as_u64() == Some(initial_runtime_generation + 1)
                    && member["status"].as_str() == Some("idle")
            });
        let start_cycles = store
            .runtime_commands(&current_space_id(&home))
            .expect("RuntimeCommands during follow-up")
            .into_iter()
            .filter(|command| {
                command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
            })
            .count();
        resumed_once = acknowledged
            && same_session_idle
            && start_cycles == 2
            && exact_current_session_bound(harness_core::agentfirm_api::RuntimeResidency::Attached);
        if resumed_once {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        resumed_once,
        "only the new Host input should start one same-session recovery cycle"
    );
}
