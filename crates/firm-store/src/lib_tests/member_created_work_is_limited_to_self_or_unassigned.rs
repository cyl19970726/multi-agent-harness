use super::*;

#[test]
fn member_created_work_cannot_embed_runtime_ownership() {
    let (root, store, run, member_a, member_b) = work_test_fixture("member-work-authority");
    let operations_before = store.work_operations().unwrap().len();

    let mut peer_owned = unassigned_test_work(&run.id, "work-peer-owned");
    peer_owned.active_member_run_id = Some(member_b.id.clone());
    peer_owned.claim_mode = WorkClaimMode::HostAssign;
    let error = store
        .insert_work(
            peer_owned,
            member_work_context(
                &member_a.id,
                "we-member-peer",
                "member-create-peer",
                "unix-ms:2",
            ),
        )
        .expect_err("ordinary Member must not assign peer-owned Work");
    assert!(error
        .to_string()
        .contains("LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED"));

    let mut self_owned = unassigned_test_work(&run.id, "work-self-owned");
    self_owned.active_member_run_id = Some(member_a.id.clone());
    self_owned.claim_mode = WorkClaimMode::HostAssign;
    let self_owned_error = store
        .insert_work(
            self_owned,
            member_work_context(
                &member_a.id,
                "we-member-self",
                "member-create-self",
                "unix-ms:3",
            ),
        )
        .expect_err("Member cannot create runtime-owned Work");
    assert!(self_owned_error
        .to_string()
        .contains("LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED"));

    let mut host_preassigned = unassigned_test_work(&run.id, "work-host-preassigned");
    host_preassigned.owner_member_id = Some(member_a.agent_member_id.clone());
    host_preassigned.assignee_membership_id = Some(format!(
        "membership:{}:{}",
        run.agent_team_id, member_a.agent_member_id
    ));
    let error = store
        .insert_work(
            host_preassigned,
            host_work_context(
                "we-host-preassigned",
                "host-create-preassigned",
                "unix-ms:3.1",
            ),
        )
        .expect_err("Host creation cannot bypass canonical membership assignment");
    assert!(error
        .to_string()
        .contains("WORK_CREATE_UNASSIGNED_REQUIRED"));

    let mut member_self_owned = unassigned_test_work(&run.id, "work-member-self-owned");
    member_self_owned.owner_member_id = Some(member_a.agent_member_id.clone());
    let error = store
        .insert_work(
            member_self_owned,
            member_work_context(
                &member_a.id,
                "we-member-stable-self",
                "member-create-stable-self",
                "unix-ms:3.2",
            ),
        )
        .expect_err("Member creation cannot self-assign stable responsibility");
    assert!(error
        .to_string()
        .contains("WORK_CREATE_UNASSIGNED_REQUIRED"));

    let mut mismatched = unassigned_test_work(&run.id, "work-mismatched-membership");
    mismatched.owner_member_id = Some(member_a.agent_member_id.clone());
    mismatched.assignee_membership_id = Some(format!(
        "membership:{}:{}",
        run.agent_team_id, member_b.agent_member_id
    ));
    let error = store
        .insert_work(
            mismatched,
            host_work_context("we-host-mismatch", "host-create-mismatch", "unix-ms:3.3"),
        )
        .expect_err("mismatched responsibility cannot be created");
    assert!(error
        .to_string()
        .contains("WORK_CREATE_UNASSIGNED_REQUIRED"));
    assert_eq!(store.work_operations().unwrap().len(), operations_before);

    let unassigned = store
        .insert_work(
            unassigned_test_work(&run.id, "work-unassigned-child"),
            member_work_context(
                &member_a.id,
                "we-member-open",
                "member-create-open",
                "unix-ms:4",
            ),
        )
        .expect("Member creates unassigned Work");
    assert!(unassigned.owner_member_id.is_none());
    assert!(unassigned.active_member_run_id.is_none());
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &unassigned,
        &member_a,
        "we-assign-after-create",
        "assign-after-create",
        "unix-ms:5",
    );
    assert_eq!(
        assigned.owner_member_id.as_deref(),
        Some(member_a.agent_member_id.as_str())
    );
    assert_eq!(assigned.active_member_run_id, None);

    let mut failed_member = member_a.clone();
    failed_member.status = MemberRunStatus::Failed;
    failed_member.finished_at = Some("unix-ms:6".into());
    store
        .compare_and_append_member_run(&member_a, &failed_member)
        .expect("persist Active+Failed provider runtime fixture");
    let before_failed_create = store.work_operations().unwrap();
    let error = store
        .insert_work(
            unassigned_test_work(&run.id, "work-created-by-failed-runtime"),
            member_work_context(
                &failed_member.id,
                "we-failed-create",
                "failed-create",
                "unix-ms:7",
            ),
        )
        .expect_err("Active+Failed runtime cannot author new Work");
    assert!(error
        .to_string()
        .contains("only a live ProviderRuntimeProjection"));
    assert_eq!(store.work_operations().unwrap(), before_failed_create);

    std::fs::remove_dir_all(root).expect("remove temp store");
}
