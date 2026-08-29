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
                    &context(worker.clone(), "failure.create", "closed-failure", 0),
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

    let reopened = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(human("host"), "member_run.reopen", "reopen-exact-run", 2),
            "runtime-worker",
            CurrentTeamMemberLifecycleTransition::Reopen,
            "t6",
        )
        .expect("reopen the same MemberRun at generation two");
    assert_eq!(reopened.canonical.projection.runtime_generation, 2);

    let work_before_settlement = harness
        .store
        .latest_works()
        .expect("Works before reopened settlement")
        .into_iter()
        .find(|work| work.id == "work-1")
        .expect("original Work before reopened settlement");
    let operations_before_progress = harness
        .store
        .canonical_operations()
        .expect("operations before reopened progress")
        .len();
    let mut stale_progress = report(
        "generation-2-stale-progress",
        WorkReportKind::Progress,
        &worker,
    );
    stale_progress.work_revision = work_before_settlement.version;
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_work_report(
                    &context(
                        worker.clone(),
                        "report.create",
                        "generation-2-stale-progress",
                        0,
                    ),
                    &team_id,
                    stale_progress,
                )
                .expect_err("Reopen must not revive ordinary stale Work authoring")
        ),
        TrustErrorCode::MemberRunGenerationFenced
    );
    assert_eq!(
        harness
            .store
            .canonical_operations()
            .expect("operations after reopened progress rejection")
            .len(),
        operations_before_progress,
        "ordinary stale authoring must remain zero-write"
    );

    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let mut settlement_result = report(
        "generation-2-result-settlement",
        WorkReportKind::Result,
        &worker,
    );
    settlement_result.work_revision = work_before_settlement.version + 1;
    settlement_result.candidate_fingerprint = Some(canonical_json_fingerprint(
        &serde_json::to_value(&candidate).expect("serialize candidate"),
    ));
    settlement_result.candidate = Some(candidate);
    settlement_result.evidence_refs = vec!["evidence://same-session-result".into()];
    harness
        .store
        .create_trust_work_report(
            &context(
                worker.clone(),
                "report.create",
                "generation-2-result-settlement",
                0,
            ),
            &team_id,
            settlement_result,
        )
        .expect("same-session Reopen may settle exact ProviderReceived Work");
    let submitted_work = harness
        .store
        .latest_works()
        .expect("Works after settlement")
        .into_iter()
        .find(|work| work.id == "work-1")
        .expect("submitted Work");
    assert_eq!(submitted_work.phase, WorkPhase::Review);
    let settled_binding = harness
        .store
        .fabric_work_execution_bindings(SPACE)
        .expect("bindings after settlement")
        .into_iter()
        .find(|binding| binding.work_id == "work-1")
        .expect("settled binding");
    assert_eq!(settled_binding.status, WorkExecutionBindingStatus::Released);

    let run = harness
        .store
        .team_runs()
        .expect("TeamRuns")
        .into_iter()
        .find(|run| run.agent_team_id == team_id)
        .expect("exact TeamRun");
    seed_team_work_from_run(&harness.store, &run, "work-generation-2");
    harness
        .store
        .assign_work_to_membership(
            "work-generation-2",
            1,
            "membership-team-exact-binding-worker",
            SPACE,
            WorkCommandContext {
                event_id: "assign-generation-2".into(),
                performed_by_actor: harness
                    .store
                    .exact_team_run_host_actor(&run.id)
                    .expect("exact Host"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "assign-generation-2".into(),
                created_at: "t7".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign generation-two Work responsibility");
    let session = harness
        .store
        .fabric_agent_sessions(SPACE)
        .expect("AgentSessions")
        .into_iter()
        .find(|session| session.id == "session-runtime-worker")
        .expect("same AgentSession generation");
    let runtime_binding = RuntimeCommandBinding {
        target_member_run_id: Some("runtime-worker".into()),
        target_member_run_generation: Some(2),
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
        ..Default::default()
    };
    harness
        .store
        .bind_responsible_work_execution(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.bind",
                "binding-generation-2",
                0,
            ),
            &runtime_binding,
            WorkExecutionBinding {
                id: "binding-generation-2".into(),
                work_id: "work-generation-2".into(),
                work_revision: 2,
                team_id: team_id.clone(),
                team_membership_id: "membership-team-exact-binding-worker".into(),
                agent_member_id: "worker".into(),
                agent_session_id: session.id.clone(),
                agent_session_generation: session.runtime_generation,
                delivery_id: "work-delivery:work-generation-2:1".into(),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                bound_at: "t8".into(),
                ended_at: None,
            },
        )
        .expect("bind exact reopened MemberRun generation");
    harness
        .store
        .claim_work_for_provider(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.claim",
                "claim-generation-2",
                0,
            ),
            "work-delivery:work-generation-2:1",
            NODE,
            "daemon-test",
            1,
            "claim-generation-2",
            RuntimeDispatchMode::QueueOnly,
            "t8.5",
        )
        .expect("claim generation-two Work delivery");
    harness
        .store
        .record_work_provider_receipt(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.receipt",
                "receipt-generation-2",
                0,
            ),
            "work-delivery:work-generation-2:1",
            NODE,
            "daemon-test",
            1,
            "claim-generation-2",
            "provider-receipt-generation-2",
            "t8.75",
        )
        .expect("record generation-two provider receipt");
    let mut current_progress = report("generation-2-progress", WorkReportKind::Progress, &worker);
    current_progress.work_id = "work-generation-2".into();
    current_progress.work_revision = 2;
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "generation-2-progress", 0),
            &team_id,
            current_progress,
        )
        .expect("new exact admission restores Work evidence authority");
}
