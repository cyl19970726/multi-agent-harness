use super::*;

#[test]
fn dynamic_member_admission_rejects_foreign_only_identity_without_any_durable_write() {
    let (store, root) = temp_store("dynamic-member-admission-space-fence");
    let created = create_two_member_team_run(&store);
    let late = TeamMemberSpec {
        agent_member_id: "agent-dynamic-foreign-only".into(),
        name: "DynamicForeignOnly".into(),
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
        "foreign-member-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("seed foreign-only AgentMember");
    let before = durable_store_file_bytes(&store);

    let error = add_team_run_member(
        &store,
        None,
        &created.team_run.id,
        &late,
        Some("review the accepted result"),
    )
    .expect_err("foreign-only AgentMember must fail dynamic exact-space admission");
    assert!(
        error.to_string().contains("selected Execution Space"),
        "unexpected scoped error: {error}"
    );
    assert_eq!(
        durable_store_file_bytes(&store),
        before,
        "failed dynamic admission must have byte-zero durable side effects"
    );

    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("materialize the same AgentMember in the selected space");
    let (run, member, work) = add_team_run_member(
        &store,
        None,
        &created.team_run.id,
        &late,
        Some("review the accepted result"),
    )
    .expect("zero-write rejection must leave the same name and identity retryable");
    assert!(run.member_run_ids.contains(&member.id));
    assert!(work.is_some());
    assert!(store
        .trust_member_runs("unit-test-space")
        .expect("canonical MemberRuns")
        .iter()
        .any(|candidate| candidate.id == member.id));
    std::fs::remove_dir_all(root).expect("cleanup");
}
