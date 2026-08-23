use super::*;

#[test]
fn rebind_redelivers_same_member_run_id_at_a_higher_runtime_generation() {
    let (root, store, run, member, _) = work_test_fixture("same-id-generation-rebind");
    let mut assigned = unassigned_test_work(&run.id, "work-same-id-rebind");
    assigned.claim_mode = WorkClaimMode::HostAssign;
    assigned.owner_member_id = Some(member.agent_member_id.clone());
    assigned.active_member_run_id = Some(member.id.clone());
    let created = store
        .insert_work(
            assigned,
            member_work_context(
                &member.id,
                "event-create-same-id-rebind",
                "command-create-same-id-rebind",
                "unix-ms:3",
            ),
        )
        .expect("create assigned Work");

    let mut failed = member.clone();
    failed.status = MemberRunStatus::Failed;
    failed.finished_at = Some("unix-ms:4".into());
    store
        .compare_and_append_member_run(&member, &failed)
        .expect("record failed generation");
    let mut replacement = member.clone();
    replacement.runtime_generation += 1;
    replacement.status = MemberRunStatus::Idle;
    replacement.started_at = "unix-ms:5".into();
    replacement.finished_at = None;
    store
        .compare_and_advance_member_run_generation(&failed, &replacement)
        .expect("append same-id replacement generation");

    let rebound = store
        .rebind_work(
            &created.id,
            created.version,
            &replacement.id,
            host_work_context(
                "event-rebind-same-id-generation",
                "command-rebind-same-id-generation",
                "unix-ms:6",
            ),
        )
        .expect("higher same-id generation must fence and redeliver Work");
    assert_eq!(rebound.active_member_run_id, created.active_member_run_id);
    assert_eq!(rebound.accountable_team_id, created.accountable_team_id);
    assert_eq!(rebound.created_by_member_id, created.created_by_member_id);
    let operation = store
        .work_operations()
        .unwrap()
        .into_iter()
        .find(|operation| operation.event.kind == WorkEventKind::Rebound)
        .expect("Rebound operation");
    assert_eq!(operation.event.payload["previous_runtime_generation"], 1);
    assert_eq!(operation.event.payload["replacement_runtime_generation"], 2);
    assert!(store
        .legacy_provider_work_dispatches_for_export()
        .unwrap()
        .iter()
        .any(|delivery| {
            delivery.work_id == rebound.id
                && delivery.work_version == rebound.version
                && delivery.recipient_member_run_id == replacement.id
                && delivery.status == ProviderWorkDispatchStatus::Queued
        }));
    assert!(store
        .rebind_work(
            &rebound.id,
            rebound.version,
            &replacement.id,
            host_work_context(
                "event-repeat-same-id-generation",
                "command-repeat-same-id-generation",
                "unix-ms:7",
            ),
        )
        .expect_err("same runtime generation cannot rebound twice")
        .to_string()
        .contains("WORK_ALREADY_BOUND"));

    std::fs::remove_dir_all(root).expect("remove temp store");
}
