use super::*;

/// `team-run work release` used to be the only work verb that demanded
/// `--team-run-id`, and `redeliver` is the Host exit from a delivery frozen on
/// a member generation that no longer exists. Both are exercised through the
/// real CLI dispatch, not through the application layer directly.
#[test]
fn team_work_cli_redelivers_and_derives_release_team_run() {
    let (store, root) = temp_store("cli-work-redeliver");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "redeliver-supervisor",
            std::process::id(),
            "test://redeliver",
            current_unix_ms_u64(),
            60_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let space_id = lease.execution_space_id.clone();
    let resolved = ResolvedStore {
        root: root.clone(),
        source: StoreSource::SpaceCurrent,
        project_selection_explicit: false,
        context: None,
        execution_space_context: Some(ExecutionSpace {
            id: space_id.clone(),
            name: "Redeliver space".into(),
            store_root: root.clone(),
            default_project_binding_id: None,
            company_id: None,
        }),
    };
    let create_work = |id: &str| {
        harness_application::WorkApplication::new(&store)
            .create(harness_application::CreateWorkCommand {
                work_id: id.into(),
                team_run_id: created.team_run.id.clone(),
                accountable_team_id: created.team_run.agent_team_id.clone(),
                title: id.into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "done".into(),
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                expected_version: 0,
                context: WorkCommandContext {
                    event_id: format!("event-{id}"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("create-{id}"),
                    created_at: now_string(),
                    duplicate_ok: false,
                },
            })
            .expect("create Work")
    };

    // `release` derives the TeamRun from the Work, like every other work verb.
    let work = create_work("work-cli-release");
    let assigned = assign_test_work_to_member(&store, &space_id, &created, &member, &work);
    team_run_work_command(
        &store,
        &resolved,
        &[
            "release".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--idempotency-key".into(),
            "cli-release-derived".into(),
        ],
    )
    .expect("team-run work release must not require --team-run-id");
    let released = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == assigned.id)
        .unwrap();
    assert_eq!(released.version, assigned.version + 1);
    assert_eq!(released.assignee_membership_id, None);

    // The explicit `--team-run-id` form stays accepted for compatibility.
    let compat = create_work("work-cli-release-compat");
    let compat = assign_test_work_to_member(&store, &space_id, &created, &member, &compat);
    team_run_work_command(
        &store,
        &resolved,
        &[
            "release".into(),
            "--team-run-id".into(),
            created.team_run.id.clone(),
            "--work-id".into(),
            compat.id.clone(),
            "--expected-version".into(),
            compat.version.to_string(),
            "--idempotency-key".into(),
            "cli-release-explicit".into(),
        ],
    )
    .expect("the explicit --team-run-id form still works");

    // `redeliver` reaches the store guard through the real dispatch: an
    // unassigned Work has no member generation to redeliver to.
    let unassigned = create_work("work-cli-redeliver");
    let error = team_run_work_command(
        &store,
        &resolved,
        &[
            "redeliver".into(),
            "--work-id".into(),
            unassigned.id.clone(),
            "--expected-version".into(),
            unassigned.version.to_string(),
            "--reason".into(),
            "member reopened".into(),
            "--idempotency-key".into(),
            "cli-redeliver-unassigned".into(),
        ],
    )
    .expect_err("an unassigned Work has nothing to redeliver");
    assert!(error.to_string().contains("WORK_NOT_ASSIGNED"), "{error}");

    let mismatch = team_run_work_command(
        &store,
        &resolved,
        &[
            "redeliver".into(),
            "--team-run-id".into(),
            "another-team-run".into(),
            "--work-id".into(),
            unassigned.id.clone(),
            "--expected-version".into(),
            unassigned.version.to_string(),
            "--idempotency-key".into(),
            "cli-redeliver-mismatched-run".into(),
        ],
    )
    .expect_err("an explicit --team-run-id must identify the Work's TeamRun");
    assert!(
        mismatch.to_string().contains(&format!(
            "--team-run-id another-team-run does not match Work {}'s TeamRun {}",
            unassigned.id, created.team_run.id
        )),
        "{mismatch}"
    );

    // With a live WorkExecutionBinding the delivery is not stale, and the
    // refusal proves the verb reads the selected space's binding fabric.
    let live_work = create_work("work-cli-redeliver-live");
    let live_work = assign_test_work_to_member(&store, &space_id, &created, &member, &live_work);
    bind_test_responsible_work_execution(&store, &lease, &member, &live_work);
    let live = team_run_work_command(
        &store,
        &resolved,
        &[
            "redeliver".into(),
            "--work-id".into(),
            live_work.id.clone(),
            "--expected-version".into(),
            live_work.version.to_string(),
            "--idempotency-key".into(),
            "cli-redeliver-live".into(),
        ],
    )
    .expect_err("a live execution binding is not a stale delivery");
    assert!(live.to_string().contains("WORK_DELIVERY_LIVE"), "{live}");

    // Redelivery is space-scoped like every other canonical Work mutation.
    let spaceless = ResolvedStore {
        root: root.clone(),
        source: StoreSource::StoreFlag,
        project_selection_explicit: false,
        context: None,
        execution_space_context: None,
    };
    let usage = team_run_work_command(
        &store,
        &spaceless,
        &[
            "redeliver".into(),
            "--work-id".into(),
            unassigned.id.clone(),
            "--expected-version".into(),
            unassigned.version.to_string(),
        ],
    )
    .expect_err("redelivery requires an explicitly selected Execution Space");
    assert!(usage.to_string().contains("--space"), "{usage}");

    let unknown = team_run_work_command(
        &store,
        &resolved,
        &[
            "redeliver".into(),
            "--work-id".into(),
            unassigned.id.clone(),
            "--expected-version".into(),
            unassigned.version.to_string(),
            "--membership-id".into(),
            "membership-1".into(),
        ],
    )
    .expect_err("redelivery never moves responsibility");
    assert!(
        unknown.to_string().contains("unknown work option"),
        "{unknown}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
