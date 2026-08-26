use super::*;

#[test]
fn member_owned_work_records_require_the_exact_active_execution_binding() {
    let harness = TestStore::new("exact-work-binding-records");
    let team_id = seed_active_team_work(&harness.store, "exact-binding", "work-1");
    let worker = member_actor("worker");
    harness
        .store
        .transition_current_team_member_lifecycle(
            &context(worker.clone(), "member_run.close", "close-old-run", 1),
            "runtime-worker",
            CurrentTeamMemberLifecycleTransition::Close,
            "t4",
        )
        .expect("close predecessor MemberRun");
    let before = harness.store.canonical_operations().unwrap().len();
    let mut progress = report("closed-progress", WorkReportKind::Progress, &worker);
    progress.work_revision = 3;
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(worker.clone(), "report.create", "closed-progress", 0),
                    &team_id,
                    progress,
                )
                .expect_err("closed member must require an exact active Work rebind")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    let finding = WorkFinding {
        id: "closed-finding".into(),
        work_id: "work-1".into(),
        work_revision: 3,
        kind: WorkFindingKind::Discovery,
        summary: "closed member cannot author before rebind".into(),
        detail_markdown: "exact active binding is no longer active".into(),
        affected_work_refs: Vec::new(),
        reusable_asset_refs: Vec::new(),
        invalidated_assumptions: Vec::new(),
        evidence_refs: Vec::new(),
        confidence: Confidence::High,
        reported_by: worker.clone(),
        created_at: "t5".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_finding(
                    &context(worker.clone(), "finding.create", "closed-finding", 0),
                    &team_id,
                    finding,
                )
                .expect_err("closed member finding must require explicit Work rebind")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    let failure = FailureAnalysis {
        id: "closed-failure".into(),
        work_id: "work-1".into(),
        work_revision: 3,
        member_run_id: Some("runtime-worker".into()),
        candidate: None,
        observed_failure: "closed member attempted stale binding".into(),
        impact: "none".into(),
        primary_cause_status: PrimaryCauseStatus::Confirmed,
        primary_cause: Some("missing explicit rebind".into()),
        contributing_causes: Vec::new(),
        attempts_already_made: Vec::new(),
        last_safe_checkpoint: None,
        retry_safety: RetrySafety::Safe,
        side_effect_summary: Some("none".into()),
        recovery_options: vec!["rebind".into()],
        recommended_host_decision: "rebind explicitly".into(),
        evidence_refs: Vec::new(),
        confidence: Confidence::High,
        reported_by: worker.clone(),
        created_at: "t5".into(),
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_failure_analysis(
                    &context(worker, "failure.create", "closed-failure", 0),
                    &team_id,
                    failure,
                )
                .expect_err("closed member failure must require explicit Work rebind")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "stale WorkExecutionBinding rejection must have zero canonical side effects"
    );
}
