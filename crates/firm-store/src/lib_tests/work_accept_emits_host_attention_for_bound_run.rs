use super::*;

#[test]
fn canonical_work_accept_emits_attention_for_the_exact_bound_run() {
    let (root, store, run, member, _) = work_test_fixture("work-accept-ha");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-accept-ha-1"),
            host_work_context("accept-create", "accept-create", "unix-ms:2"),
        )
        .expect("create Work");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "accept-assign",
        "accept-assign",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "accept-start",
        "accept-start",
        "unix-ms:4",
    );
    let submitted = submit_started_work_for_test(
        &store,
        &active,
        &member,
        "accept-result",
        "done",
        (vec!["artifact://z".into()], Vec::new()),
        "unix-ms:5",
    );
    accept_result_for_test(
        &store,
        &submitted,
        "accept-result",
        "accept-result-host",
        "unix-ms:6",
    );

    let attentions = store.host_attentions().expect("host attentions");
    let accepted = attentions
        .iter()
        .find(|attention| {
            attention.work_id == created.id && attention.kind == HostAttentionKind::WorkAccepted
        })
        .expect("canonical acceptance attention");
    assert_eq!(accepted.team_run_id, run.id);
    assert_eq!(accepted.member_run_id.as_deref(), Some(member.id.as_str()));
    assert!(attentions.iter().any(|attention| {
        attention.work_id == created.id
            && attention.kind == HostAttentionKind::WorkReviewRequested
            && attention.member_run_id.as_deref() == Some(member.id.as_str())
    }));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
