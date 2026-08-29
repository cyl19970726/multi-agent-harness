use super::*;

#[test]
fn host_mode_transition_is_closed_generation_fenced_and_atomic() {
    let (root, store, managed_run, _, _) = work_test_fixture("host-mode-transition");
    let host_actor = store
        .exact_team_run_host_actor(&managed_run.id)
        .expect("exact Host actor");
    let managed_host = store
        .member_runs()
        .expect("read MemberRuns")
        .into_iter()
        .find(|member| member.agent_member_id == "agent-host")
        .expect("exact Host MemberRun");
    let mut closed_managed = managed_host.clone();
    closed_managed.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed_managed.status = MemberRunStatus::Stopped;
    store
        .compare_and_append_member_run(&managed_host, &closed_managed)
        .expect("close managed Host");

    let mut external_run = managed_run.clone();
    external_run.host_control_mode = firm_core::HostControlMode::ExternalInteractive;
    external_run.host_thread_id = Some("external-host-thread".into());
    external_run.updated_at = "unix-ms:2".into();
    let mut external_host = closed_managed.clone();
    external_host.provider_profile = Some(external_interactive_test_profile("codex"));
    external_host.native_session = None;
    external_host.coordination_status = firm_core::MemberCoordinationStatus::Active;
    external_host.status = MemberRunStatus::Idle;
    external_host.runtime_generation += 1;
    external_host.started_at = "unix-ms:2".into();
    external_host.last_event_at = Some("unix-ms:2".into());
    external_host.finished_at = None;
    store
        .compare_and_transition_host_mode(
            &host_actor,
            &managed_run,
            &external_run,
            &closed_managed,
            &external_host,
        )
        .expect("managed Host becomes explicit external Host");
    assert_eq!(
        store
            .team_runs()
            .unwrap()
            .into_iter()
            .rev()
            .find(|run| run.id == managed_run.id)
            .unwrap()
            .host_control_mode,
        firm_core::HostControlMode::ExternalInteractive
    );

    let mut closed_external = external_host.clone();
    closed_external.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed_external.status = MemberRunStatus::Stopped;
    store
        .compare_and_append_member_run(&external_host, &closed_external)
        .expect("close external Host coordination");
    let mut managed_again_run = external_run.clone();
    managed_again_run.host_control_mode = firm_core::HostControlMode::Managed;
    managed_again_run.host_thread_id = None;
    managed_again_run.updated_at = "unix-ms:3".into();
    let mut managed_again_host = closed_external.clone();
    let mut managed_profile = provider_compatibility_test_profile();
    managed_profile.agent_runtime_provider = Some(firm_core::AgentRuntimeProvider("codex".into()));
    managed_profile.provider = "codex".into();
    managed_profile.execution_mode = "codex_app_server".into();
    managed_again_host.provider_profile = Some(managed_profile);
    managed_again_host.coordination_status = firm_core::MemberCoordinationStatus::Active;
    managed_again_host.status = MemberRunStatus::Queued;
    managed_again_host.runtime_generation += 1;
    managed_again_host.started_at = "unix-ms:3".into();
    managed_again_host.last_event_at = Some("unix-ms:3".into());
    managed_again_host.finished_at = None;
    store
        .compare_and_transition_host_mode(
            &host_actor,
            &external_run,
            &managed_again_run,
            &closed_external,
            &managed_again_host,
        )
        .expect("external Host becomes managed Host");
    assert_eq!(
        store
            .trust_member_runs("unit-test-space")
            .unwrap()
            .into_iter()
            .find(|member| member.id == managed_again_host.id)
            .unwrap()
            .runtime_generation,
        managed_again_host.runtime_generation
    );
    assert!(store
        .compare_and_transition_host_mode(
            &host_actor,
            &external_run,
            &managed_again_run,
            &closed_external,
            &managed_again_host,
        )
        .expect_err("stale Host transition is fenced")
        .to_string()
        .contains("changed concurrently"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
