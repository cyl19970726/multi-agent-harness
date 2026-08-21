use super::*;

#[test]
fn dynamic_member_admission_revalidates_paused_local_identity_before_writing() {
    let (store, root) = temp_store("dynamic-member-admission-paused-fence");
    let created = create_two_member_team_run(&store);
    let late = TeamMemberSpec {
        agent_member_id: "agent-dynamic-paused-local".into(),
        name: "DynamicPausedLocal".into(),
        role: "reviewer".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        "unit-test-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("seed local AgentMember");
    ensure_unit_test_canonical_members(
        &store,
        "foreign-member-space",
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("seed active foreign decoy for the old all-space precheck");
    store
        .transition_trust_agent_member(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "test-host".into(),
                },
                authority_actor: None,
                command_name: "test.agent_member.pause".into(),
                idempotency_key: "pause-local-before-admission".into(),
                expected_version: 1,
                request_fingerprint: None,
            },
            &late.agent_member_id,
            harness_core::agentfirm_api::AgentMemberOrganizationStatus::Paused,
            "unix-ms:3",
        )
        .expect("pause selected-space AgentMember");
    let before = durable_store_file_bytes(&store);

    let error = add_team_run_member(
        &store,
        None,
        &created.team_run.id,
        &late,
        Some("must not start while paused"),
    )
    .expect_err("selected-space paused state must win over active foreign decoy");
    assert!(
        error.to_string().contains("AGENT_MEMBER_PAUSED"),
        "unexpected paused error: {error}"
    );
    assert_eq!(
        durable_store_file_bytes(&store),
        before,
        "paused exact-space revalidation must occur before every durable admission write"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
