use super::*;

#[test]
fn work_delegation_cancel_is_cas_fenced_and_idempotent() {
    let (root, store, run_a, member_a, run_b, member_b) =
        delegation_test_fixture("delegation-cancel");
    let source = store
        .insert_work(
            assigned_delegation_work(&run_a, &member_a, "source-cancel"),
            host_work_context("work-source-cancel", "create-source-cancel", "unix-ms:2"),
        )
        .expect("create source Work");
    let (delegation, _) = store
        .create_work_delegation_with_target_work(
            delegation_request("delegation-cancel", &source, &run_b.agent_team_id),
            assigned_delegation_work(&run_b, &member_b, "target-cancel"),
            host_work_context(
                "delegation-create-cancel",
                "delegate-source-cancel",
                "unix-ms:3",
            ),
        )
        .expect("create Delegation");
    let stale = store
        .cancel_work_delegation(
            &delegation.id,
            0,
            "target no longer needed",
            host_work_context(
                "delegation-cancel-stale",
                "cancel-delegation-stale",
                "unix-ms:4",
            ),
        )
        .expect_err("stale expected version is fenced");
    assert!(stale.to_string().contains("DELEGATION_VERSION_CONFLICT"));
    let context = host_work_context(
        "delegation-cancel-event",
        "cancel-delegation-command",
        "unix-ms:5",
    );
    let cancelled = store
        .cancel_work_delegation(
            &delegation.id,
            delegation.version,
            "target no longer needed",
            context.clone(),
        )
        .expect("cancel Delegation");
    assert_eq!(cancelled.state, WorkDelegationState::Cancelled);
    assert_eq!(cancelled.version, 2);
    assert_eq!(
        store
            .cancel_work_delegation(
                &delegation.id,
                delegation.version,
                "target no longer needed",
                context,
            )
            .expect("same cancel command replays idempotently"),
        cancelled
    );
    let conflict = store
        .cancel_work_delegation(
            &delegation.id,
            delegation.version,
            "different reason",
            host_work_context("ignored", "cancel-delegation-command", "unix-ms:6"),
        )
        .expect_err("same key cannot change cancel reason");
    assert!(conflict.to_string().contains("IDEMPOTENCY_CONFLICT"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
