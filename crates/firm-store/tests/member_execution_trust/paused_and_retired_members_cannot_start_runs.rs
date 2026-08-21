use super::*;

#[test]
fn paused_and_retired_members_cannot_start_runs() {
    let harness = TestStore::new("member-status");
    let host = human("host");
    let team_run = seed_team(&harness.store, "status", &["paused", "retired"]);
    // seed_team already created both durable AgentMembers; pause/retire them.
    harness
        .store
        .transition_trust_agent_member(
            &context(host.clone(), "member.pause", "pause", 1),
            "paused",
            AgentMemberOrganizationStatus::Paused,
            "t2",
        )
        .expect("pause member");
    assert_eq!(
        trust_code(
            admit_existing_member_run(
                &harness.store,
                &host,
                member_run("run-paused", "paused", &team_run.id, false),
                runtime_member_run(
                    &member_run("run-paused", "paused", &team_run.id, false),
                    "Paused",
                ),
            )
            .expect_err("paused member cannot run")
        ),
        TrustErrorCode::AgentMemberPaused
    );
    harness
        .store
        .transition_trust_agent_member(
            &context(host.clone(), "member.retire", "retire", 1),
            "retired",
            AgentMemberOrganizationStatus::Retired,
            "t2",
        )
        .expect("retire member");
    assert_eq!(
        trust_code(
            admit_existing_member_run(
                &harness.store,
                &host,
                member_run("run-retired", "retired", &team_run.id, false),
                runtime_member_run(
                    &member_run("run-retired", "retired", &team_run.id, false),
                    "Retired",
                ),
            )
            .expect_err("retired member cannot run")
        ),
        TrustErrorCode::AgentMemberRetired
    );
}
