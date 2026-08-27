use super::*;

#[test]
fn kimi_empty_terminal_rounds_trip_the_bounded_circuit_and_real_output_resets_it() {
    let home = TempHome::new("team-run-kimi-empty-circuit");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = home.base().join("kimi-empty-prompts");
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_EMPTY_TERMINAL", "1"),
            ("FAKE_KIMI_KEEP_WORK_ACTIVE", "1"),
            // Round 3 produces a real report. The breaker must reset there,
            // then open only after rounds 4/5/6 are empty again.
            ("FAKE_KIMI_REAL_ON_PROMPT", "3"),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            // Disable the integration harness's default one-turn retirement;
            // this test intentionally exercises production continuation.
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", ""),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Bound repeated empty Kimi terminal rounds",
            "members": [{"name": "kimi-empty", "role": "implementer", "provider": "kimi", "initial_work": "Keep the Work active while the fake provider emits empty terminal rounds"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let accountable_member = member_run_for_work_owner(&created["result"], 0);
    let member_id = accountable_member["id"].as_str().unwrap().to_string();
    let agent_member_id = accountable_member["agent_member_id"]
        .as_str()
        .expect("accountable AgentMember")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut stopped = None;
    let mut last_snapshot = serde_json::Value::Null;
    let mut post_reset_nudge_sent = false;
    let mut circuit_converged = false;
    for _ in 0..500 {
        // Predicate-gated wake intentionally sleeps after round 3 produces a
        // real report without changing Work. One explicit Host nudge starts
        // the next empty sequence; the bounded zero-output probation then
        // drives rounds 5/6 to the circuit threshold without fixed polling.
        if !post_reset_nudge_sent
            && std::fs::read_to_string(&prompts)
                .ok()
                .is_some_and(|content| content.lines().count() >= 3)
        {
            let (status, nudge) = serve.post_json(
                &format!("/v1/team-runs/{run_id}/messages"),
                &serde_json::json!({
                    "sender_runtime_id": "host",
                    "recipient_runtime_ids": [member_id],
                    "kind": "message",
                    "response_intent": "response_required",
                    "body": "Continue the active lane after the productive reset round",
                }),
            );
            assert_eq!(status, 200, "body: {nudge}");
            post_reset_nudge_sent = true;
        }
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        last_snapshot = snapshot.clone();
        stopped = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("provider_circuit_breaker")
            })
            .cloned();
        if stopped.is_some() {
            let member_failed = snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("failed")
                });
            // The breaker audit action and ProviderRuntimeProjection transition live in
            // separate append-only ledgers. A snapshot may briefly observe
            // the action before the failed ProviderRuntimeProjection revision; require both
            // facts to converge instead of treating that split read as a
            // product failure.
            if !member_failed {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let empty_rounds = snapshot["member_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|action| {
                    action["member_run_id"].as_str() == Some(member_id.as_str())
                        && action["action_type"].as_str() == Some("empty_provider_round")
                })
                .collect::<Vec<_>>();
            assert_eq!(empty_rounds.len(), 5, "snapshot: {snapshot}");
            assert!(empty_rounds
                .iter()
                .all(|action| action["status"].as_str() == Some("failed")));
            circuit_converged = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = stopped.unwrap_or_else(|| {
        panic!(
            "repeated empty rounds must open the circuit; prompts={:?}; snapshot={last_snapshot}",
            std::fs::read_to_string(&prompts).ok()
        )
    });
    assert!(
        circuit_converged,
        "breaker action and failed ProviderRuntimeProjection did not converge: {last_snapshot}"
    );
    let summary = stopped["summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("3 consecutive unproductive rounds"),
        "{summary}"
    );
    assert!(summary.contains("empty terminal success"), "{summary}");
    assert!(summary.contains("capacity remains unknown"), "{summary}");

    let active_work = last_snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["owner_member_id"].as_str() == Some(agent_member_id.as_str()))
        .expect("the circuit must preserve the member's active Work");
    assert!(active_work["active_member_run_id"].is_null());
    assert_eq!(
        active_work["phase"].as_str(),
        Some("active"),
        "the provider circuit must not rewrite active Work: {active_work}"
    );
    let work_id = active_work["id"].as_str().expect("active Work id");
    // Confirm the active Work and provider receipt against the canonical
    // identity/session-bound fabric after the runtime has stopped. The
    // retired run-addressed WorkDelivery snapshot is intentionally excluded
    // from current runtime authority.
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let stored_work = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("stored active Work");
    assert_eq!(stored_work.phase, harness_core::WorkPhase::Active);
    let active_bindings = store
        .fabric_work_execution_bindings(&current_space_id(&home))
        .expect("canonical Work execution bindings")
        .into_iter()
        .filter(|binding| {
            binding.work_id == work_id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .collect::<Vec<_>>();
    let [active_binding] = active_bindings.as_slice() else {
        panic!("active Work must retain one exact execution binding: {active_bindings:?}");
    };
    assert_eq!(active_binding.agent_member_id, agent_member_id);
    let canonical_deliveries = store
        .fabric_work_deliveries(&current_space_id(&home))
        .expect("canonical Work deliveries");
    let stored_delivery = canonical_deliveries
        .into_iter()
        .find(|delivery| delivery.work_id == work_id)
        .expect("stored provider-received delivery");
    assert_eq!(
        stored_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(stored_delivery.attempt, 1);
    assert!(stored_delivery.provider_receipt_id.is_some());
    let applied_turns = store
        .runtime_commands(&current_space_id(&home))
        .expect("canonical RuntimeCommands")
        .into_iter()
        .filter(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
                && command.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                && command.effect_certainty
                    == harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
        })
        .count();
    assert_eq!(
        applied_turns, 6,
        "six distinct accepted turns must have six exact, settled RuntimeCommands"
    );

    // The fake process exits with the member. Six prompts prove the report on
    // round 3 reset the counter; without reset the circuit would stop at 3.
    std::thread::sleep(Duration::from_millis(50));
    let prompt_count = std::fs::read_to_string(&prompts)
        .expect("prompt marker")
        .lines()
        .count();
    assert_eq!(
        prompt_count, 6,
        "real output must reset the empty-round counter"
    );
}
