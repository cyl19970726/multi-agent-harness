use super::*;

#[test]
fn work_delegation_is_atomic_idempotent_and_prevents_flat_team_cycles() {
    let (root, store, run_a, member_a, run_b, member_b) =
        delegation_test_fixture("delegation-atomic-cycle");
    let source = store
        .insert_work(
            assigned_delegation_work(&run_a, &member_a, "source-a"),
            host_work_context("work-source-a", "create-source-a", "unix-ms:2"),
        )
        .expect("create source Work");
    let request = delegation_request("delegation-a-b", &source, &run_b.agent_team_id);
    let target_request = assigned_delegation_work(&run_b, &member_b, "target-b");
    let create_context = host_work_context(
        "delegation-create-a-b",
        "delegate-source-a-to-b",
        "unix-ms:3",
    );
    let (created, target) = store
        .create_work_delegation_with_target_work(
            request.clone(),
            target_request.clone(),
            create_context.clone(),
        )
        .expect("create Delegation and target Work atomically");
    assert_eq!(created.version, 1);
    assert_eq!(created.target_work_ref.work_id, target.id);
    assert_eq!(
        target.accountable_team_id.as_deref(),
        Some(run_b.agent_team_id.as_str())
    );
    assert_eq!(
        store
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")
            .expect("atomic operations")
            .len(),
        1
    );

    let retry = store
        .create_work_delegation_with_target_work(
            request.clone(),
            target_request.clone(),
            create_context.clone(),
        )
        .expect("same command retry is idempotent");
    assert_eq!(retry, (created.clone(), target.clone()));
    assert_eq!(store.latest_work_delegations().unwrap().len(), 1);

    let mut changed_target_intent = target_request.clone();
    changed_target_intent.title = "different delegated outcome".into();
    let fingerprint_conflict = store
        .create_work_delegation_with_target_work(
            request.clone(),
            changed_target_intent,
            create_context,
        )
        .expect_err("idempotency key cannot hide changed target Work intent");
    assert!(fingerprint_conflict
        .to_string()
        .contains("IDEMPOTENCY_CONFLICT"));

    let mut changed_entity_ids = request.clone();
    changed_entity_ids.id = "different-delegation-id".into();
    changed_entity_ids.target_work_ref.work_id = "different-target-work-id".into();
    let mut changed_target_id = target_request;
    changed_target_id.id = "different-target-work-id".into();
    let identity_conflict = store
        .create_work_delegation_with_target_work(
            changed_entity_ids,
            changed_target_id,
            host_work_context(
                "delegation-created-retry-envelope",
                "delegate-source-a-to-b",
                "unix-ms:4",
            ),
        )
        .expect_err("idempotency key cannot hide changed explicit entity ids");
    assert!(identity_conflict
        .to_string()
        .contains("IDEMPOTENCY_CONFLICT"));

    let mut conflicting = request.clone();
    conflicting.source_work_ref.work_id = "different-source".into();
    let conflict = store
        .create_work_delegation_with_target_work(
            conflicting,
            assigned_delegation_work(&run_b, &member_b, "unused-target"),
            host_work_context("ignored", "delegate-source-a-to-b", "unix-ms:4"),
        )
        .expect_err("one idempotency key cannot change intent");
    assert!(conflict.to_string().contains("IDEMPOTENCY_CONFLICT"));

    let reverse = delegation_request("delegation-b-a", &target, &run_a.agent_team_id);
    let reverse_target = assigned_delegation_work(&run_a, &member_a, "target-a-reverse");
    let cycle = store
        .create_work_delegation_with_target_work(
            reverse,
            reverse_target,
            host_work_context(
                "delegation-create-b-a",
                "delegate-target-b-to-a",
                "unix-ms:5",
            ),
        )
        .expect_err("A -> B -> A Team cycle must be rejected");
    assert!(cycle.to_string().contains("DELEGATION_CYCLE"));
    assert!(!store
        .latest_works()
        .unwrap()
        .iter()
        .any(|work| work.id == "target-a-reverse"));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
