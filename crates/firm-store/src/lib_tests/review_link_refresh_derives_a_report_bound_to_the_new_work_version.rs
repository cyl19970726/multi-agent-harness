use super::*;

#[test]
#[ignore = "legacy Work acceptance route is retired; canonical report-bound acceptance is covered by member_execution_trust"]
fn review_link_refresh_derives_a_report_bound_to_the_new_work_version() {
    let (root, store, run, member, _) = work_test_fixture("github-review-report-refresh");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "github-review-report-refresh-1"),
            host_work_context("we-grr-1", "create-grr", "unix-ms:2"),
        )
        .expect("create Work");
    let claimed = store
        .claim_work(
            &created.id,
            created.version,
            &member.id,
            member_work_context(&member.id, "we-grr-2", "claim-grr", "unix-ms:3"),
        )
        .expect("claim Work");
    let submitted = store
        .submit_work_with_revision_and_links(
            &claimed.id,
            claimed.version,
            &member.id,
            "candidate",
            vec!["artifact://candidate".into()],
            vec!["check://candidate".into()],
            Vec::new(),
            Some("base-sha".into()),
            Some("candidate-sha".into()),
            member_work_context(&member.id, "we-grr-3", "submit-grr", "unix-ms:4"),
        )
        .expect("submit Work");
    let refreshed = store
        .update_work_github_links(
            &submitted.id,
            submitted.version,
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-grr-4", "refresh-grr", "unix-ms:5"),
        )
        .expect("refresh review links");

    let reports = store.work_reports().expect("reports");
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].work_version, submitted.version);
    assert_eq!(reports[1].work_version, refreshed.version);
    assert_eq!(reports[1].candidate_revision, "candidate-sha");
    assert_eq!(reports[1].report_revision, reports[0].report_revision + 1);
    let evidence = store.work_evidence().expect("evidence");
    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[1].work_report_id, reports[1].id);
    assert_eq!(evidence[1].work_version, refreshed.version);

    let accepted = store
        .accept_work(
            &refreshed.id,
            refreshed.version,
            host_work_context("we-grr-5", "accept-grr", "unix-ms:6"),
        )
        .expect("current derived report authorizes acceptance");
    assert_eq!(accepted.phase, WorkPhase::Closed);
    assert_eq!(
        store.work_operational_decisions().unwrap()[0]
            .work_report_id
            .as_deref(),
        Some(reports[1].id.as_str())
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
