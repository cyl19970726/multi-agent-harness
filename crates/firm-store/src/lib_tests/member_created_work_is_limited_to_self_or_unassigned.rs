use super::*;

#[test]
fn member_created_work_cannot_embed_runtime_ownership() {
    let (root, store, run, member_a, member_b) = work_test_fixture("member-work-authority");

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

    std::fs::remove_dir_all(root).expect("remove temp store");
}
