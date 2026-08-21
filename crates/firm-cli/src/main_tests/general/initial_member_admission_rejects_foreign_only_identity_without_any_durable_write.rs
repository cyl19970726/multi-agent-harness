use super::*;

#[test]
fn initial_member_admission_rejects_foreign_only_identity_without_any_durable_write() {
    let (store, root) = temp_store("initial-member-admission-space-fence");
    let (project_context, team_id) =
        ensure_legacy_unit_test_team_binding(&store).expect("seed exact Team binding");
    let local = TeamMemberSpec {
        agent_member_id: "agent-initial-local".into(),
        name: "InitialLocal".into(),
        role: "builder".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/local".into()],
        resume_native_session_id: None,
        initial_work: Some("deliver the local half".into()),
    };
    let foreign_only = TeamMemberSpec {
        agent_member_id: "agent-initial-foreign-only".into(),
        name: "InitialForeignOnly".into(),
        role: "builder".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/foreign".into()],
        resume_native_session_id: None,
        initial_work: Some("deliver the foreign half".into()),
    };
    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &team_id,
        std::slice::from_ref(&local),
    )
    .expect("seed local AgentMember");
    ensure_unit_test_canonical_members(
        &store,
        "foreign-member-space",
        &team_id,
        std::slice::from_ref(&foreign_only),
    )
    .expect("seed foreign-only AgentMember");
    let members = vec![local, foreign_only];
    let before = durable_store_file_bytes(&store);

    let error = match create_team_run(
        &store,
        Some(&project_context),
        Some("unit-test-space"),
        None,
        "Reject a mixed-scope initial roster atomically",
        None,
        "test",
        None,
        None,
        Some(team_id.clone()),
        None,
        None,
        &members,
    ) {
        Ok(_) => panic!("foreign-only AgentMember must fail exact-space admission"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("not part of AgentTeam"),
        "unexpected scoped error: {error}"
    );
    assert_eq!(
            durable_store_file_bytes(&store),
            before,
            "failed initial admission must not write TeamRun, runtime, event, Work, provider, or canonical ledgers"
        );

    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &team_id,
        std::slice::from_ref(&members[1]),
    )
    .expect("materialize the same AgentMember in the selected space");
    let retried = create_team_run(
        &store,
        Some(&project_context),
        Some("unit-test-space"),
        None,
        "Retry the same initial roster",
        None,
        "test",
        None,
        None,
        Some(team_id),
        None,
        None,
        &members,
    )
    .expect("zero-write rejection must leave the roster retryable");
    assert_eq!(retried.member_runs.len(), 2);
    assert_eq!(retried.works.len(), 2);
    std::fs::remove_dir_all(root).expect("cleanup");
}
