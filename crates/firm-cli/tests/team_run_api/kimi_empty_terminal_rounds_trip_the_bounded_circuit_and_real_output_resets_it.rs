use super::*;

/// Load-independent round gate: the shim's own prompt marker is the round
/// counter, so each phase waits for round completions instead of a wall-clock
/// cadence. A phase fails with its name, the observed round count and the
/// elapsed time instead of a bare iteration timeout.
fn wait_for_prompt_count(
    prompts: &std::path::Path,
    expected: usize,
    phase: &str,
    started_at: std::time::Instant,
    budget: Duration,
) {
    loop {
        let count = std::fs::read_to_string(prompts)
            .map(|content| content.lines().count())
            .unwrap_or(0);
        if count >= expected {
            return;
        }
        assert!(
            started_at.elapsed() < budget,
            "phase '{phase}' exceeded its hard deadline: {:?} elapsed with {count}/{expected} provider rounds completed",
            started_at.elapsed()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

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

    // The circuit's own bound is 3 consecutive unproductive rounds
    // (WakePolicy::default, supervisor_wake.rs:81) per sequence, and this
    // test exercises two sequences: rounds 1-3 (round 3 is the productive
    // reset) and rounds 4-6. The per-round budget derives from that bound
    // with >10x headroom over the isolated per-round cadence (~1-2s): a phase
    // fails only when a single round stalls for 30s, which is a wedge, not
    // scheduling jitter. Rounds are observed through the shim's prompt
    // marker, never through wall-clock polling cadence.
    const PER_ROUND_BUDGET: Duration = Duration::from_secs(30);
    let started_at = std::time::Instant::now();
    wait_for_prompt_count(
        &prompts,
        3,
        "rounds 1-3: productive reset round",
        started_at,
        PER_ROUND_BUDGET * 3,
    );

    // Predicate-gated wake intentionally sleeps after round 3 produces a
    // real report without changing Work. One explicit Host nudge starts
    // the next empty sequence; the bounded zero-output probation then
    // drives rounds 5/6 to the circuit threshold without fixed polling.
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

    wait_for_prompt_count(
        &prompts,
        6,
        "rounds 4-6: post-reset empty probation",
        started_at,
        PER_ROUND_BUDGET * 6,
    );

    // The breaker audit action and ProviderRuntimeProjection transition live in
    // separate append-only ledgers, and both follow the sixth round. Require
    // the breaker action, the failed projection and all five empty-round
    // records to converge instead of treating a split read as a product
    // failure.
    let (stopped, last_snapshot) = loop {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let stopped = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("provider_circuit_breaker")
            })
            .cloned();
        let member_failed = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("failed")
            });
        let empty_rounds = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("empty_provider_round")
            })
            .count();
        let breaker_seen = stopped.is_some();
        if breaker_seen && member_failed && empty_rounds == 5 {
            break (stopped.expect("breaker action observed"), snapshot);
        }
        assert!(
            started_at.elapsed() < PER_ROUND_BUDGET * 6 + Duration::from_secs(60),
            "phase 'circuit convergence' exceeded its hard deadline: {:?} elapsed with breaker_action={} member_failed={} empty_rounds={}/5; prompts={:?}; snapshot={snapshot}",
            started_at.elapsed(),
            breaker_seen,
            member_failed,
            empty_rounds,
            std::fs::read_to_string(&prompts).ok()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let empty_round_actions = last_snapshot["member_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            action["member_run_id"].as_str() == Some(member_id.as_str())
                && action["action_type"].as_str() == Some("empty_provider_round")
        })
        .collect::<Vec<_>>();
    assert!(empty_round_actions
        .iter()
        .all(|action| action["status"].as_str() == Some("failed")));
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
