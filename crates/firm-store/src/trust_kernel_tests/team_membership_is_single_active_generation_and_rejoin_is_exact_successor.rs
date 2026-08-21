use super::*;

#[test]
fn team_membership_is_single_active_generation_and_rejoin_is_exact_successor() {
    let (store, root) = fabric_store();
    seed_membership_scope(&store);
    let first = membership_fixture("membership-1", 1);
    store
        .join_team_membership(
            &context("host", "membership.join", "membership-1", 0),
            first.clone(),
        )
        .unwrap();

    let operations_before_duplicate = store.canonical_operations().unwrap();
    let subscriptions_before_duplicate = store.fabric_message_subscriptions("space-test").unwrap();
    let duplicate = store
        .join_team_membership(
            &context("host", "membership.join", "membership-2", 0),
            membership_fixture("membership-2", 2),
        )
        .expect_err("a second active generation must fail under the Store lock");
    assert!(duplicate.to_string().contains("already have an active"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_duplicate
    );
    assert_eq!(
        store.fabric_message_subscriptions("space-test").unwrap(),
        subscriptions_before_duplicate
    );

    let mut leave_context = context(
        "membership-agent",
        "membership.leave",
        "membership-1:leave",
        1,
    );
    leave_context.authenticated_actor.kind = ActorKind::AgentMember;
    store
        .leave_team_membership(&leave_context, &first.id, "t-leave")
        .unwrap();

    let wrong_generation = store
        .join_team_membership(
            &context("host", "membership.join", "membership-3", 0),
            membership_fixture("membership-3", 3),
        )
        .expect_err("rejoin cannot skip a membership generation");
    assert!(wrong_generation
        .to_string()
        .contains("exact successor generation 2"));
    store
        .join_team_membership(
            &context("host", "membership.join", "membership-2", 0),
            membership_fixture("membership-2", 2),
        )
        .unwrap();
    let active = store
        .fabric_team_memberships("space-test")
        .unwrap()
        .into_iter()
        .filter(|membership| {
            membership.state == TeamMembershipStatus::Active
                && membership.agent_member_id == "membership-agent"
        })
        .collect::<Vec<_>>();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].membership_generation, 2);
    fs::remove_dir_all(root).unwrap();
}
