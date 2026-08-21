use super::*;

#[test]
fn remote_fact_is_redacted_digest_bound_and_target_scoped() {
    let test = TestStore::new("publication");
    let target = active_delegation(&test.store);
    let mut native_target = target.clone();
    native_target.work_revision += 1;
    native_target.work_event_id = "event-b-2".into();
    native_target.digest = format!("sha256:{:064x}", 2);
    let fact = serde_json::json!({
        "submitted_work_revision": 1,
        "outcome": "implemented",
        "checks": ["check:unit"],
        "evidence": ["artifact:diff"],
        "target_host_decision": "accepted"
    });
    let digest = canonical_json_fingerprint(&fact);
    let publication = RemoteFactPublication {
        id: "publication-1".into(),
        company_id: "company-1".into(),
        delegation_id: "delegation-1".into(),
        origin_node_id: "node-b".into(),
        origin_team_id: "team-b".into(),
        fact_work_ref: target.clone(),
        native_fact_work_ref: native_target,
        delegation_source_work_ref: source_attestation().source_work_ref,
        fact_kind: RemoteFactKind::Report,
        fact_id: "report-b-1".into(),
        fact_revision: 1,
        fact_digest: digest.clone(),
        summary: "target result is ready for source integration".into(),
        classification: "team-visible".into(),
        snapshot: RemoteFactSnapshot {
            publication_id: "publication-1".into(),
            fact_schema: "agentfirm.work-report.v1".into(),
            canonical_redacted_fact: fact,
            canonical_digest: digest,
        },
        artifact_refs: vec!["artifact:diff".into()],
        evidence_refs: vec!["check:unit".into()],
        operational_decision_ref: None,
        created_by: actor(ActorKind::AgentMember, "member-b"),
        created_at: "2026-08-11T00:00:05Z".into(),
        retain_until: "2026-09-10T00:00:05Z".into(),
    };
    let publish_context = context(
        publication.created_by.clone(),
        "remote_fact.publish",
        "publish-1",
        0,
    );
    let mut forged = publication.clone();
    forged.fact_digest = format!("sha256:{:064x}", 999);
    let before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .publish_remote_fact(
            &publish_context,
            &forged,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap(), before);

    let mut wrong_target_work = publication.clone();
    wrong_target_work.id = "publication-wrong-target-work".into();
    wrong_target_work.fact_work_ref.work_id = "work-b-sibling".into();
    wrong_target_work.snapshot.publication_id = wrong_target_work.id.clone();
    let before = test.store.collaboration_operations().unwrap();
    assert!(test
        .store
        .publish_remote_fact(
            &context(
                wrong_target_work.created_by.clone(),
                "remote_fact.publish",
                "publish-wrong-target-work",
                0,
            ),
            &wrong_target_work,
            std::slice::from_ref(&wrong_target_work.created_by),
            &placement(13),
        )
        .is_err());
    assert_eq!(
        test.store.collaboration_operations().unwrap(),
        before,
        "a fact for a sibling target Work must not append any collaboration operation"
    );

    let published = test
        .store
        .publish_remote_fact(
            &publish_context,
            &publication,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .expect("publish exact redacted snapshot");
    assert!(!published.replayed);
    let replay = test
        .store
        .publish_remote_fact(
            &publish_context,
            &publication,
            std::slice::from_ref(&publication.created_by),
            &placement(13),
        )
        .expect("publication replay");
    assert!(replay.replayed);

    let operational_decision = WorkOperationalDecisionRef {
        decision_id: "work-decision-b-1".into(),
        work_ref: publication.native_fact_work_ref.clone(),
        decision_revision: 1,
        digest: format!("sha256:{:064x}", 77),
    };
    let available = test
        .store
        .mark_delegation_result_available(
            &context(
                authority().target_host.clone(),
                "delegation.result_available",
                "result-available-1",
                3,
            ),
            "delegation-1",
            &publication.id,
            &operational_decision,
            &authority(),
            &placement(13),
        )
        .expect("target Host exposes accepted target result");
    assert_eq!(available.projection.state, DelegationState::ResultAvailable);
    assert_eq!(available.projection.source_work_ref.work_revision, 9);
    let operations_after_available = test.store.collaboration_operations().unwrap();
    let available_replay = test
        .store
        .mark_delegation_result_available(
            &context(
                authority().target_host.clone(),
                "delegation.result_available",
                "result-available-1",
                3,
            ),
            "delegation-1",
            &publication.id,
            &operational_decision,
            &authority(),
            &placement(13),
        )
        .expect("exact result-available replay");
    assert!(available_replay.replayed);
    assert_eq!(
        test.store.collaboration_operations().unwrap(),
        operations_after_available,
        "exact receipt replay must not append a second Delegation transition"
    );

    let integrated_source = work_ref("node-a", "team-a", "work-a", 10);
    let completed = test
        .store
        .complete_delegation_after_source_integration(
            &context(
                authority().source_host.clone(),
                "delegation.complete_after_source_integration",
                "source-integrated-1",
                4,
            ),
            "delegation-1",
            &integrated_source,
            "source-work-event-accepted-10",
            &authority(),
        )
        .expect("source Host independently integrates and closes relationship");
    assert_eq!(completed.projection.state, DelegationState::Terminal);
    assert_eq!(
        completed.projection.terminal_outcome,
        Some(DelegationTerminalOutcome::Completed)
    );
    // The relationship stores integration evidence but never rewrites source
    // Work authority into its own projection.
    assert_eq!(completed.projection.source_work_ref.work_revision, 9);
}
