use super::*;

fn pull_request(status: &str) -> firm_core::GitHubLink {
    firm_core::GitHubLink {
        kind: firm_core::GitHubLinkKind::PullRequest,
        owner: "example".into(),
        repo: "project".into(),
        number: 11,
        url: "https://github.com/example/project/pull/11".into(),
        status: Some(status.into()),
        ci_status: Some("success".into()),
        ci_url: None,
    }
}

/// Terminal Work is immutable (docs/current/product/agent-team-works.md).
/// Every Host-side writer answers with the one kernel code instead of its own
/// bespoke sentence, so an operator can recognize the refusal class once.
#[test]
fn closed_work_refuses_every_host_mutation_with_one_terminal_code() {
    let (root, store, run, _, _) = work_test_fixture("terminal-immutability");
    let mut draft = unassigned_test_work(&run.id, "terminal-immutability-work");
    draft.github_links = vec![pull_request("OPEN")];
    let created = store
        .insert_work(
            draft,
            host_work_context("terminal-create", "terminal-create", "unix-ms:2"),
        )
        .expect("create Work");
    let peer = store
        .insert_work(
            unassigned_test_work(&run.id, "terminal-immutability-peer"),
            host_work_context("terminal-peer", "terminal-peer", "unix-ms:2"),
        )
        .expect("create peer Work");
    let closed = store
        .cancel_work(
            &created.id,
            created.version,
            "Host closed the responsibility",
            host_work_context("terminal-cancel", "terminal-cancel", "unix-ms:3"),
        )
        .expect("cancel Work");
    assert!(closed.is_terminal());

    let execution_space_id = store
        .current_team_run_execution_space(&run)
        .expect("resolve the TeamRun Execution Space");
    let daemon = store
        .seed_machine_authority_for_test(
            &run.execution_node_id,
            "terminal-immutability-daemon",
            "terminal-immutability-instance",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis() as u64,
            60_000,
        )
        .expect("acquire exact NodeDaemon");

    let refusals: [(&str, StoreError); 7] = [
        (
            "assign",
            store
                .assign_work_to_membership(
                    &closed.id,
                    closed.version,
                    "membership-any",
                    &execution_space_id,
                    host_work_context("terminal-assign", "terminal-assign", "unix-ms:4"),
                )
                .expect_err("terminal Work cannot be reassigned"),
        ),
        (
            "retarget",
            store
                .retarget_work_execution(
                    &closed.id,
                    closed.version,
                    "tr-successor",
                    host_work_context("terminal-retarget", "terminal-retarget", "unix-ms:5"),
                )
                .expect_err("terminal Work cannot be retargeted"),
        ),
        (
            "redeliver",
            store
                .redeliver_work_to_current_session(
                    &closed.id,
                    closed.version,
                    &execution_space_id,
                    Some("retry"),
                    host_work_context("terminal-redeliver", "terminal-redeliver", "unix-ms:6"),
                )
                .expect_err("terminal Work cannot be redelivered"),
        ),
        (
            "recover-lost-execution",
            store
                .recover_lost_work_execution(
                    &closed.id,
                    closed.version,
                    &execution_space_id,
                    Some("lost"),
                    host_work_context("terminal-recover", "terminal-recover", "unix-ms:7"),
                )
                .expect_err("terminal Work cannot be recovered"),
        ),
        (
            "cancel",
            store
                .cancel_work(
                    &closed.id,
                    closed.version,
                    "again",
                    host_work_context("terminal-recancel", "terminal-recancel", "unix-ms:8"),
                )
                .expect_err("terminal Work cannot be cancelled twice"),
        ),
        (
            "replace-dependencies",
            store
                .replace_work_dependencies(
                    &closed.id,
                    closed.version,
                    vec![peer.id.clone()],
                    host_work_context("terminal-deps", "terminal-deps", "unix-ms:9"),
                )
                .expect_err("terminal Work keeps its dependency set"),
        ),
        (
            "poll-github-ci",
            store
                .update_work_github_links(
                    &closed.id,
                    closed.version,
                    vec![pull_request("MERGED")],
                    &execution_space_id,
                    &daemon,
                    WorkCommandContext {
                        event_id: "terminal-github".into(),
                        performed_by_actor: TeamActorRef {
                            kind: TeamActorKind::Service,
                            id: daemon.daemon_id.clone(),
                            display_name: None,
                            authn_source: Some("test_node_daemon".into()),
                        },
                        authority_actor: run.host_actor.clone(),
                        causation_ref: None,
                        idempotency_key: "terminal-github".into(),
                        created_at: "unix-ms:11".into(),
                        duplicate_ok: false,
                    },
                )
                .expect_err("terminal Work cannot grow a new evidence revision"),
        ),
    ];

    for (verb, error) in &refusals {
        assert!(
            error.to_string().contains(crate::WORK_TERMINAL_IMMUTABLE),
            "{verb} must refuse a closed Work with the one terminal code: {error}"
        );
    }
    assert_eq!(
        store
            .latest_works()
            .expect("read Work")
            .into_iter()
            .find(|work| work.id == closed.id)
            .expect("closed Work")
            .version,
        closed.version,
        "no refused command appended a revision to the terminal Work"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

/// The `Updated` writers carry no lifecycle change, so they are the easiest
/// place for a terminal Work to grow a new version. The GitHub evidence
/// refresh is pinned here; the two ledger-shaped `Updated` writers
/// (reconcile-projection and migrate-responsibility) are pinned in
/// `tests/work_responsibility_cutover.rs`, where a raw pre-cutover terminal
/// row can be seeded.
#[test]
fn terminal_work_refuses_external_evidence_updates() {
    let (root, store, run, _, _) = work_test_fixture("terminal-updated-writers");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "terminal-updated-work"),
            host_work_context("updated-create", "updated-create", "unix-ms:2"),
        )
        .expect("create Work");
    let closed = store
        .cancel_work(
            &created.id,
            created.version,
            "Host closed the responsibility",
            host_work_context("updated-cancel", "updated-cancel", "unix-ms:3"),
        )
        .expect("cancel Work");

    let execution_space_id = store
        .current_team_run_execution_space(&run)
        .expect("resolve the TeamRun Execution Space");
    let daemon = store
        .seed_machine_authority_for_test(
            &run.execution_node_id,
            "terminal-updated-daemon",
            "terminal-updated-instance",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis() as u64,
            60_000,
        )
        .expect("acquire exact NodeDaemon");
    let evidence = store
        .update_work_github_links(
            &closed.id,
            closed.version,
            vec![pull_request("MERGED")],
            &execution_space_id,
            &daemon,
            WorkCommandContext {
                event_id: "updated-github".into(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::Service,
                    id: daemon.daemon_id.clone(),
                    display_name: None,
                    authn_source: Some("test_node_daemon".into()),
                },
                authority_actor: run.host_actor.clone(),
                causation_ref: None,
                idempotency_key: "updated-github".into(),
                created_at: "unix-ms:5".into(),
                duplicate_ok: false,
            },
        )
        .expect_err("external CI evidence cannot advance a closed Work");
    assert!(
        evidence
            .to_string()
            .contains(crate::WORK_TERMINAL_IMMUTABLE),
        "error: {evidence}"
    );
    assert_eq!(
        store
            .latest_works()
            .expect("read Work")
            .into_iter()
            .find(|work| work.id == closed.id)
            .expect("closed Work")
            .version,
        closed.version,
        "a refused Updated writer leaves the terminal revision untouched"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
