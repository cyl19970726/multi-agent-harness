use super::*;
use harness_core::CurrentWorkDraft;

fn create_unrelated_run_with_work(store: &HarnessStore, index: usize) -> CreatedTeamRun {
    let worker_id = format!("bounded-worker-{index}");
    let specs = [
        TeamMemberSpec {
            agent_member_id: worker_id.clone(),
            name: format!("Bounded Worker {index}"),
            role: "worker".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_app_server".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        },
        TeamMemberSpec {
            agent_member_id: "host".into(),
            name: format!("Bounded Host {index}"),
            role: "host".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_app_server".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        },
    ];
    let source_team = store
        .latest_teams()
        .expect("read unit-test Team")
        .into_values()
        .next()
        .expect("unit-test Team exists");
    let team = AgentTeam {
        id: format!("bounded-unrelated-team-{index}"),
        name: format!("Bounded unrelated Team {index}"),
        member_ids: vec![worker_id],
        legacy_mission_id: None,
        mission_id: String::new(),
        ..source_team
    };
    ensure_unit_test_canonical_team(store, "unit-test-space", &team, &specs)
        .expect("create unrelated canonical Team");
    let project_root = store.root().join("unit-test-project");
    let project = ProjectContext {
        id: "unit-test-project".into(),
        project_root: project_root.clone(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };
    let created = create_team_run(
        store,
        Some(&project),
        Some("unit-test-space"),
        Some(project_root.to_string_lossy().into_owned()),
        &format!("Unrelated projection fixture {index}"),
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        Some(team.id),
        None,
        None,
        &specs,
    )
    .expect("create unrelated TeamRun");
    insert_fixture_work(
        store,
        &created,
        &format!("unrelated-projection-work-{index}"),
        &format!("unrelated-projection-work-event-{index}"),
    );
    created
}

fn insert_fixture_work(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    work_id: &str,
    event_id: &str,
) -> Work {
    let mut draft = CurrentWorkDraft::new(
        work_id.into(),
        created.team_run.id.clone(),
        created.team_run.agent_team_id.clone(),
        format!("Projection fixture {work_id}"),
        "Exercise scoped projection".into(),
        "The selected projection stays identical".into(),
        WorkClaimMode::HostAssign,
        WorkPriority::Normal,
        compatibility_team_actor("host", "test"),
        "unix-ms:1000".into(),
    );
    draft.eligible_member_ids = vec![created.member_runs[0].agent_member_id.clone()];
    store
        .insert_work(
            draft.into_work(),
            WorkCommandContext {
                event_id: event_id.into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: event_id.into(),
                created_at: "unix-ms:1000".into(),
                duplicate_ok: false,
            },
        )
        .expect("insert fixture Work")
}

fn assert_scoped_matches_filtered_global(store: &HarnessStore, team_run_id: &str) {
    let mut filtered_global = dashboard_team_run_snapshot_via_global(store, team_run_id)
        .expect("filter global multi-run snapshot");
    let mut scoped =
        dashboard_team_run_snapshot(store, team_run_id).expect("resolve scoped multi-run snapshot");
    filtered_global["generated_at"] = serde_json::Value::Null;
    scoped["generated_at"] = serde_json::Value::Null;
    assert_eq!(
        scoped, filtered_global,
        "scoped snapshot must equal the filtered global snapshot byte-for-byte"
    );
}

fn acquire_fixture_lease(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    suffix: &str,
) -> TeamSupervisorLease {
    store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            &format!("scoped-retained-{suffix}"),
            std::process::id(),
            &format!("test://scoped-retained-{suffix}"),
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire fixture TeamRun Supervisor")
}

fn member_context(member_run_id: &str, event_id: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: event_id.into(),
        created_at: "unix-ms:1200".into(),
        duplicate_ok: false,
    }
}

fn dispatch_fixture_work(
    store: &HarnessStore,
    lease: &TeamSupervisorLease,
    created: &CreatedTeamRun,
    work: &Work,
) {
    let delivery = store
        .fabric_work_deliveries(&lease.execution_space_id)
        .expect("read fixture Work deliveries")
        .into_iter()
        .find(|delivery| delivery.work_id == work.id)
        .expect("fixture Work delivery exists");
    let claim_id = format!("{}:claim", work.id);
    store
        .claim_work_for_provider(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "test.scoped_retained.work.claim",
                claim_id.clone(),
                0,
            ),
            &delivery.id,
            &created.team_run.execution_node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            &claim_id,
            harness_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            "unix-ms:1150",
        )
        .expect("claim fixture Work delivery");
    store
        .record_work_provider_receipt(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "test.scoped_retained.work.provider_received",
                format!("{}:receipt", work.id),
                0,
            ),
            &delivery.id,
            &created.team_run.execution_node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            &claim_id,
            &format!("{}:provider-receipt", work.id),
            "unix-ms:1160",
        )
        .expect("record fixture Work provider receipt");
}

