use super::*;

#[test]
fn concurrent_team_membership_join_has_one_linearized_winner() {
    let (store, root) = fabric_store();
    seed_membership_scope(&store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for suffix in ["a", "b"] {
        let root = root.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let contender = HarnessStore::new(root);
            barrier.wait();
            contender.join_team_membership(
                &context(
                    "host",
                    "membership.join",
                    &format!("membership-concurrent-{suffix}"),
                    0,
                ),
                membership_fixture(&format!("membership-concurrent-{suffix}"), 1),
            )
        }));
    }
    barrier.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().expect("membership contender"))
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
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
    assert_eq!(
        store
            .fabric_message_subscriptions("space-test")
            .unwrap()
            .into_iter()
            .filter(|subscription| {
                subscription.subscriber_kind == MessageSubjectKind::AgentMember
                    && subscription.subscriber_ref == "membership-agent"
                    && subscription.status == MessageSubscriptionStatus::Active
            })
            .count(),
        2
    );
    fs::remove_dir_all(root).unwrap();
}
