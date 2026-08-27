use super::*;

fn pull_request(status: &str) -> firm_core::GitHubLink {
    firm_core::GitHubLink {
        kind: firm_core::GitHubLinkKind::PullRequest,
        owner: "example".into(),
        repo: "project".into(),
        number: 7,
        url: "https://github.com/example/project/pull/7".into(),
        status: Some(status.into()),
        ci_status: Some("success".into()),
        ci_url: Some("https://github.com/example/project/actions/runs/7".into()),
    }
}

#[test]
fn update_work_github_links_refreshes_only_evidence_without_churn() {
    let (root, store, run, _, _) = work_test_fixture("github-evidence-refresh");
    let mut draft = unassigned_test_work(&run.id, "github-evidence-refresh-1");
    draft.github_links = vec![pull_request("OPEN")];
    let created = store
        .insert_work(
            draft,
            host_work_context("github-create", "github-create", "unix-ms:2"),
        )
        .expect("create GitHub-linked Work");
    let daemon = store
        .acquire_node_daemon_lease(
            &run.execution_node_id,
            "github-evidence-daemon",
            "github-evidence-instance",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis() as u64,
            60_000,
        )
        .expect("acquire exact NodeDaemon");
    let evidence_context = |event_id: &str, key: &str, at: &str| WorkCommandContext {
        event_id: event_id.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Service,
            id: daemon.daemon_id.clone(),
            display_name: None,
            authn_source: Some("test_node_daemon".into()),
        },
        authority_actor: run.host_actor.clone(),
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: at.into(),
        duplicate_ok: false,
    };
    let before = store.work_operations().unwrap();
    let unchanged = store
        .update_work_github_links(
            &created.id,
            created.version,
            created.github_links.clone(),
            "unit-test-space",
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation,
            evidence_context("github-noop", "github-noop", "unix-ms:3"),
        )
        .expect("unchanged refresh");
    assert_eq!(unchanged, created);
    assert_eq!(store.work_operations().unwrap(), before);

    let refreshed = store
        .update_work_github_links(
            &created.id,
            created.version,
            vec![pull_request("MERGED")],
            "unit-test-space",
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation,
            evidence_context("github-refresh", "github-refresh", "unix-ms:4"),
        )
        .expect("refresh external evidence");
    assert_eq!(refreshed.phase, created.phase);
    assert_eq!(refreshed.version, created.version + 1);
    assert_eq!(refreshed.github_links[0].status.as_deref(), Some("MERGED"));
    let operations = store.work_operations().unwrap();
    assert_eq!(operations.len(), before.len() + 1);
    assert_eq!(
        operations.last().unwrap().event.kind,
        WorkEventKind::Updated
    );
    assert!(operations.last().unwrap().reports.is_empty());
    assert!(store.host_attentions().unwrap().is_empty());

    let stale_replay = store
        .update_work_github_links(
            &created.id,
            created.version,
            vec![pull_request("MERGED")],
            "unit-test-space",
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation + 1,
            evidence_context("github-refresh-retry", "github-refresh", "unix-ms:5"),
        )
        .expect_err("replay must still prove the exact current daemon generation");
    assert!(
        stale_replay.to_string().contains("GENERATION_FENCED"),
        "error: {stale_replay}"
    );

    let replayed = store
        .update_work_github_links(
            &created.id,
            created.version,
            vec![pull_request("MERGED")],
            "unit-test-space",
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation,
            evidence_context("github-refresh-retry", "github-refresh", "unix-ms:5"),
        )
        .expect("same authenticated request replays before the old-version CAS");
    assert_eq!(replayed, refreshed);
    assert_eq!(store.work_operations().unwrap(), operations);

    let changed_payload = store
        .update_work_github_links(
            &created.id,
            created.version,
            vec![pull_request("CLOSED")],
            "unit-test-space",
            &run.execution_node_id,
            &daemon.daemon_id,
            daemon.generation,
            evidence_context("github-refresh-drift", "github-refresh", "unix-ms:6"),
        )
        .expect_err("one idempotency key cannot replace its GitHub evidence payload");
    assert!(
        changed_payload.to_string().contains("IDEMPOTENCY_CONFLICT"),
        "error: {changed_payload}"
    );
    assert_eq!(store.work_operations().unwrap(), operations);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
