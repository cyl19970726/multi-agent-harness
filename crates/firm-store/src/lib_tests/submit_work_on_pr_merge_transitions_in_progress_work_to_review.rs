use super::*;

#[test]
fn submit_work_on_pr_merge_transitions_in_progress_work_to_review() {
    let (root, store, run, member, _) = work_test_fixture("github-merge-submit");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "github-merge-submit-1"),
            host_work_context("we-ms-1", "create-github-merge", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &created.id,
            created.version,
            &member.id,
            member_work_context(&member.id, "we-ms-2", "claim-github-merge", "unix-ms:3"),
        )
        .expect("claim Work");
    assert_eq!(claimed.phase, WorkPhase::Open);
    let claimed = start_claimed_work_for_test(
        &store,
        &claimed,
        &member,
        "we-ms-start",
        "start-github-merge",
        "unix-ms:3.5",
    );
    assert_eq!(claimed.phase, WorkPhase::Active);

    // Refuses when no MERGED pull_request link is present.
    let not_merged = store.submit_work_on_pr_merge(
        &claimed.id,
        claimed.version,
        "auto-submit",
        vec![test_github_link("OPEN", Some("success"))],
        host_work_context("we-ms-3", "submit-merge-reject", "unix-ms:4"),
    );
    assert!(
        not_merged.is_err()
            && not_merged
                .unwrap_err()
                .to_string()
                .contains("PR_MERGE_REQUIRED"),
        "auto-submit without a MERGED link must be refused"
    );

    // Observed merge transitions InProgress -> Review with the fresh
    // snapshot stored.
    let submitted = store
        .submit_work_on_pr_merge(
            &claimed.id,
            claimed.version,
            "auto-submitted by GitHub merge observation",
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-ms-4", "submit-merge-ok", "unix-ms:5"),
        )
        .expect("auto-submit on merge");
    assert_eq!(submitted.phase, WorkPhase::Review);
    assert_eq!(
        submitted.result_summary.as_deref(),
        Some("auto-submitted by GitHub merge observation")
    );
    assert_eq!(submitted.github_links[0].status.as_deref(), Some("MERGED"));
    assert_eq!(
        submitted.github_links[0].ci_status.as_deref(),
        Some("success")
    );

    // A review Work is not auto-submittable again.
    let re_submit = store.submit_work_on_pr_merge(
        &submitted.id,
        submitted.version,
        "again",
        vec![test_github_link("MERGED", Some("success"))],
        host_work_context("we-ms-5", "submit-merge-again", "unix-ms:6"),
    );
    assert!(
        re_submit.is_err()
            && re_submit
                .unwrap_err()
                .to_string()
                .contains("required state"),
        "review Work must not be auto-submitted twice"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
