use super::*;

#[test]
fn result_and_failure_reports_require_their_risk_evidence() {
    let harness = TestStore::new("reports");
    let team_id = seed_active_team_work(&harness.store, "reports", "work-1");
    let worker = member_actor("worker");
    let before = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "result-missing", 0),
                    &team_id,
                    {
                        let mut report = report("result-missing", WorkReportKind::Result, &worker);
                        report.work_revision = 4;
                        report
                    },
                )
                .expect_err("result without candidate evidence must fail")
        ),
        TrustErrorCode::ReportEvidenceMissing
    );
    assert_eq!(harness.store.canonical_operations().unwrap().len(), before);

    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "failure-missing", 0),
                    &team_id,
                    report("failure-missing", WorkReportKind::Failure, &worker),
                )
                .expect_err("failure without analysis must fail")
        ),
        TrustErrorCode::FailureAnalysisMissing
    );
    let mut missing_reference = report("failure-missing-ref", WorkReportKind::Failure, &worker);
    missing_reference.failure_analysis_ref = Some("analysis-missing".into());
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "failure-missing-ref", 0,),
                    &team_id,
                    missing_reference,
                )
                .expect_err("failure analysis reference must resolve")
        ),
        TrustErrorCode::FailureAnalysisMissing
    );
    harness
        .store
        .create_trust_failure_analysis(
            &context(worker.clone(), "failure_analysis.create", "analysis", 0),
            &team_id,
            FailureAnalysis {
                id: "analysis-1".into(),
                work_id: "work-1".into(),
                work_revision: 3,
                member_run_id: Some("runtime-worker".into()),
                candidate: None,
                observed_failure: "provider exited".into(),
                impact: "work incomplete".into(),
                primary_cause_status: PrimaryCauseStatus::Suspected,
                primary_cause: Some("provider failure".into()),
                contributing_causes: Vec::new(),
                attempts_already_made: vec!["one retry".into()],
                last_safe_checkpoint: Some("base".into()),
                retry_safety: RetrySafety::Safe,
                side_effect_summary: Some("none".into()),
                recovery_options: vec!["resume".into()],
                recommended_host_decision: "retry".into(),
                evidence_refs: vec!["evidence://provider-log".into()],
                confidence: Confidence::Medium,
                reported_by: worker.clone(),
                created_at: "t2".into(),
            },
        )
        .expect("create failure analysis");
    let mut failure = report("failure-ok", WorkReportKind::Failure, &worker);
    failure.failure_analysis_ref = Some("analysis-1".into());
    harness
        .store
        .create_trust_work_report(
            &context(worker.clone(), "report.create", "failure-ok", 0),
            &team_id,
            failure,
        )
        .expect("failure report with analysis reference");

    let mut result = report("result-ok", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "0123456789abcdef".into(),
    };
    result.candidate_fingerprint = Some(canonical_json_fingerprint(
        &serde_json::to_value(&candidate).expect("serialize candidate"),
    ));
    result.candidate = Some(candidate);
    result.evidence_refs = vec!["evidence://checks".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "result-ok", 0),
            &team_id,
            result,
        )
        .expect("evidence-backed result atomically submits Work");
    let submitted = harness
        .store
        .latest_works()
        .expect("read Work")
        .into_iter()
        .find(|work| work.id == "work-1")
        .expect("submitted Work");
    assert_eq!(submitted.version, 4);
    assert_eq!(submitted.phase, WorkPhase::Review);
}
