use super::*;

/// DEV-230 (#799, #734): `team-run work recover-lost-execution` is the Host
/// exit from a Work whose execution authority is provably gone. This proves
/// the verb reaches the store's fail-closed guards through the real CLI
/// dispatch: it refuses a Work with nothing lost, refuses a live binding, and
/// requires an explicitly selected Execution Space.
#[test]
fn team_work_cli_recovers_lost_execution_through_dispatch() {
    let (store, root) = temp_store("cli-work-recover-lost");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "recover-supervisor",
            std::process::id(),
            "test://recover",
            current_unix_ms_u64(),
            60_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let space_id = lease.execution_space_id.clone();
    let resolved_for = |space: Option<ExecutionSpace>| ResolvedStore {
        root: root.clone(),
        source: StoreSource::SpaceCurrent,
        project_selection_explicit: false,
        context: None,
        execution_space_context: space,
    };
    let resolved = resolved_for(Some(ExecutionSpace {
        id: space_id.clone(),
        name: "Recover space".into(),
        store_root: root.clone(),
        default_project_binding_id: None,
        company_id: None,
    }));
    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: "work-cli-recover".into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: "work-cli-recover".into(),
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
                event_id: "event-work-cli-recover".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "create-work-cli-recover".into(),
                created_at: now_string(),
                duplicate_ok: false,
            },
        })
        .expect("create Work");
    let assigned = assign_test_work_to_member(&store, &space_id, &created, &member, &work);

    // The verb needs an explicitly selected Execution Space, like redeliver.
    let unscoped = team_run_work_command(
        &store,
        &resolved_for(None),
        &[
            "recover-lost-execution".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--idempotency-key".into(),
            "cli-recover-unscoped".into(),
        ],
    )
    .expect_err("recovery requires --space");
    assert!(
        unscoped
            .to_string()
            .contains("requires an explicitly selected --space"),
        "{unscoped}"
    );

    // An assigned, never-dispatched Work has nothing lost: the store guard is
    // reached through the real dispatch.
    let not_lost = team_run_work_command(
        &store,
        &resolved,
        &[
            "recover-lost-execution".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--reason".into(),
            "daemon lost the generation".into(),
            "--idempotency-key".into(),
            "cli-recover-not-lost".into(),
        ],
    )
    .expect_err("a never-dispatched Work has nothing to recover");
    assert!(
        not_lost.to_string().contains("WORK_EXECUTION_NOT_LOST"),
        "{not_lost}"
    );

    // An explicit --team-run-id must identify the Work's TeamRun.
    let mismatch = team_run_work_command(
        &store,
        &resolved,
        &[
            "recover-lost-execution".into(),
            "--team-run-id".into(),
            "another-team-run".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--idempotency-key".into(),
            "cli-recover-mismatched-run".into(),
        ],
    )
    .expect_err("an explicit --team-run-id must identify the Work's TeamRun");
    assert!(
        mismatch.to_string().contains(&format!(
            "--team-run-id another-team-run does not match Work {}'s TeamRun {}",
            assigned.id, created.team_run.id
        )),
        "{mismatch}"
    );

    // With a live WorkExecutionBinding the execution is not lost, and the
    // refusal proves the verb reads the selected space's binding fabric.
    bind_test_responsible_work_execution(&store, &lease, &member, &assigned);
    let live = team_run_work_command(
        &store,
        &resolved,
        &[
            "recover-lost-execution".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--idempotency-key".into(),
            "cli-recover-live".into(),
        ],
    )
    .expect_err("a live binding is not a lost execution");
    assert!(
        live.to_string().contains("WORK_EXECUTION_AUTHORITY_LIVE"),
        "{live}"
    );
    let unchanged = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == assigned.id)
        .unwrap();
    assert_eq!(
        unchanged.version, assigned.version,
        "refusals write nothing"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The success path through the real dispatch: a live binding whose MemberRun
/// generation advanced without a clean Close is released and the Work returns
/// to Open at the next revision, with the Host event appended.
#[test]
fn team_work_cli_recovers_lost_execution_after_a_superseded_generation() {
    let (store, root) = temp_store("cli-work-recover-lost-success");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "recover-success-supervisor",
            std::process::id(),
            "test://recover-success",
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
            name: "Recover success space".into(),
            store_root: root.clone(),
            default_project_binding_id: None,
            company_id: None,
        }),
    };
    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: "work-cli-recover-success".into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: "work-cli-recover-success".into(),
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
                event_id: "event-work-cli-recover-success".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "create-work-cli-recover-success".into(),
                created_at: now_string(),
                duplicate_ok: false,
            },
        })
        .expect("create Work");
    let assigned = assign_test_work_to_member(&store, &space_id, &created, &member, &work);
    bind_test_responsible_work_execution(&store, &lease, &member, &assigned);

    // The MemberRun generation advances through generic recovery, with no
    // Close: the binding is frozen on generation 1 and can never settle.
    let projection = store
        .member_runs()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .expect("member projection");
    let mut advanced = projection.clone();
    advanced.runtime_generation += 1;
    advanced.status = MemberRunStatus::Queued;
    store
        .compare_and_advance_member_run_generation(&projection, &advanced)
        .expect("generic recovery advances the generation without a Close");

    team_run_work_command(
        &store,
        &resolved,
        &[
            "recover-lost-execution".into(),
            "--work-id".into(),
            assigned.id.clone(),
            "--expected-version".into(),
            assigned.version.to_string(),
            "--reason".into(),
            "generation advanced without a clean Close".into(),
            "--idempotency-key".into(),
            "cli-recover-success".into(),
        ],
    )
    .expect("a binding frozen on a superseded generation is recovered");
    let recovered = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == assigned.id)
        .unwrap();
    assert_eq!(recovered.version, assigned.version + 1);
    assert_eq!(recovered.phase, WorkPhase::Open);
    assert_eq!(
        recovered.assignee_membership_id,
        assigned.assignee_membership_id
    );
    let bindings = store
        .fabric_work_execution_bindings(&space_id)
        .unwrap()
        .into_iter()
        .filter(|binding| binding.work_id == assigned.id)
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].status,
        harness_core::agentfirm_api::WorkExecutionBindingStatus::Released
    );
    let recovered_event = store
        .work_operations()
        .unwrap()
        .into_iter()
        .find(|operation| {
            operation.work.id == assigned.id
                && operation.event.kind == harness_core::WorkEventKind::ExecutionRecovered
        })
        .expect("one ExecutionRecovered operation");
    assert_eq!(
        recovered_event.event.payload["released_binding"]["id"],
        serde_json::json!(bindings[0].id)
    );
    std::fs::remove_dir_all(root).unwrap();
}
