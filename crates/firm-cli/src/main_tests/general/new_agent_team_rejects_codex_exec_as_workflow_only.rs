use super::*;

#[test]
fn new_agent_team_rejects_codex_exec_as_workflow_only() {
    let (store, root) = temp_store("team-reject-codex-exec");
    let result = create_team_run(
        &store,
        None,
        None,
        None,
        "Do interactive team work",
        None,
        "test",
        None,
        None,
        None,
        None,
        None,
        &[TeamMemberSpec {
            agent_member_id: "agent-batch-member".into(),
            name: "BatchMember".into(),
            role: "worker".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_exec".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        }],
    );
    let error = match result {
        Ok(_) => panic!("Agent Team must reject one-shot Codex exec"),
        Err(error) => error,
    };
    assert!(
        matches!(error, CliError::Usage(ref message) if message.contains("workflow-only")),
        "unexpected error: {error:?}"
    );

    let created = create_two_member_team_run(&store);
    let add_result = add_team_run_member(
        &store,
        None,
        &created.team_run.id,
        &TeamMemberSpec {
            agent_member_id: "agent-late-batch-member".into(),
            name: "LateBatchMember".into(),
            role: "worker".into(),
            provider: "codex".into(),
            execution_mode: Some("codex_exec".into()),
            model: None,
            effort: None,
            service_tier: None,
            provider_cwd_hint: None,
            owned_paths: Vec::new(),
            resume_native_session_id: None,
            initial_work: None,
        },
        Some("Join the running collaboration"),
    );
    let add_error = match add_result {
        Ok(_) => panic!("Agent Team add-member must reject one-shot Codex exec"),
        Err(error) => error,
    };
    assert!(
        matches!(add_error, CliError::Usage(ref message) if message.contains("workflow-only")),
        "unexpected add-member error: {add_error:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}
