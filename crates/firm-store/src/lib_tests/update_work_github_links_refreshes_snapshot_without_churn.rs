use super::*;

#[test]
fn update_work_github_links_refreshes_snapshot_without_churn() {
    let (root, store, run, _member, _) = work_test_fixture("github-update");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "github-update-1"),
            host_work_context("we-gu-1", "create-github-update", "unix-ms:2"),
        )
        .expect("create Work");
    assert!(created.github_links.is_empty());

    let refreshed = store
        .update_work_github_links(
            &created.id,
            created.version,
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-gu-2", "poll-github-update-1", "unix-ms:3"),
        )
        .expect("refresh snapshot");
    assert_eq!(refreshed.version, created.version + 1);
    assert_eq!(refreshed.github_links.len(), 1);
    assert_eq!(
        refreshed.github_links[0].ci_status.as_deref(),
        Some("success")
    );

    // Steady-state poll with identical links must not churn versions.
    let unchanged = store
        .update_work_github_links(
            &created.id,
            refreshed.version,
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-gu-3", "poll-github-update-2", "unix-ms:4"),
        )
        .expect("steady-state poll is a no-op");
    assert_eq!(unchanged.version, refreshed.version);

    // A changed CI outcome appends one more Updated operation.
    let re_polled = store
        .update_work_github_links(
            &created.id,
            unchanged.version,
            vec![test_github_link("MERGED", Some("failure"))],
            host_work_context("we-gu-4", "poll-github-update-3", "unix-ms:5"),
        )
        .expect("changed CI refreshes");
    assert_eq!(re_polled.version, unchanged.version + 1);
    assert_eq!(
        re_polled.github_links[0].ci_status.as_deref(),
        Some("failure")
    );

    // Stale expected version is rejected.
    let stale = store.update_work_github_links(
        &created.id,
        created.version,
        vec![test_github_link("MERGED", Some("success"))],
        host_work_context("we-gu-5", "poll-github-update-4", "unix-ms:6"),
    );
    assert!(
        stale.is_err() && stale.unwrap_err().to_string().contains("VERSION_CONFLICT"),
        "stale poll must conflict"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
