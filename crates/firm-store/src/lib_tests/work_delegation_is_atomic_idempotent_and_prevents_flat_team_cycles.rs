use super::*;

#[test]
fn work_delegation_is_atomic_idempotent_and_prevents_flat_team_cycles() {
    let (root, store, run_a, member_a, run_b, member_b) =
        delegation_test_fixture("delegation-atomic-cycle");
    let source = insert_assigned_delegation_work(
        &store,
        &run_a,
        &member_a,
        "source-a",
        "work-source-a",
        "create-source-a",
        "unix-ms:2",
    );
    let request = delegation_request("delegation-a-b", &source, &run_b.agent_team_id);
    let target_request = delegation_work(&run_b, "target-b");
    let create_context = run_host_work_context(
        &run_a,
        "delegation-create-a-b",
        "delegate-source-a-to-b",
        "unix-ms:3",
    );
    let works_before_refusals = store.latest_works().unwrap().len();
    let operations_before_refusals = store
        .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")
        .unwrap()
        .len();
    let mut preassigned_target = delegation_work(&run_b, "target-preassigned");
    preassigned_target.owner_member_id = Some(member_b.agent_member_id.clone());
    preassigned_target.assignee_membership_id = Some(format!(
        "membership:{}:{}",
        run_b.agent_team_id, member_b.agent_member_id
    ));
    let error = store
        .create_work_delegation_with_target_work(
            request.clone(),
            preassigned_target,
            run_host_work_context(
                &run_a,
                "delegation-preassigned-target",
                "delegation-preassigned-target",
                "unix-ms:2.5",
            ),
        )
        .expect_err("delegated target creation must also be unassigned");
    assert!(error
        .to_string()
        .contains("WORK_CREATE_UNASSIGNED_REQUIRED"));
    assert_eq!(store.latest_works().unwrap().len(), works_before_refusals);
    assert_eq!(
        store
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")
            .unwrap()
            .len(),
        operations_before_refusals
    );
    for (kind, id, suffix) in [
        (TeamActorKind::Host, "forged-host", "host"),
        (TeamActorKind::Operator, "operator", "operator"),
        (TeamActorKind::Service, "service", "service"),
    ] {
        let mut forged = create_context.clone();
        forged.event_id = format!("delegation-refused-{suffix}");
        forged.idempotency_key = format!("delegation-refused-{suffix}");
        forged.performed_by_actor = TeamActorRef {
            kind,
            id: id.into(),
            display_name: None,
            authn_source: Some("hostile-test".into()),
        };
        let error = store
            .create_work_delegation_with_target_work(
                request.clone(),
                target_request.clone(),
                forged,
            )
            .expect_err("only the exact Host may use Host delegation authority");
        assert!(
            error.to_string().contains("DELEGATION_NOT_AUTHORIZED")
                || error
                    .to_string()
                    .contains("TEAM_RUN_HOST_AUTHORITY_MISMATCH")
        );
    }
    assert_eq!(store.latest_works().unwrap().len(), works_before_refusals);
    assert_eq!(
        store
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")
            .unwrap()
            .len(),
        operations_before_refusals,
        "forged Host, Operator, and Service attempts have zero delegation side effects"
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
            run_host_work_context(
                &run_a,
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
            delegation_work(&run_b, "unused-target"),
            run_host_work_context(&run_a, "ignored", "delegate-source-a-to-b", "unix-ms:4"),
        )
        .expect_err("one idempotency key cannot change intent");
    assert!(conflict.to_string().contains("IDEMPOTENCY_CONFLICT"));

    let target = store
        .assign_work_to_membership(
            &target.id,
            target.version,
            &format!(
                "membership:{}:{}",
                run_b.agent_team_id, member_b.agent_member_id
            ),
            "delegation-test-space",
            run_host_work_context(&run_b, "assign-target-b", "assign-target-b", "unix-ms:4"),
        )
        .expect("assign target before using it as a delegation source");
    let reverse = delegation_request("delegation-b-a", &target, &run_a.agent_team_id);
    let reverse_target = delegation_work(&run_a, "target-a-reverse");
    let cycle = store
        .create_work_delegation_with_target_work(
            reverse,
            reverse_target,
            run_host_work_context(
                &run_b,
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
