use super::*;

#[test]
fn reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session() {
    let home = TempHome::new("team-run-reviewed-stable-id-recovery");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let acp_marker = home.base().join("reviewed-recovery-acp-started.log");
    let acp_marker_value = acp_marker.display().to_string();

    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Recover one durable member generation without minting identities",
            "--member",
            "recoverer:builder:kimi",
            "--json",
        ],
    );
    let run_id = created["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));

    // Team provenance is required at TeamRun creation after the clean cutover.
    let linked_run = store
        .team_runs()
        .expect("TeamRun rows")
        .into_iter()
        .rev()
        .find(|run| run.id == run_id)
        .expect("TeamRun");
    assert!(!linked_run.agent_team_id.is_empty());
    let work = member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--as-member-run-id",
            &member_id,
            "--owner-member-run-id",
            &member_id,
            "--work-id",
            "work-stable-recovery",
            "--title",
            "Preserve stable recovery provenance",
            "--completion-criteria",
            "One rebound revision and one fresh delivery",
            "--event-id",
            "work-event-stable-recovery-create",
            "--idempotency-key",
            "work-command-stable-recovery-create",
            "--json",
        ],
    );
    let original_version = work["version"].as_u64().expect("Work version");
    let original_team_id = work["accountable_team_id"]
        .as_str()
        .expect("durable accountable Work Team")
        .to_string();
    let original_creator = work["created_by_member_id"]
        .as_str()
        .expect("durable Work creator")
        .to_string();

    let active_member = store
        .member_runs()
        .expect("ProviderRuntimeProjection rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("ProviderRuntimeProjection");
    let mut stopped_member = active_member.clone();
    let original_generation = stopped_member.runtime_generation;
    stopped_member.status = harness_core::MemberRunStatus::Stopped;
    stopped_member.coordination_status = harness_core::MemberCoordinationStatus::Closed;
    stopped_member.finished_at = Some("unix-ms:stable-recovery-stop".to_string());
    stopped_member.last_event_at = stopped_member.finished_at.clone();
    store
        .compare_and_append_member_run(&active_member, &stopped_member)
        .expect("record stopped generation");

    let recover = |idempotent_retry: bool| {
        let output = run_firm_with_env(
            &home,
            home.base(),
            &[
                "--project",
                &project_id,
                "team-run",
                "recover",
                "--id",
                &run_id,
                "--json",
            ],
            &[
                ("KIMI_CODE_BIN", fake_kimi.as_str()),
                ("FAKE_KIMI_VERSION", "0.36.1"),
                ("FAKE_KIMI_ENV_MARKER", acp_marker_value.as_str()),
            ],
        );
        assert!(
            output.status.success(),
            "{} recovery failed: {}",
            if idempotent_retry { "retry" } else { "initial" },
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("recovery JSON")
    };

    let first_report = recover(false);
    assert_eq!(first_report["rebound_works"].as_u64(), Some(1));
    assert_eq!(first_report["reopened"].as_u64(), Some(0));
    assert!(
        !acp_marker.exists(),
        "recovery redelivery must not start the provider"
    );

    let rebound_work = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|work| work.id == "work-stable-recovery")
        .expect("rebound Work");
    assert_eq!(rebound_work.version, original_version + 1);
    assert_eq!(
        rebound_work.active_member_run_id.as_deref(),
        Some(member_id.as_str())
    );
    assert_eq!(
        rebound_work.accountable_team_id.as_deref(),
        Some(original_team_id.as_str())
    );
    assert_eq!(
        rebound_work.created_by_member_id.as_deref(),
        Some(original_creator.as_str())
    );
    assert_eq!(
        store
            .latest_works()
            .expect("latest Works")
            .into_iter()
            .filter(|work| work.id == "work-stable-recovery")
            .count(),
        1,
        "recovery must revise, never recreate, Work"
    );

    let latest_member = store
        .member_runs()
        .expect("ProviderRuntimeProjection rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("recovered ProviderRuntimeProjection");
    assert_eq!(latest_member.id, member_id);
    assert_eq!(latest_member.runtime_generation, original_generation + 1);
    assert!(latest_member.native_session.is_none());
    assert_eq!(
        store
            .member_runs()
            .expect("ProviderRuntimeProjection rows")
            .into_iter()
            .map(|member| member.id)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([member_id.clone()]),
        "recovery must not mint a replacement durable ProviderRuntimeProjection identity"
    );

    let rebound_events = store
        .work_events()
        .expect("WorkEvents")
        .into_iter()
        .filter(|event| {
            event.work_id == "work-stable-recovery"
                && event.kind == harness_core::WorkEventKind::Rebound
        })
        .collect::<Vec<_>>();
    assert_eq!(rebound_events.len(), 1);
    assert_eq!(
        rebound_events[0].payload["previous_runtime_generation"],
        original_generation
    );
    assert_eq!(
        rebound_events[0].payload["replacement_runtime_generation"],
        original_generation + 1
    );
    let fresh_deliveries = store
        .latest_work_deliveries()
        .expect("WorkDeliveries")
        .into_iter()
        .filter(|delivery| {
            delivery.work_id == "work-stable-recovery"
                && delivery.work_version == rebound_work.version
        })
        .collect::<Vec<_>>();
    assert_eq!(fresh_deliveries.len(), 1);
    assert_eq!(fresh_deliveries[0].recipient_member_run_id, member_id);
    assert_eq!(
        fresh_deliveries[0].status,
        harness_core::ProviderWorkDispatchStatus::Queued
    );
    assert!(fresh_deliveries[0].provider_receipt_id.is_none());

    let retry_report = recover(true);
    assert_eq!(retry_report["rebound_works"].as_u64(), Some(0));
    let after_retry = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|work| work.id == "work-stable-recovery")
        .expect("Work after retry");
    assert_eq!(after_retry.version, rebound_work.version);
    assert_eq!(
        store
            .work_events()
            .expect("WorkEvents")
            .into_iter()
            .filter(|event| {
                event.work_id == "work-stable-recovery"
                    && event.kind == harness_core::WorkEventKind::Rebound
            })
            .count(),
        1,
        "idempotent recovery must not duplicate the rebound revision"
    );
}
