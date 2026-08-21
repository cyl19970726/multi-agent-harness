use super::*;

#[test]
fn exact_result_report_submits_and_accepts_work_in_canonical_operations() {
    let harness = TestStore::new("accept-work");
    let team_id = seed_active_team_work(&harness.store, "accept-work", "work-1");
    let worker = member_actor("worker");
    let host = human("host");
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(&candidate).expect("serialize candidate"));
    let mut result = report("report-accept", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    result.candidate = Some(candidate);
    result.candidate_fingerprint = Some(candidate_fingerprint.clone());
    result.evidence_refs = vec!["evidence://exact-candidate".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "report-accept", 0),
            &team_id,
            result,
        )
        .expect("result submission");
    let before_rejected = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .accept_trust_work(
                    &context(host.clone(), "work.accept", "accept-stale", 4),
                    &team_id,
                    "work-1",
                    "report-accept",
                    "sha256:stale",
                    "t5",
                )
                .expect_err("stale Candidate must not accept Work")
        ),
        TrustErrorCode::ReportEvidenceMissing
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_rejected,
        "rejected acceptance has zero side effects"
    );
    let command = context(host, "work.accept", "accept-exact", 4);
    let accepted = harness
        .store
        .accept_trust_work(
            &command,
            &team_id,
            "work-1",
            "report-accept",
            &candidate_fingerprint,
            "t5",
        )
        .expect("exact Candidate acceptance");
    assert_eq!(accepted.projection.phase, WorkPhase::Closed);
    assert_eq!(accepted.projection.version, 5);
    let replay = harness
        .store
        .accept_trust_work(
            &command,
            &team_id,
            "work-1",
            "report-accept",
            &candidate_fingerprint,
            "t5",
        )
        .expect("accept replay");
    assert!(replay.replayed);
    assert_eq!(replay.event.id, accepted.event.id);
}