fn add_cross_team_delegation_fixture(
    store: &HarnessStore,
    source_run: &CreatedTeamRun,
    target_run: &CreatedTeamRun,
) {
    let source_lease = acquire_fixture_lease(store, source_run, "delegation-source");
    let target_lease = acquire_fixture_lease(store, target_run, "delegation-target");
    ensure_test_runtime_fabric(store, source_run, &source_lease);
    ensure_test_runtime_fabric(store, target_run, &target_lease);

    let source_member = &source_run.member_runs[0];
    let source = insert_fixture_work(
        store,
        source_run,
        "bounded-delegation-source",
        "bounded-delegation-source-created",
    );
    let source = assign_test_work_to_member(
        store,
        &source_lease.execution_space_id,
        source_run,
        source_member,
        &source,
    );
    let mut target = CurrentWorkDraft::new(
        "bounded-delegation-target".into(),
        target_run.team_run.id.clone(),
        target_run.team_run.agent_team_id.clone(),
        "Cross-Team projection target".into(),
        "Emit the Delegation rollup on the target Work ledger".into(),
        "Both scoped projections retain the latest Delegation".into(),
        WorkClaimMode::HostAssign,
        WorkPriority::Normal,
        target_run
            .team_run
            .host_actor
            .clone()
            .expect("target Host actor"),
        "unix-ms:1100".into(),
    )
    .into_work();
    target.eligible_member_ids = vec![target_run.member_runs[0].agent_member_id.clone()];
    let delegation = WorkDelegation {
        id: "bounded-cross-team-delegation".into(),
        source_work_ref: WorkRef {
            team_run_id: source.team_run_id.clone(),
            work_id: source.id.clone(),
        },
        source_work_version: source.version,
        source_owner_member_id: source
            .owner_member_id
            .clone()
            .expect("assigned source owner"),
        created_by_member_run_id: None,
        target_agent_team_id: target_run.team_run.agent_team_id.clone(),
        target_work_ref: WorkRef {
            team_run_id: String::new(),
            work_id: String::new(),
        },
        delegated_by_actor: source_run
            .team_run
            .host_actor
            .clone()
            .expect("source Host actor"),
        state: WorkDelegationState::Active,
        resolution_summary: None,
        blocker_reason: None,
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let (_, target) = store
        .create_work_delegation_with_target_work(
            delegation,
            target,
            WorkCommandContext {
                event_id: "bounded-cross-team-delegation-created".into(),
                performed_by_actor: source_run
                    .team_run
                    .host_actor
                    .clone()
                    .expect("source Host actor"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "bounded-cross-team-delegation-created".into(),
                created_at: "unix-ms:1100".into(),
                duplicate_ok: false,
            },
        )
        .expect("create cross-Team Delegation");
    let target_member = &target_run.member_runs[0];
    let target = assign_test_work_to_member(
        store,
        &target_lease.execution_space_id,
        target_run,
        target_member,
        &target,
    );
    bind_test_responsible_work_execution(store, &target_lease, target_member, &target);
    dispatch_fixture_work(store, &target_lease, target_run, &target);
    let target = store
        .start_work(
            &target.id,
            target.version,
            &target_member.id,
            member_context(&target_member.id, "bounded-delegation-target-started"),
        )
        .expect("start delegated target Work");
    store
        .block_work(
            &target.id,
            target.version,
            &target_member.id,
            "projection fixture blocker",
            member_context(&target_member.id, "bounded-delegation-target-blocked"),
        )
        .expect("block target and emit Delegation rollup");
}

fn add_retarget_fixture(
    store: &HarnessStore,
    root: &std::path::Path,
    source_run: &CreatedTeamRun,
) -> CreatedTeamRun {
    let project = ProjectContext {
        id: source_run.team_run.project_binding_id.clone(),
        project_root: root.to_path_buf(),
        store_root: store.root().to_path_buf(),
        kind: ProjectKind::Repo,
        is_git_repo: false,
    };
    let specs = source_run
        .member_runs
        .iter()
        .map(|member| TeamMemberSpec {
            agent_member_id: member.agent_member_id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            provider: member.provider.clone(),
            execution_mode: member
                .provider_profile
                .as_ref()
                .map(|profile| profile.execution_mode.clone()),
            model: member.model.clone(),
            effort: None,
            service_tier: None,
            provider_cwd_hint: Some(root.to_string_lossy().into_owned()),
            owned_paths: member.owned_paths.clone(),
            resume_native_session_id: None,
            initial_work: None,
        })
        .collect::<Vec<_>>();
    let successor = create_team_run(
        store,
        Some(&project),
        Some("unit-test-space"),
        Some(root.to_string_lossy().into_owned()),
        "Successor projection fixture",
        None,
        "test",
        None,
        HostControlMode::Managed,
        Some(source_run.team_run.id.clone()),
        Some(source_run.team_run.agent_team_id.clone()),
        None,
        None,
        &specs,
    )
    .expect("create successor TeamRun");
    let work = insert_fixture_work(
        store,
        source_run,
        "bounded-retargeted-work",
        "bounded-retargeted-work-created",
    );
    store
        .retarget_work_execution(
            &work.id,
            work.version,
            &successor.team_run.id,
            WorkCommandContext {
                event_id: "bounded-retargeted-work-retargeted".into(),
                performed_by_actor: source_run
                    .team_run
                    .host_actor
                    .clone()
                    .expect("source Host actor"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "bounded-retargeted-work-retargeted".into(),
                created_at: "unix-ms:1050".into(),
                duplicate_ok: false,
            },
        )
        .expect("retarget Work onto successor TeamRun");
    successor
}

fn add_unrelated_fabric_rows(store: &HarnessStore, created: &CreatedTeamRun, index: usize) {
    let lease = acquire_fixture_lease(store, created, &format!("unrelated-{index}"));
    ensure_test_runtime_fabric(store, created, &lease);
    let team = store
        .latest_teams()
        .expect("read unrelated Team")
        .remove(&created.team_run.agent_team_id)
        .expect("unrelated Team exists");
    author_test_canonical_message(
        store,
        created,
        &lease,
        &lease.execution_space_id,
        &format!("bounded-unrelated-message-{index}"),
        &team.host_agent_id,
        &created.member_runs[0].agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Unrelated retained-row fixture",
        &format!("bounded-unrelated-correlation-{index}"),
        None,
        harness_core::agentfirm_api::ResponseIntent::Informational,
    );
}

fn append_jsonl<T: serde::Serialize>(store: &HarnessStore, name: &str, row: &T) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join(name))
        .expect("open fixture ledger");
    serde_json::to_writer(&mut file, row).expect("serialize fixture row");
    file.write_all(b"\n").expect("terminate fixture row");
    file.sync_all().expect("persist fixture row");
}

#[test]
fn retained_projected_rows_remain_bounded_with_200_unrelated_team_runs_and_works() {
    let (store, root) = temp_store("scoped-dashboard-retained-row-bound");
    let selected = create_two_member_team_run(&store);
    insert_fixture_work(
        &store,
        &selected,
        "bounded-selected-work",
        "bounded-selected-work-event",
    );

    // A second equivalence fixture exercises several real unrelated Teams and
    // Works through both projection paths before the cheaper 200-row scale
    // fixture is appended.
    let unrelated = (0..3)
        .map(|index| create_unrelated_run_with_work(&store, index))
        .collect::<Vec<_>>();
    add_cross_team_delegation_fixture(&store, &selected, &unrelated[0]);
    let successor = add_retarget_fixture(&store, &root, &selected);
    assert_scoped_matches_filtered_global(&store, &selected.team_run.id);
    assert_scoped_matches_filtered_global(&store, &successor.team_run.id);
    assert_scoped_matches_filtered_global(&store, &unrelated[0].team_run.id);

    let successor_snapshot = dashboard_team_run_snapshot(&store, &successor.team_run.id)
        .expect("resolve successor fixture snapshot");
    assert!(successor_snapshot["works"]
        .as_array()
        .expect("successor Works")
        .iter()
        .any(|work| work["id"] == "bounded-retargeted-work"));
    assert_eq!(
        successor_snapshot["work_events"]
            .as_array()
            .expect("successor Work events")
            .iter()
            .filter(|event| event["work_id"] == "bounded-retargeted-work")
            .count(),
        2,
        "the successor retains the Work's complete pre- and post-retarget history"
    );
    let source_snapshot = dashboard_team_run_snapshot(&store, &selected.team_run.id)
        .expect("resolve source fixture snapshot");
    assert!(!source_snapshot["works"]
        .as_array()
        .expect("source Works")
        .iter()
        .any(|work| work["id"] == "bounded-retargeted-work"));
    assert!(source_snapshot["work_delegations"]
        .as_array()
        .expect("source Delegations")
        .iter()
        .any(|delegation| {
            delegation["id"] == "bounded-cross-team-delegation" && delegation["state"] == "blocked"
        }));

    let scoped = source_snapshot;

    let baseline_counts = projected_row_counts(&scoped);
    // #264 follow-up reference for the post-scale identity check: the
    // filtered-global reference cannot be REBUILT after the raw scale append
    // — the raw rows deliberately lack canonical Host bindings, and the
    // global builder fails closed on them — so capture it now. The whole
    // #264 claim is that unrelated rows never enter the selected run's
    // projection, so this reference is exactly what a filtered-global built
    // after the append would contain.
    let mut filtered_global_reference =
        dashboard_team_run_snapshot_via_global(&store, &selected.team_run.id)
            .expect("filtered-global reference before the scale append");
    filtered_global_reference["generated_at"] = serde_json::Value::Null;
    let seed_run = selected.team_run.clone();
    let seed_operation = store
        .work_operations()
        .expect("read seed Work operation")
        .into_iter()
        .find(|operation| operation.work.id == "bounded-selected-work")
        .expect("seed Work operation exists");
    for index in 3..6 {
        let unrelated = create_unrelated_run_with_work(&store, index);
        add_unrelated_fabric_rows(&store, &unrelated, index);
    }
    for index in 0..200 {
        let mut run = seed_run.clone();
        run.id = format!("raw-unrelated-team-run-{index}");
        run.objective = format!("Raw unrelated TeamRun {index}");
        run.member_run_ids.clear();
        append_jsonl(&store, "team_runs.jsonl", &run);

        let mut operation = seed_operation.clone();
        operation.work.id = format!("raw-unrelated-work-{index}");
        operation.work.team_run_id = run.id.clone();
        operation.event.id = format!("raw-unrelated-work-event-{index}");
        operation.event.work_id = operation.work.id.clone();
        operation.event.team_run_id = run.id;
        operation.event.idempotency_key = operation.event.id.clone();
        append_jsonl(&store, "work_operations.jsonl", &operation);
    }

    let raw_started = std::time::Instant::now();
    let raw_team_run_rows = store.team_runs().expect("deserialize TeamRun ledger").len();
    let raw_work_rows = store
        .work_operations()
        .expect("deserialize Work ledger")
        .len();
    let raw_deserialization_elapsed = raw_started.elapsed();

    let scoped_started = std::time::Instant::now();
    let after = dashboard_team_run_snapshot(&store, &selected.team_run.id)
        .expect("resolve scoped snapshot after unrelated rows");
    let scoped_total_elapsed = scoped_started.elapsed();
    let after_counts = projected_row_counts(&after);
    assert_eq!(
        after_counts, baseline_counts,
        "retained/projected target rows must not grow with unrelated TeamRuns or Works"
    );
    // #264 follow-up: the scoped == filtered-global identity held before the
    // 200-row scale append (:471-473); it must also hold at scale. The scoped
    // snapshot after the append must equal the pre-append filtered-global
    // reference byte-for-byte — which both re-proves the identity and proves
    // the unrelated raw rows changed nothing in the selected run's
    // projection.
    let mut scoped_after = after;
    scoped_after["generated_at"] = serde_json::Value::Null;
    assert_eq!(
        scoped_after, filtered_global_reference,
        "scoped == filtered-global identity must still hold at scale: the scoped snapshot after the raw append must equal the pre-append filtered-global reference"
    );

    let projection_estimate = scoped_total_elapsed.saturating_sub(raw_deserialization_elapsed);
    eprintln!(
        "SCOPED_RETAINED_PROJECTION_BOUND target_projected_rows={} unrelated_team_runs=206 unrelated_works=206 post_baseline_real_runs_with_memberships_sessions_messages_deliveries=3 raw_team_run_rows={} raw_work_rows={} raw_deserialization_ms={} scoped_total_ms={} projection_after_raw_probe_ms={} raw_jsonl_deserialization_scales_with_store=true",
        after_counts.values().sum::<usize>(),
        raw_team_run_rows,
        raw_work_rows,
        raw_deserialization_elapsed.as_millis(),
        scoped_total_elapsed.as_millis(),
        projection_estimate.as_millis(),
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
