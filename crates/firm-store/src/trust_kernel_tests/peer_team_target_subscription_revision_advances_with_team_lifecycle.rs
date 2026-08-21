use super::*;

#[test]
fn peer_team_target_subscription_revision_advances_with_team_lifecycle() {
    let (store, root) = fabric_store();
    let (source_team, target_team, source_membership, _target_membership) =
        seed_peer_message_scope(&store);
    let subscription_id = format!("team-inbox:{}", target_team.id);
    let subscription_at = |store: &HarnessStore| {
        store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .find(|subscription| subscription.id == subscription_id)
            .expect("team inbox subscription")
    };
    let team_at = |store: &HarnessStore| {
        store
            .agent_teams("space-test")
            .unwrap()
            .into_iter()
            .find(|team| team.id == "target-team")
            .expect("target team")
    };
    let authority_at = |store: &HarnessStore| {
        let target_team = team_at(store);
        let subscription = subscription_at(store);
        peer_authority_fixture(
            "company-test",
            &source_team,
            &source_membership,
            "remote-sender",
            "session-peer-sender",
            &target_team,
            &subscription,
            None,
        )
    };
    let initial = authority_at(&store);
    assert_eq!(initial.target_subscription_revision, 1);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &initial)
        .expect("current subscription revision revalidates");

    // Team lifecycle transitions advance the durable subscription
    // revision; an authority frozen at the old revision is permanently
    // stale and must be re-resolved from the Store.
    store
        .transition_agent_team(
            &context(
                "fixture-host",
                "team.lifecycle.transition",
                "target-team-off",
                1,
            ),
            &target_team.id,
            firm_core::AgentTeamStatus::Inactive,
            "t-off",
        )
        .unwrap();
    assert_eq!(subscription_at(&store).revision, 2);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &initial)
        .expect_err("deactivated Team admits no new peer delivery");
    // Reactivation restores the Host membership generation first, then the
    // Team; the subscription revision advances again.
    let host_membership = store
        .fabric_team_memberships("space-test")
        .unwrap()
        .into_iter()
        .find(|membership| {
            membership.team_id == "target-team" && membership.role == TeamMembershipRole::Host
        })
        .unwrap();
    store
        .activate_team_membership(
            &context(
                "fixture-host",
                "team.membership.activate",
                "target-team-host-on",
                host_membership.revision,
            ),
            &host_membership.id,
            "t-host-on",
        )
        .unwrap();
    store
        .transition_agent_team(
            &context(
                "fixture-host",
                "team.lifecycle.transition",
                "target-team-on",
                2,
            ),
            &target_team.id,
            firm_core::AgentTeamStatus::Active,
            "t-on",
        )
        .unwrap();
    assert_eq!(subscription_at(&store).revision, 3);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &initial)
        .expect_err("the revision-1 authority stays stale after reactivation");

    let current = authority_at(&store);
    assert_eq!(current.target_subscription_revision, 3);
    assert_eq!(current.target_team_revision, team_at(&store).revision);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &current)
        .expect("re-resolved current subscription revision revalidates");
    let message = peer_message_fixture(
        "peer-after-reactivation",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "target-team".into(),
        },
        None,
    );
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-after-reactivation", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(current)),
        )
        .expect("authoring resumes under the current subscription revision");
    let delivery = store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.message_id == message.id)
        .expect("one Team Inbox delivery");
    assert_eq!(delivery.subscription_revision, 3);
    fs::remove_dir_all(root).unwrap();
}
