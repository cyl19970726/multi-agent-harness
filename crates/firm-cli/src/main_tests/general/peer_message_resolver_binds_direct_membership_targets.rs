use super::*;

#[test]
fn peer_message_resolver_binds_direct_membership_targets() {
    let fixture = PeerMessagingFixture::new("peer-resolver-member-target");
    let mut draft = fixture.team_draft("hello one member");
    let recipient = harness_core::agentfirm_api::MessageRecipientRef {
        kind: harness_core::agentfirm_api::MessageRecipientKind::AgentMember,
        id: fixture.target_member_id.clone(),
    };
    draft.address_kind = harness_core::agentfirm_api::MessageAddressKind::DirectAgent;
    draft.target_ref = recipient.clone();
    draft.recipients = vec![recipient];
    let resolved = resolve_peer_team_message_admission_authority(
        &fixture.store,
        &fixture.firm_home,
        "space-test",
        &fixture.node_id,
        &fixture.sender_actor(),
        &draft,
        None,
    )
    .expect("direct member resolution");
    assert_eq!(
        resolved.authority.target_membership_id.as_deref(),
        Some("membership:target-team:target-member")
    );
    assert_eq!(resolved.authority.target_membership_generation, Some(1));
    assert_eq!(
        resolved.authority.target_agent_member_id.as_deref(),
        Some("target-member")
    );
    assert_eq!(
        resolved.authority.target_subscription_id,
        "direct:target-member:membership:target-team:target-member"
    );
    assert_eq!(
        resolved.authority.target_authorization_policy_ref,
        "team.direct.active-members"
    );
    fixture
        .store
        .revalidate_peer_team_delivery_subscription("space-test", &resolved.authority)
        .expect("direct authority revalidates");
    fixture.cleanup();
}
