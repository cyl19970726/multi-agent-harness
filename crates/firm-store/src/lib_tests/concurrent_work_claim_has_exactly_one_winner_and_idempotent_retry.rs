use super::*;

#[test]
fn concurrent_work_claim_has_exactly_one_winner_and_idempotent_retry() {
    let (root, store, run, member_a, member_b) = work_test_fixture("work-claim-race");
    store
        .insert_work(
            unassigned_test_work(&run.id, "work-race"),
            host_work_context("we-race-1", "create-race", "unix-ms:2"),
        )
        .expect("create Work");
    let contenders = [member_a.clone(), member_b.clone()];
    let store = Arc::new(store);
    let barrier = Arc::new(Barrier::new(3));
    let handles = [member_a, member_b]
        .into_iter()
        .enumerate()
        .map(|(index, member)| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.claim_work(
                    "work-race",
                    1,
                    &member.id,
                    member_work_context(
                        &member.id,
                        &format!("we-race-{}", index + 2),
                        &format!("claim-race-{index}"),
                        "unix-ms:3",
                    ),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).expect("winner");
    assert_eq!(winner.phase, WorkPhase::Open);
    assert_eq!(winner.active_member_run_id, None);
    let winner_agent_member = winner
        .owner_member_id
        .as_deref()
        .expect("stable accountable AgentMember");
    let retry_member = contenders
        .iter()
        .find(|member| member.agent_member_id == winner_agent_member)
        .map(|member| member.id.as_str())
        .expect("claiming MemberRun");
    assert!(winner.assignee_membership_id.is_some());
    assert!(store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("claim bindings")
        .is_empty());
    assert!(store.work_reports().expect("claim reports").is_empty());
    let retried = store
        .claim_work(
            "work-race",
            1,
            retry_member,
            member_work_context(
                retry_member,
                "ignored",
                if retry_member.ends_with("-a") {
                    "claim-race-0"
                } else {
                    "claim-race-1"
                },
                "unix-ms:4",
            ),
        )
        .expect("idempotent retry");
    assert_eq!(retried, winner);
    let before_unbound_execution = store.work_operations().expect("Work operations");
    let start_error = store
        .start_work(
            &winner.id,
            winner.version,
            retry_member,
            member_work_context(
                retry_member,
                "we-race-unbound-start",
                "unbound-start",
                "unix-ms:5",
            ),
        )
        .expect_err("claim alone must not authorize Start");
    assert!(start_error
        .to_string()
        .contains("does not hold responsibility"));
    let submit_error = store
        .submit_work(
            &winner.id,
            winner.version,
            retry_member,
            "unbound submit",
            Vec::new(),
            Vec::new(),
            member_work_context(
                retry_member,
                "we-race-unbound-submit",
                "unbound-submit",
                "unix-ms:5",
            ),
        )
        .expect_err("claim alone must not authorize Submit");
    assert!(submit_error
        .to_string()
        .contains("does not hold active Work responsibility"));
    assert_eq!(
        store.work_operations().expect("Work operations"),
        before_unbound_execution
    );
    let winner_member = contenders
        .iter()
        .find(|member| member.id == retry_member)
        .expect("winning MemberRun fixture");
    let started = start_claimed_work_for_test(
        &store,
        &winner,
        winner_member,
        "we-race-bound-start",
        "bound-start",
        "unix-ms:6",
    );
    let submitted = store
        .submit_work(
            &started.id,
            started.version,
            retry_member,
            "bound submit",
            Vec::new(),
            Vec::new(),
            member_work_context(
                retry_member,
                "we-race-bound-submit",
                "bound-submit",
                "unix-ms:7",
            ),
        )
        .expect("canonical binding authorizes Submit");
    assert_eq!(submitted.phase, WorkPhase::Review);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
