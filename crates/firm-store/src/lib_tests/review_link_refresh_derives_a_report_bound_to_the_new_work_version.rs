use super::*;

fn merged_link() -> firm_core::GitHubLink {
    firm_core::GitHubLink {
        kind: firm_core::GitHubLinkKind::PullRequest,
        owner: "example".into(),
        repo: "project".into(),
        number: 17,
        url: "https://github.com/example/project/pull/17".into(),
        status: Some("MERGED".into()),
        ci_status: Some("success".into()),
        ci_url: Some("https://github.com/example/project/actions/runs/17".into()),
    }
}

fn service_refresh_context(
    run: &AgentTeamRun,
    daemon: &firm_core::NodeDaemonLease,
    key: &str,
) -> WorkCommandContext {
    WorkCommandContext {
        event_id: format!("event-{key}"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Service,
            id: daemon.daemon_id.clone(),
            display_name: None,
            authn_source: Some("test_node_daemon".into()),
        },
        authority_actor: run.host_actor.clone(),
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: "unix-ms:6".into(),
        duplicate_ok: false,
    }
}

#[test]
fn review_link_refresh_preserves_the_exact_result_and_host_acceptance() {
    let (root, store, run, member, _) = work_test_fixture("github-review-report-refresh");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "github-review-report-refresh-1"),
            host_work_context("refresh-create", "refresh-create", "unix-ms:2"),
        )
        .expect("create Work");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "refresh-assign",
        "refresh-assign",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "refresh-start",
        "refresh-start",
        "unix-ms:4",
    );
    let submitted = submit_started_work_for_test(
        &store,
        &active,
        &member,
        "refresh-result",
        "candidate",
        (
            vec!["artifact://candidate".into()],
            vec!["check://candidate".into()],
        ),
        "unix-ms:5",
    );
    let report_count = store
        .canonical_operations()
        .unwrap()
        .into_iter()
        .filter(|operation| operation.event.aggregate_kind == "work_report")
        .count();
    let daemon = store
        .latest_node_daemon_lease(&run.execution_node_id)
        .unwrap()
        .expect("test NodeDaemon");
    let refreshed = store
        .update_work_github_links(
            &submitted.id,
            submitted.version,
            vec![merged_link()],
            "unit-test-space",
            &daemon,
            service_refresh_context(&run, &daemon, "refresh-github"),
        )
        .expect("refresh GitHub evidence");
    assert_eq!(refreshed.phase, WorkPhase::Review);
    assert_eq!(
        store
            .canonical_operations()
            .unwrap()
            .into_iter()
            .filter(|operation| operation.event.aggregate_kind == "work_report")
            .count(),
        report_count,
        "external evidence must not derive or rewrite a Member Result"
    );
    let accepted = accept_result_for_test(
        &store,
        &refreshed,
        "refresh-result",
        "refresh-accept",
        "unix-ms:7",
    );
    assert_eq!(accepted.phase, WorkPhase::Closed);
    assert_eq!(accepted.resolution, Some(WorkResolution::Accepted));
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn non_evidence_review_revision_drift_cannot_reuse_an_older_result() {
    let (root, store, run, member, _) = work_test_fixture("review-semantic-drift");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "review-semantic-drift-1"),
            host_work_context("drift-create", "drift-create", "unix-ms:2"),
        )
        .expect("create Work");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "drift-assign",
        "drift-assign",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "drift-start",
        "drift-start",
        "unix-ms:4",
    );
    let submitted = submit_started_work_for_test(
        &store,
        &active,
        &member,
        "drift-result",
        "candidate",
        (Vec::new(), Vec::new()),
        "unix-ms:5",
    );
    let drifted = {
        let _lock = store.acquire_write_lock().expect("lock test Store");
        let current = store
            .current_work_unlocked(&submitted.id, submitted.version)
            .expect("current submitted Work");
        let mut next = current.clone();
        next.version += 1;
        next.updated_at = "unix-ms:6".into();
        store
            .append_work_transition_with_payload_unlocked(
                current,
                next,
                WorkEventKind::Updated,
                host_work_context("semantic-drift", "semantic-drift", "unix-ms:6"),
                serde_json::json!({"reason": "semantic_review_edit"}),
            )
            .expect("append non-evidence semantic drift")
    };
    let candidate = firm_core::agentfirm_api::CandidateRef {
        kind: firm_core::agentfirm_api::CandidateKind::GitCommit,
        value: "candidate-drift-result".into(),
    };
    let fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(candidate).expect("candidate JSON"));
    let error = store
        .accept_trust_work(
            &firm_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: firm_core::agentfirm_api::ActorRef {
                    kind: firm_core::agentfirm_api::ActorKind::Human,
                    id: "reviewer".into(),
                },
                authority_actor: None,
                command_name: "test.work.accept".into(),
                idempotency_key: "semantic-drift-accept".into(),
                expected_version: drifted.version,
                request_fingerprint: None,
            },
            drifted.accountable_team_id.as_deref().expect("team id"),
            &drifted.id,
            "report-drift-result",
            &fingerprint,
            "unix-ms:7",
        )
        .expect_err("non-evidence semantic drift must stale the older Result");
    assert!(
        error.to_string().contains("REPORT_EVIDENCE_MISSING"),
        "error: {error}"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
