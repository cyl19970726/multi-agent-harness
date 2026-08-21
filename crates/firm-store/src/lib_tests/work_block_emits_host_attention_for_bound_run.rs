use super::*;

#[test]
fn work_block_emits_host_attention_for_bound_run() {
    let (root, store, run, member, _) = work_test_fixture("work-block-ha");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-block-ha-1"),
            host_work_context("we-block-1", "create-block-ha", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &work.id,
            work.version,
            &member.id,
            member_work_context(&member.id, "we-block-2", "claim-block-ha", "unix-ms:3"),
        )
        .expect("claim Work");
    let _blocked = store
        .block_work(
            &claimed.id,
            claimed.version,
            &member.id,
            "dependency missing",
            member_work_context(&member.id, "we-block-3", "block-block-ha", "unix-ms:4"),
        )
        .expect("block Work");
    let attentions = store.host_attentions().expect("host attentions");
    let blocked = attentions
        .iter()
        .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkBlocked);
    assert!(
        blocked.is_some(),
        "bound run must emit WorkBlocked on block"
    );
    assert_eq!(blocked.unwrap().team_run_id, run.id);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
