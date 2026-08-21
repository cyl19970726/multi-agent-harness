use super::*;

#[test]
fn workspace_binding_requires_exact_member_run_and_project_placement() {
    let harness = TestStore::new("workspace-placement");
    let host = human("host");
    let team_run = seed_team(&harness.store, "workspace-placement", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );

    let mut missing_run = workspace_binding("missing-run", "/trust-test/missing", &host);
    missing_run.team_run_id = team_run.id.clone();
    missing_run.member_run_id = "run-missing".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_workspace_binding(
                    &context(host.clone(), "workspace.bind", "missing-run", 0),
                    missing_run,
                )
                .expect_err("workspace MemberRun must resolve")
        ),
        TrustErrorCode::InvalidStateTransition
    );

    let mut wrong_project = workspace_binding("wrong-project", "/trust-test/project", &host);
    wrong_project.team_run_id = team_run.id.clone();
    wrong_project.member_run_id = "runtime-member-a".into();
    wrong_project.project_binding_id = "project-other".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_workspace_binding(
                    &context(host, "workspace.bind", "wrong-project", 0),
                    wrong_project,
                )
                .expect_err("workspace ProjectBinding must match TeamRun placement")
        ),
        TrustErrorCode::WorkspaceRepositoryMismatch
    );
}
