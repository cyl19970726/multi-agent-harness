use super::*;

#[test]
fn partial_legacy_team_run_is_diagnostic_only_until_canonical_completeness_is_restored() {
    let (store, root) = temp_store("partial-current-team-run-fence");
    let created = create_two_member_team_run(&store);
    let dangling_spec = TeamMemberSpec {
        agent_member_id: "agent-dangling-legacy".into(),
        name: "DanglingLegacy".into(),
        role: "legacy_only".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/dangling".into()],
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&dangling_spec),
    )
    .expect("seed exact-space durable identity, but no canonical MemberRun");
    let dangling = build_member_run_for_team(
        None,
        &created.team_run.id,
        &dangling_spec,
        created.team_run.execution_root.as_deref(),
    )
    .expect("build historical dangling runtime projection");
    let current = latest_team_run(&store, &created.team_run.id).expect("current TeamRun");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-partial-fence",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("seed current Supervisor before reconstructing partial history");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let mut partial = current.clone();
    partial.member_run_ids.push(dangling.id.clone());
    partial.updated_at = "unix-ms:s6-partial".into();
    // Reconstruct the exact historical S6 on-disk state without exposing
    // a production Store writer capable of creating it.
    for (ledger, value) in [
        (
            "team_runs.jsonl",
            serde_json::to_value(&partial).expect("serialize partial TeamRun"),
        ),
        (
            "member_runs.jsonl",
            serde_json::to_value(&dangling).expect("serialize dangling runtime"),
        ),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(store.root().join(ledger))
            .expect("open historical fixture ledger");
        serde_json::to_writer(&mut file, &value).expect("write historical fixture row");
        writeln!(file).expect("terminate historical fixture row");
        file.sync_all().expect("persist historical fixture row");
    }
    assert!(store
        .member_runs()
        .expect("raw Legacy diagnostics remain readable")
        .iter()
        .any(|member| member.id == dangling.id));
    assert!(store
        .trust_member_runs("unit-test-space")
        .expect("canonical members")
        .iter()
        .all(|member| member.id != dangling.id));

    let third = TeamMemberSpec {
        agent_member_id: "agent-valid-third".into(),
        name: "ValidThird".into(),
        role: "reviewer".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/review".into()],
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&third),
    )
    .expect("seed valid third identity before the zero-write boundary");
    let token = "c".repeat(64);
    let capability = test_collaboration_capability(&store, &lease, &created.member_runs[0], &token);
    let (control_rx, _control_registration) =
        register_live_member_control(&created.member_runs[0], &capability, 2);
    let before = durable_store_file_bytes(&store);
    let expect_incomplete = |error: CliError| {
        let rendered = error.to_string();
        assert!(
            rendered.contains("MEMBER_RUN_MATERIALIZATION_INCOMPLETE")
                && rendered.contains(&dangling.id),
            "partial current TeamRun must name the exact missing member: {rendered}"
        );
    };
    expect_incomplete(CliError::Store(
        store
            .acquire_team_supervisor_under_node_lease(
                &partial.id,
                &partial.execution_node_id,
                &lease.node_daemon_id,
                lease.node_daemon_generation,
                &lease.execution_space_id,
                &partial.project_binding_id,
                &lease.supervisor_id,
                lease.owner_process_id,
                &lease.owner_locator,
                current_unix_ms_u64(),
                60_000,
            )
            .expect_err("partial TeamRun cannot acquire or reuse Supervisor authority"),
    ));

    expect_incomplete(
        add_team_run_member(
            &store,
            None,
            &created.team_run.id,
            &third,
            Some("review without mutating a partial TeamRun"),
        )
        .expect_err("partial TeamRun cannot admit a valid third member or initial Work"),
    );
    let partial = latest_team_run(&store, &created.team_run.id).expect("partial TeamRun");
    expect_incomplete(
        team_run_display_json(&store, &partial, None)
            .expect_err("current status must fail closed for partial TeamRun"),
    );
    expect_incomplete(
        canonical_team_messages_for_run(&store, &partial.id)
            .expect_err("current inbox/lineage must fail closed for partial TeamRun"),
    );
    expect_incomplete(
        member_run_detail_json(&store, &created.member_runs[0].id)
            .expect_err("current member detail must fail closed for partial TeamRun"),
    );
    let ledger = TeamRunLedger::without_supervisor(&store, &partial.id);
    expect_incomplete(
        require_member_provider_session_authority(&ledger, &created.member_runs[0], false)
            .expect_err("provider/control authority must fail before resolving a session"),
    );
    expect_incomplete(
        transition_team_run(&store, &partial.id, TeamRunStatus::Cancelled)
            .expect_err("current lifecycle mutation must fail closed for partial TeamRun"),
    );
    expect_incomplete(
        rename_team_run_member(
            &store,
            &partial.id,
            &created.member_runs[0].id,
            "RenamedWhilePartial",
        )
        .expect_err("rename must reject a partial TeamRun"),
    );
    expect_incomplete(
        deactivate_team_run_member(
            &store,
            &partial.id,
            &created.member_runs[0].id,
            "must remain byte-zero",
        )
        .expect_err("deactivate must reject a partial TeamRun"),
    );
    expect_incomplete(
        reopen_team_member_value(
            &store,
            &partial.id,
            &created.member_runs[0].id,
            &serde_json::json!({"reopened_by": "host", "reason": "partial fence"}),
        )
        .expect_err("reopen must reject before provider-profile refresh"),
    );
    let start_error = match prepare_team_run_start_body(&store, &partial.id, 2) {
        Ok(_) => panic!("start preparation must reject before provider-profile refresh"),
        Err(error) => error,
    };
    expect_incomplete(start_error);
    expect_incomplete(
        team_run_board_summary_text(&store, &partial.id)
            .expect_err("board summary must reject partial current state"),
    );
    expect_incomplete(CliError::Store(
        store
            .current_team_run_events(&partial.id)
            .expect_err("current event projection must reject partial state"),
    ));
    assert!(
        !store
            .legacy_team_run_events()
            .expect("explicit Legacy event diagnostic remains readable")
            .is_empty(),
        "historical event rows remain available only through the Legacy reader"
    );

    let close_admission_hook_calls = AtomicUsize::new(0);
    expect_incomplete(
        dispatch_local_live_member_control_with_close_admission_hook(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &AtomicBool::new(true),
            &Mutex::new(()),
            LiveMemberControlRequest::Steer {
                team_run_id: partial.id.clone(),
                member_run_id: created.member_runs[0].id.clone(),
                content: "must never reach provider".into(),
                requested_by: "host".into(),
            },
            || {
                close_admission_hook_calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .expect_err("Steer must reject before touching the live provider control"),
    );
    expect_incomplete(
        dispatch_local_live_member_control_with_close_admission_hook(
            &store,
            &lease.supervisor_id,
            lease.generation,
            &AtomicBool::new(true),
            &Mutex::new(()),
            LiveMemberControlRequest::Close {
                team_run_id: partial.id.clone(),
                member_run_id: created.member_runs[0].id.clone(),
                reason: "must never latch".into(),
                requested_by: "host".into(),
            },
            || {
                close_admission_hook_calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .expect_err("Close must reject before its durable admission hook"),
    );
    assert_eq!(close_admission_hook_calls.load(Ordering::SeqCst), 0);
    assert!(
        matches!(
            control_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "partial TeamRun control reached the provider channel"
    );
    assert_eq!(
            durable_store_file_bytes(&store),
            before,
            "all rejected current projections, controls, admission, and Work creation must be byte-zero"
        );

    let canonical = canonical_member_run_admission("unit-test-space", &dangling);
    store
        .legacy_import_create_trust_member_run_projection(&canonical.context, canonical.run)
        .expect("explicit test-only reconstruction restores canonical completeness");
    assert_eq!(
        store
            .current_team_run_execution_space(&partial)
            .expect("fully coherent TeamRun resolves"),
        "unit-test-space"
    );
    let (repaired, admitted, work) = add_team_run_member(
        &store,
        None,
        &partial.id,
        &third,
        Some("review after explicit coherence restoration"),
    )
    .expect("same request succeeds only after explicit canonical reconstruction");
    assert!(repaired.member_run_ids.contains(&admitted.id));
    assert!(work.is_some());
    std::fs::remove_dir_all(root).expect("cleanup");
}
