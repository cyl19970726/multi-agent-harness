use super::*;

#[test]
fn kimi_quota_like_failure_requires_recovery_without_fabricating_capacity() {
    let home = TempHome::new("team-run-kimi-quota-circuit");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let prompts = home.base().join("kimi-quota-prompts");
    let prompts_value = prompts.display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_QUOTA_ERROR", "1"),
            ("FAKE_KIMI_KEEP_WORK_ACTIVE", "1"),
            ("FAKE_KIMI_PROMPT_MARKER", prompts_value.as_str()),
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", ""),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Bound repeated quota-like Kimi failures",
            "members": [{"name": "kimi-quota", "role": "implementer", "provider": "kimi", "initial_work": "Exercise quota-like provider failures"}]
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

    let mut final_snapshot = None;
    for _ in 0..500 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let recovery_required = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("runtime_recovery_required")
                    && action["status"].as_str() == Some("failed")
            });
        // `/v1/snapshot` folds the append-only ledgers independently. The
        // runtime persists the terminal ProviderRuntimeProjection revision before publishing
        // the circuit-breaker action, but a snapshot can begin reading the
        // member ledger before that revision and finish reading the action
        // ledger after the action is appended. Treat the transition as
        // observed only when both projections have converged in one snapshot;
        // waiting on the action alone permits a legitimate old-member/new-
        // action fractured read and makes this assertion timing-dependent.
        let member_blocked = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .is_some_and(|member| member["status"].as_str() == Some("blocked"));
        if recovery_required && member_blocked {
            final_snapshot = Some(snapshot);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let snapshot = final_snapshot.unwrap_or_else(|| {
        panic!(
            "quota-like failure must require reconciliation: {}",
            serve.get_json("/v1/snapshot").1
        )
    });
    let member = snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id.as_str()))
        .expect("member row");
    assert_eq!(member["status"].as_str(), Some("blocked"));
    assert_eq!(
        member["provider_capacity"]["state"].as_str(),
        Some("unknown")
    );
    assert!(member["provider_capacity"]["windows"]
        .as_array()
        .is_some_and(|windows| windows.is_empty()));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        std::fs::read_to_string(&prompts)
            .expect("prompt marker")
            .lines()
            .count(),
        1,
        "an uncertain quota-like provider effect must not be replayed"
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
    assert_eq!(dispatches.len(), 1);
    assert_eq!(
        dispatches[0].status,
        harness_core::agentfirm_api::RuntimeCommandStatus::Applied
    );
    assert_eq!(
        dispatches[0].postcondition_status,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
        "quota-like terminal failure does not erase the earlier StartCycle receipt"
    );
}
