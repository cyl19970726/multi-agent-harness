use super::*;

#[test]
fn peer_message_resolver_reads_current_target_subscription_revision() {
    let fixture = PeerMessagingFixture::new("peer-resolver-current-revision");
    let draft = fixture.team_draft("hello peer team");
    let resolved = resolve_peer_team_message_admission_authority(
        &fixture.store,
        &fixture.firm_home,
        "space-test",
        &fixture.node_id,
        &fixture.sender_actor(),
        &draft,
        None,
    )
    .expect("local peer resolution");
    assert!(!resolved.requires_remote_route);
    assert_eq!(resolved.authority.company_id, "space:space-test");
    assert_eq!(resolved.authority.target_subscription_revision, 1);
    assert_eq!(
        resolved.authority.target_subscription_id,
        "team-inbox:target-team"
    );
    assert_eq!(
        resolved.authority.source_membership_id,
        "membership:source-team:sender-member"
    );
    assert_eq!(resolved.authority.source_session_id, "session-sender");

    // The availability trap: a target Team lifecycle transition advances
    // the durable subscription revision; re-resolution must read the
    // current revision instead of freezing a hardcoded one.
    let host_membership = fixture
        .store
        .fabric_team_memberships("space-test")
        .expect("memberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == "target-team"
                && membership.role == harness_core::agentfirm_api::TeamMembershipRole::Host
        })
        .expect("target host membership");
    let operator = harness_core::agentfirm_api::MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Human,
            id: "operator".into(),
        },
        authority_actor: None,
        command_name: "unit_test.team.lifecycle".into(),
        idempotency_key: "unit-test-team-off".into(),
        expected_version: 1,
        request_fingerprint: None,
    };
    fixture
        .store
        .transition_agent_team(
            &operator,
            "target-team",
            harness_core::AgentTeamStatus::Inactive,
            "t-off",
        )
        .expect("deactivate target team");
    resolve_peer_team_message_admission_authority(
        &fixture.store,
        &fixture.firm_home,
        "space-test",
        &fixture.node_id,
        &fixture.sender_actor(),
        &draft,
        None,
    )
    .expect_err("a Paused target Team Inbox is not admissible");
    fixture
        .store
        .activate_team_membership(
            &harness_core::agentfirm_api::MutationContext {
                idempotency_key: "unit-test-host-on".into(),
                expected_version: host_membership.revision + 1,
                ..operator.clone()
            },
            &host_membership.id,
            "t-host-on",
        )
        .expect("reactivate target host membership");
    fixture
        .store
        .transition_agent_team(
            &harness_core::agentfirm_api::MutationContext {
                idempotency_key: "unit-test-team-on".into(),
                expected_version: 2,
                ..operator
            },
            "target-team",
            harness_core::AgentTeamStatus::Active,
            "t-on",
        )
        .expect("reactivate target team");
    let resolved = resolve_peer_team_message_admission_authority(
        &fixture.store,
        &fixture.firm_home,
        "space-test",
        &fixture.node_id,
        &fixture.sender_actor(),
        &draft,
        None,
    )
    .expect("re-resolution after lifecycle");
    assert_eq!(resolved.authority.target_subscription_revision, 3);
    assert_eq!(resolved.authority.target_team_revision, 3);
    // The frozen authority passes the same Store fence the target applies.
    fixture
        .store
        .revalidate_peer_team_delivery_subscription("space-test", &resolved.authority)
        .expect("resolved authority revalidates against the durable subscription");
    fixture.cleanup();
}
