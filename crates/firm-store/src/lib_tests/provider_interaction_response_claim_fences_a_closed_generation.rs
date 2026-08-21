use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn provider_interaction_response_claim_fences_a_closed_generation() {
    let root = team_test_root("provider-interaction-stale-claim");
    let store = HarnessStore::new(&root);
    let (request_body, request) =
        seed_provider_interaction_bridge(&store, "run-interaction-stale-claim");
    let response = provider_interaction_response(&request_body, &request, "continue");
    store
        .record_provider_interaction_response(&response, "unix-ms:4")
        .expect("record queued response");

    let current = latest_by_id(store.member_runs().expect("members"), |member| {
        member.id.clone()
    })
    .remove(&request_body.member)
    .expect("member");
    let mut closed = current.clone();
    closed.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed.status = MemberRunStatus::Stopped;
    closed.finished_at = Some("unix-ms:5".into());
    store
        .compare_and_append_member_run(&current, &closed)
        .expect("close member");
    store
        .acquire_test_supervisor_lease(
            &request.team_run_id,
            "supervisor-stale-claim",
            43,
            "test",
            100,
            1_000,
        )
        .expect("lease");
    assert!(store
        .claim_team_message_delivery(
            &request.team_run_id,
            &response.id,
            &request_body.member,
            "supervisor-stale-claim",
            1,
            "stale-claim",
            101,
            1_000,
            "unix-ms:6",
        )
        .expect_err("closed generation cannot receive provider response")
        .to_string()
        .contains("stale"));
    assert_eq!(
        store
            .record_provider_interaction_response(&response, "unix-ms:7")
            .expect("exact command retry still converges")
            .id,
        response.id
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
