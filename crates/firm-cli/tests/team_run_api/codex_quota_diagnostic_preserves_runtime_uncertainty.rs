use super::*;

#[test]
fn codex_quota_diagnostic_preserves_runtime_uncertainty_without_replay() {
    for thread_status in ["idle", "systemError"] {
        let home = TempHome::new(&format!("codex-quota-{thread_status}"));
        let project_id = init_project(&home, "alpha");
        let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin"));
        let path = format!(
            "{}:{}",
            fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let serve = ServeHandle::spawn_with_env(
            &home,
            home.base(),
            &[],
            &[
                ("PATH", path.as_str()),
                ("FAKE_CODEX_TERMINAL_QUOTA", "1"),
                ("FAKE_CODEX_FAILURE_THREAD_STATUS", thread_status),
            ],
        );
        let (_, created) = serve.post_json("/v1/team-runs", &serde_json::json!({
            "objective":"Preserve quota and runtime facts separately",
            "members":[{"name":"codex-quota","role":"implementer","provider":"codex","execution_mode":"codex_app_server","initial_work":"Exercise typed failure"}]
        }));
        let run_id = created["result"]["team_run"]["id"].as_str().unwrap();
        let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
            .as_str()
            .unwrap()
            .to_string();
        let work_id = created["result"]["works"][0]["id"].as_str().unwrap();
        assert_eq!(
            serve
                .post_json(
                    &format!("/v1/team-runs/{run_id}/start"),
                    &serde_json::json!({})
                )
                .0,
            202
        );
        let mut observed = None;
        for _ in 0..400 {
            let (_, snapshot) = serve.get_json("/v1/snapshot");
            let actions = snapshot["member_actions"].as_array().unwrap();
            let quota = actions.iter().any(|action| {
                action["member_run_id"] == member_id
                    && action["action_type"] == "provider_error"
                    && action["provider_status"] == "provider_terminal:usageLimitExceeded:-"
            });
            let recovery = actions.iter().any(|action| {
                action["member_run_id"] == member_id
                    && action["action_type"] == "runtime_recovery_required"
            });
            let member_state = snapshot["member_runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|member| member["id"] == member_id)
                .unwrap()["status"]
                .as_str()
                .unwrap();
            if quota && (thread_status == "idle" || (recovery && member_state == "blocked")) {
                observed = Some(snapshot);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let snapshot = observed.unwrap_or_else(|| {
            let (_, snapshot) = serve.get_json("/v1/snapshot");
            panic!(
                "typed quota missing/convergence failed: actions={}, members={}",
                snapshot["member_actions"], snapshot["member_runs"]
            );
        });
        let work = snapshot["works"]
            .as_array()
            .unwrap()
            .iter()
            .find(|work| work["id"] == work_id)
            .unwrap();
        assert_ne!(work["resolution"], "accepted", "quota does not accept Work");
        let store = HarnessStore::new(home.spaces_dir().join(&project_id));
        assert!(
            store
                .work_operations()
                .unwrap()
                .iter()
                .filter(|operation| operation.event.work_id == work_id)
                .all(|operation| !matches!(
                    operation.event.kind,
                    harness_core::WorkEventKind::Released
                        | harness_core::WorkEventKind::Accepted
                        | harness_core::WorkEventKind::ExecutionRecovered
                        | harness_core::WorkEventKind::Cancelled
                )),
            "quota cannot release, accept, recover, or cancel Work"
        );
        let starts: Vec<_> = store
            .runtime_commands(&current_space_id(&home))
            .unwrap()
            .into_iter()
            .filter(|command| {
                command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
            })
            .collect();
        assert_eq!(starts.len(), 1, "no StartCycle replay");
        assert_eq!(
            starts[0].effect_certainty,
            harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
        );
        assert_eq!(
            starts[0].postcondition_status,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
            "input acceptance is not rewritten by semantic failure or unknown idle"
        );
    }
}
