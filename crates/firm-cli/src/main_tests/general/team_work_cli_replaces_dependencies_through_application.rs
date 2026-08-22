use super::*;

#[test]
fn team_work_cli_replaces_dependencies_through_application() {
    let (store, root) = temp_store("cli-replace-work-dependencies");
    let created = create_two_member_team_run(&store);
    let create = |id: &str| {
        harness_application::WorkApplication::new(&store)
            .create(harness_application::CreateWorkCommand {
                work_id: id.into(),
                team_run_id: created.team_run.id.clone(),
                accountable_team_id: created.team_run.agent_team_id.clone(),
                title: id.into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "done".into(),
                claim_mode: WorkClaimMode::TeamClaim,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                initial_member_run_id: None,
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
    let prerequisite = create("work-prerequisite");
    let dependent = create("work-dependent");
    let resolved = ResolvedStore {
        root,
        source: StoreSource::StoreFlag,
        project_selection_explicit: false,
        context: None,
        execution_space_context: None,
    };
    team_run_work_command(
        &store,
        &resolved,
        &[
            "replace-dependencies".into(),
            "--team-id".into(),
            created.team_run.agent_team_id.clone(),
            "--work-id".into(),
            dependent.id.clone(),
            "--expected-version".into(),
            dependent.version.to_string(),
            "--prerequisite-work-id".into(),
            prerequisite.id.clone(),
            "--idempotency-key".into(),
            "cli-replace-dependencies".into(),
        ],
    )
    .expect("replace dependencies through CLI");

    let updated = store
        .latest_works()
        .expect("load Works")
        .into_iter()
        .find(|work| work.id == dependent.id)
        .expect("updated Work");
    assert_eq!(updated.prerequisite_work_ids, vec!["work-prerequisite"]);
}
