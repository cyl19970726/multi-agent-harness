use super::*;
use firm_core::agentfirm_api::{
    Confidence, PrimaryCauseStatus, RetrySafety, TrustErrorCode, WorkFindingKind,
};

fn start_context(member_run_id: &str, suffix: &str) -> firm_core::WorkCommandContext {
    firm_core::WorkCommandContext {
        event_id: format!("event-start-{suffix}"),
        performed_by_actor: firm_core::TeamActorRef {
            kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: format!("command-start-{suffix}"),
        created_at: format!("t-start-{suffix}"),
        duplicate_ok: false,
    }
}

#[test]
fn start_distinguishes_assigned_undispatched_foreign_and_dispatched_work() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    for agent_member_id in ["worker-start", "other-start"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "operator",
                    "identity.create",
                    &format!("identity-{agent_member_id}"),
                    0,
                ),
                identity(agent_member_id),
            )
            .unwrap();
    }
    let membership = join_runtime_membership(
        &store,
        "membership-worker-start",
        "team-admission",
        "worker-start",
        TeamMembershipRole::Member,
    );
    let other_membership = join_runtime_membership(
        &store,
        "membership-other-start",
        "team-admission",
        "other-start",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-start", "worker-start");
    store
        .create_agent_session(
            &service_context("session.create", &target.id, 0),
            target.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run("member-run-worker-start", "worker-start", "run-admission"),
    );

    let undispatched = assign_responsibility(&store, "work-undispatched", &membership.id);
    let operations_before_undispatched = store.canonical_operations().unwrap();
    let error = store
        .start_work(
            &undispatched.id,
            undispatched.version,
            "member-run-worker-start",
            start_context("member-run-worker-start", "undispatched"),
        )
        .expect_err("assigned Work without an Active binding is transiently undispatched");
    let trust = error.trust_error().expect("typed trust rejection");
    assert_eq!(trust.code, TrustErrorCode::DeliveryNotDispatched);
    assert!(trust.retryable);
    assert_eq!(trust.resource_kind, "work");
    assert_eq!(trust.resource_id, undispatched.id);
    assert_eq!(trust.current_version, Some(undispatched.version));
    assert_eq!(
        trust.message,
        "Work is assigned to you but not yet dispatched by the Supervisor; wait for the next pass and retry"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_undispatched
    );

    let foreign = assign_responsibility(&store, "work-foreign", &other_membership.id);
    let operations_before_foreign = store.canonical_operations().unwrap();
    let error = store
        .start_work(
            &foreign.id,
            foreign.version,
            "member-run-worker-start",
            start_context("member-run-worker-start", "foreign"),
        )
        .expect_err("Work assigned to another member remains a responsibility conflict");
    assert!(
        error.trust_error().is_none(),
        "foreign Work is not a delivery wait"
    );
    assert!(
        error
            .to_string()
            .contains("does not hold responsibility for open Work"),
        "{error}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_foreign
    );

    let dispatched = assign_responsibility(&store, "work-dispatched", &membership.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-worker-start",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-worker-start".into());
    runtime_binding.target_member_run_generation = Some(1);
    let binding = execution_binding(&dispatched, &membership, &target, "binding-worker-start");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", &binding.id, 0),
            &runtime_binding,
            binding.clone(),
        )
        .unwrap();
    store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-worker-start", 0),
            &binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-worker-start",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-worker-start",
        )
        .unwrap();
    store
        .record_work_provider_receipt(
            &service_context("work.receipt", "receipt-worker-start", 0),
            &binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-worker-start",
            "provider-receipt-worker-start",
            "t-receipt-worker-start",
        )
        .unwrap();
    let started = store
        .start_work(
            &dispatched.id,
            dispatched.version,
            "member-run-worker-start",
            start_context("member-run-worker-start", "dispatched"),
        )
        .expect("dispatched, claimed, provider-received Work starts");
    assert_eq!(started.phase, firm_core::WorkPhase::Active);
}

#[test]
fn start_requires_provider_received_delivery() {
    for claimed in [false, true] {
        let suffix = if claimed { "claimed" } else { "queued" };
        let (store, _root) = fabric_store();
        append_runtime_team(&store, "team-admission", "run-admission");
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.create", "identity-worker", 0),
                identity("worker-admission"),
            )
            .unwrap();
        let membership = join_runtime_membership(
            &store,
            "membership-worker-admission",
            "team-admission",
            "worker-admission",
            TeamMembershipRole::Member,
        );
        let target = session("session-worker-admission", "worker-admission");
        store
            .create_agent_session(
                &service_context("session.create", &target.id, 0),
                target.clone(),
            )
            .unwrap();
        admit_member_run(
            &store,
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        );
        let mut runtime_binding = runtime_command_fixture(
            "runtime-result-receipt-fence",
            RuntimeCommandKind::StartCycle,
            &target,
            "start_cycle",
        )
        .0
        .binding;
        runtime_binding.target_member_run_id = Some("member-run-admission".into());
        runtime_binding.target_member_run_generation = Some(1);
        let work = assign_responsibility(&store, &format!("work-result-{suffix}"), &membership.id);
        let binding = execution_binding(
            &work,
            &membership,
            &target,
            &format!("binding-result-{suffix}"),
        );
        store
            .bind_responsible_work_execution(
                &service_context("work.bind", &binding.id, 0),
                &runtime_binding,
                binding.clone(),
            )
            .unwrap();
        if claimed {
            store
                .claim_work_for_provider(
                    &service_context("work.claim", &format!("claim-result-{suffix}"), 0),
                    &binding.delivery_id,
                    &target.node_id,
                    &target.node_daemon_id,
                    target.node_daemon_generation,
                    &format!("claim-result-{suffix}"),
                    firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
                    "t-claim-result",
                )
                .unwrap();
        }
        let operations_before = store.canonical_operations().unwrap();
        let error = store
            .start_work(
                &work.id,
                work.version,
                "member-run-admission",
                firm_core::WorkCommandContext {
                    event_id: format!("event-start-result-{suffix}"),
                    performed_by_actor: firm_core::TeamActorRef {
                        kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                        id: "member-run-admission".into(),
                        display_name: None,
                        authn_source: Some("test".into()),
                    },
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("command-start-result-{suffix}"),
                    created_at: "t-start-result".into(),
                    duplicate_ok: false,
                },
            )
            .expect_err("Start requires exact ProviderReceived evidence");
        let trust = error.trust_error().expect("typed trust rejection");
        if claimed {
            // Claimed without a provider receipt is genuinely uncertain: the
            // provider may or may not have received the delivery.
            assert_eq!(trust.code, TrustErrorCode::DeliveryRecoveryUncertain);
            assert!(!trust.retryable);
        } else {
            // Queued and never claimed is certain and self-resolving.
            assert_eq!(trust.code, TrustErrorCode::DeliveryNotDispatched);
            assert!(trust.retryable);
            assert!(error.to_string().contains("DELIVERY_NOT_DISPATCHED"));
        }
        assert_eq!(store.canonical_operations().unwrap(), operations_before);
        assert_eq!(
            store
                .fabric_work_execution_bindings("space-test")
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.id == binding.id)
                .unwrap()
                .status,
            WorkExecutionBindingStatus::Active
        );
    }
}

#[test]
fn every_member_work_authoring_path_requires_provider_received_delivery() {
    for claimed in [false, true] {
        let suffix = if claimed { "claimed" } else { "queued" };
        let (store, _root) = fabric_store();
        append_runtime_team(&store, "team-admission", "run-admission");
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.create", "identity-worker", 0),
                identity("worker-admission"),
            )
            .unwrap();
        let membership = join_runtime_membership(
            &store,
            "membership-worker-admission",
            "team-admission",
            "worker-admission",
            TeamMembershipRole::Member,
        );
        let target = session("session-worker-admission", "worker-admission");
        store
            .create_agent_session(
                &service_context("session.create", &target.id, 0),
                target.clone(),
            )
            .unwrap();
        admit_member_run(
            &store,
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        );
        let mut runtime_binding = runtime_command_fixture(
            "runtime-failure-receipt-fence",
            RuntimeCommandKind::StartCycle,
            &target,
            "start_cycle",
        )
        .0
        .binding;
        runtime_binding.target_member_run_id = Some("member-run-admission".into());
        runtime_binding.target_member_run_generation = Some(1);
        let work = assign_responsibility(&store, &format!("work-failure-{suffix}"), &membership.id);
        let binding = execution_binding(
            &work,
            &membership,
            &target,
            &format!("binding-failure-{suffix}"),
        );
        store
            .bind_responsible_work_execution(
                &service_context("work.bind", &binding.id, 0),
                &runtime_binding,
                binding.clone(),
            )
            .unwrap();
        if claimed {
            store
                .claim_work_for_provider(
                    &service_context("work.claim", &format!("claim-failure-{suffix}"), 0),
                    &binding.delivery_id,
                    &target.node_id,
                    &target.node_daemon_id,
                    target.node_daemon_generation,
                    &format!("claim-failure-{suffix}"),
                    firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
                    "t-claim-failure",
                )
                .unwrap();
        }
        let expected_delivery_code = if claimed {
            TrustErrorCode::DeliveryRecoveryUncertain
        } else {
            TrustErrorCode::DeliveryNotDispatched
        };
        let active = work.clone();
        let operations_before = store.canonical_operations().unwrap();
        let progress = WorkReport {
            id: format!("report-progress-{suffix}"),
            work_id: active.id.clone(),
            work_revision: active.version,
            report_revision: 1,
            kind: WorkReportKind::Progress,
            authored_by: ActorRef {
                kind: ActorKind::AgentMember,
                id: "worker-admission".into(),
            },
            summary: "receipt is required".into(),
            base_revision: None,
            candidate: None,
            candidate_fingerprint: None,
            finding_refs: Vec::new(),
            failure_analysis_ref: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            evidence_refs: vec!["evidence://receipt-required".into()],
            known_risks: Vec::new(),
            confidence: Some(Confidence::High),
            recommended_next_action: Some("await provider receipt".into()),
            created_at: "t-progress".into(),
        };
        let progress_error = store
            .create_trust_work_report(
                &member_context("worker-admission", "report.create", &progress.id, 0),
                "team-admission",
                progress,
            )
            .expect_err("Progress requires exact ProviderReceived evidence");
        assert_eq!(
            progress_error
                .trust_error()
                .expect("typed trust rejection")
                .code,
            expected_delivery_code
        );

        let finding_id = format!("finding-{suffix}");
        let finding_error = store
            .create_trust_finding(
                &member_context("worker-admission", "finding.create", &finding_id, 0),
                "team-admission",
                WorkFinding {
                    id: finding_id,
                    work_id: active.id.clone(),
                    work_revision: active.version,
                    kind: WorkFindingKind::Discovery,
                    summary: "receipt is required".into(),
                    detail_markdown: "provider has not acknowledged this Work delivery".into(),
                    affected_work_refs: Vec::new(),
                    reusable_asset_refs: Vec::new(),
                    invalidated_assumptions: Vec::new(),
                    evidence_refs: Vec::new(),
                    confidence: Confidence::High,
                    reported_by: ActorRef {
                        kind: ActorKind::AgentMember,
                        id: "worker-admission".into(),
                    },
                    created_at: "t-finding".into(),
                },
            )
            .expect_err("Finding requires exact ProviderReceived evidence");
        assert_eq!(
            finding_error
                .trust_error()
                .expect("typed trust rejection")
                .code,
            expected_delivery_code
        );

        let analysis_id = format!("analysis-failure-{suffix}");
        let analysis_error = store
            .create_trust_failure_analysis(
                &member_context(
                    "worker-admission",
                    "failure_analysis.create",
                    &analysis_id,
                    0,
                ),
                "team-admission",
                FailureAnalysis {
                    id: analysis_id,
                    work_id: active.id.clone(),
                    work_revision: active.version,
                    member_run_id: Some("member-run-admission".into()),
                    candidate: None,
                    observed_failure: "provider execution failed".into(),
                    impact: "work incomplete".into(),
                    primary_cause_status: PrimaryCauseStatus::Confirmed,
                    primary_cause: Some("provider failure".into()),
                    contributing_causes: Vec::new(),
                    attempts_already_made: Vec::new(),
                    last_safe_checkpoint: None,
                    retry_safety: RetrySafety::Unknown,
                    side_effect_summary: Some("none".into()),
                    recovery_options: vec!["retry after Host review".into()],
                    recommended_host_decision: "review failure".into(),
                    evidence_refs: vec!["evidence://failure".into()],
                    confidence: Confidence::High,
                    reported_by: ActorRef {
                        kind: ActorKind::AgentMember,
                        id: "worker-admission".into(),
                    },
                    created_at: "t-failure-analysis".into(),
                },
            )
            .expect_err("FailureAnalysis requires exact ProviderReceived evidence");
        assert_eq!(
            analysis_error
                .trust_error()
                .expect("typed trust rejection")
                .code,
            expected_delivery_code
        );

        assert_eq!(store.canonical_operations().unwrap(), operations_before);
        let work_before_message = store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == active.id)
            .unwrap();
        let message_id = format!("message-{suffix}");
        store
            .author_message(
                &service_context("message.author", &message_id, 0),
                work_message(
                    &message_id,
                    &active,
                    "worker-admission",
                    &target.id,
                    "worker-admission",
                ),
            )
            .expect("Message authoring is independent of Work delivery receipt state");
        let work_after_message = store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == active.id)
            .unwrap();
        assert_eq!(work_after_message, work_before_message);

        assert_eq!(
            store
                .fabric_work_execution_bindings("space-test")
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.id == binding.id)
                .unwrap()
                .status,
            WorkExecutionBindingStatus::Active
        );
    }
}

#[test]
fn responsibility_aba_and_stale_session_generation_do_not_revive_old_binding() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    for member_id in ["worker-admission", "alternate-admission"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "operator",
                    "identity.create",
                    &format!("identity-{member_id}"),
                    0,
                ),
                identity(member_id),
            )
            .unwrap();
    }
    let worker = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let alternate = join_runtime_membership(
        &store,
        "membership-alternate-admission",
        "team-admission",
        "alternate-admission",
        TeamMembershipRole::Member,
    );
    let worker_session = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", &worker_session.id, 0),
            worker_session.clone(),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        )
        .unwrap();
    let assigned = assign_responsibility(&store, "work-aba", &worker.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-aba",
        RuntimeCommandKind::StartCycle,
        &worker_session,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let old_binding = execution_binding(&assigned, &worker, &worker_session, "binding-aba");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-aba", 0),
            &runtime_binding,
            old_binding,
        )
        .unwrap();
    let assigned_to_b = store
        .assign_work_to_membership(
            &assigned.id,
            assigned.version,
            &alternate.id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: "event-assign-b".into(),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-assign-b".into(),
                created_at: "t-b".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let assigned_back_to_a = store
        .assign_work_to_membership(
            &assigned.id,
            assigned_to_b.version,
            &worker.id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: "event-assign-a-again".into(),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-assign-a-again".into(),
                created_at: "t-a-again".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let projection_error = store
        .current_work_deliveries("space-test")
        .expect_err("A to B to A cannot revive the old delivery projection");
    assert!(projection_error
        .to_string()
        .contains("CURRENT_WORK_DELIVERY_CANONICAL_JOIN_CONFLICT"));
    let before_aba = store.canonical_operations().unwrap();
    let progress = WorkReport {
        id: "report-aba".into(),
        work_id: assigned.id.clone(),
        work_revision: assigned_back_to_a.version,
        report_revision: 1,
        kind: WorkReportKind::Progress,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        summary: "old binding must not revive".into(),
        base_revision: None,
        candidate: None,
        candidate_fingerprint: None,
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        evidence_refs: Vec::new(),
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: "t-aba-report".into(),
    };
    let error = store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &progress.id, 0),
            "team-admission",
            progress,
        )
        .expect_err("A to B to A responsibility must not revive the old binding");
    assert!(error.to_string().contains("UNAUTHORIZED_ACTOR"));
    assert_eq!(store.canonical_operations().unwrap(), before_aba);

    let stale_generation_work = assign_responsibility(&store, "work-stale-session", &worker.id);
    let stale_generation_binding = execution_binding(
        &stale_generation_work,
        &worker,
        &worker_session,
        "binding-stale-session",
    );
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-stale-session", 0),
            &runtime_binding,
            stale_generation_binding,
        )
        .unwrap();

    let mut advanced_session = worker_session.clone();
    advanced_session.runtime_generation = 2;
    advanced_session.version += 1;
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &service_context("session.advance", "session-worker-generation-2", 1),
                "agent_session",
                &advanced_session.id,
                "runtime_generation_advanced",
                serde_json::json!({"runtime_generation": 2}),
                &advanced_session,
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
    }
    let work_before_message = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == stale_generation_work.id)
        .unwrap();
    store
        .author_message(
            &service_context("message.author", "message-stale-session", 0),
            work_message(
                "message-stale-session",
                &stale_generation_work,
                "worker-admission",
                &advanced_session.id,
                "alternate-admission",
            ),
        )
        .expect("a current sender Session may link Work without inheriting its stale binding");
    let work_after_message = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == stale_generation_work.id)
        .unwrap();
    assert_eq!(work_after_message, work_before_message);
}

#[test]
fn missing_or_ambiguous_current_member_run_fails_before_delivery() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-admission",
                0,
            ),
            identity("worker-admission"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", "session-worker-admission", 0),
            target.clone(),
        )
        .unwrap();
    let missing_work = assign_responsibility(&store, "work-missing-run", &membership.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-missing",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let before_missing = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-missing", 0),
            &runtime_binding,
            execution_binding(&missing_work, &membership, &target, "binding-missing"),
        )
        .expect_err("missing current MemberRun must fail closed");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_missing);

    for id in ["member-run-admission", "member-run-duplicate"] {
        store
            .legacy_import_create_trust_member_run_projection(
                &context("host", "member_run.create", id, 0),
                canonical_member_run(id, "worker-admission", "run-admission"),
            )
            .unwrap();
    }
    let ambiguous_work = assign_responsibility(&store, "work-ambiguous-run", &membership.id);
    let before_ambiguous = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-ambiguous", 0),
            &runtime_binding,
            execution_binding(&ambiguous_work, &membership, &target, "binding-ambiguous"),
        )
        .expect_err("ambiguous current MemberRun must fail closed");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_ambiguous);
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .is_empty());
    assert!(store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .is_empty());
}

#[test]
fn member_run_cutover_linearizes_before_stale_execution_admission() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-admission",
                0,
            ),
            identity("worker-admission"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", "session-worker-admission", 0),
            target.clone(),
        )
        .unwrap();
    let generation_one =
        canonical_member_run("member-run-admission", "worker-admission", "run-admission");
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            generation_one.clone(),
        )
        .unwrap();
    let work = assign_responsibility(&store, "work-cutover", &membership.id);
    let mut stale_runtime_binding = runtime_command_fixture(
        "runtime-cutover",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    stale_runtime_binding.target_member_run_id = Some(generation_one.id.clone());
    stale_runtime_binding.target_member_run_generation = Some(1);
    let stale_binding = execution_binding(&work, &membership, &target, "binding-cutover");

    let store = std::sync::Arc::new(store);
    let first = store.acquire_write_lock().expect("hold Store writer");
    let cutover_store = std::sync::Arc::clone(&store);
    let mut generation_two = generation_one;
    generation_two.runtime_generation = 2;
    generation_two.version = 2;
    let cutover = std::thread::spawn(move || {
        let _lock = cutover_store.acquire_write_lock()?;
        cutover_store.commit_trust_projection_unlocked(
            &context("host", "member_run.cutover", "member-run-admission", 1),
            "member_run",
            "member-run-admission",
            "runtime_generation_advanced",
            serde_json::to_value(&generation_two)?,
            &generation_two,
            Vec::new(),
            Vec::new(),
        )
    });
    wait_for_write_ticket(&store, 2);

    let admission_store = std::sync::Arc::clone(&store);
    let admission = std::thread::spawn(move || {
        admission_store.bind_responsible_work_execution(
            &service_context("work.bind", "binding-cutover", 0),
            &stale_runtime_binding,
            stale_binding,
        )
    });
    wait_for_write_ticket(&store, 3);
    drop(first);

    cutover
        .join()
        .expect("cutover writer joins")
        .expect("generation cutover commits first");
    let error = admission
        .join()
        .expect("admission writer joins")
        .expect_err("old generation cannot bind after cutover");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .is_empty());
    assert!(store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .is_empty());
}

/// A member that already holds one active Work is busy on the member plane.
/// That answer must not be replaced by a WorkDelivery-plane rejection just
/// because the second Work's delivery has not been dispatched yet.
#[test]
fn start_reports_member_busy_before_a_queued_second_delivery() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.create", "identity-busy-worker", 0),
            identity("busy-worker"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-busy-worker",
        "team-admission",
        "busy-worker",
        TeamMembershipRole::Member,
    );
    let target = session("session-busy-worker", "busy-worker");
    store
        .create_agent_session(
            &service_context("session.create", &target.id, 0),
            target.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run("member-run-busy-worker", "busy-worker", "run-admission"),
    );
    let mut runtime_binding = runtime_command_fixture(
        "runtime-busy-worker",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-busy-worker".into());
    runtime_binding.target_member_run_generation = Some(1);

    let active_work = assign_responsibility(&store, "work-busy-active", &membership.id);
    let active_binding =
        execution_binding(&active_work, &membership, &target, "binding-busy-active");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", &active_binding.id, 0),
            &runtime_binding,
            active_binding.clone(),
        )
        .unwrap();
    let queued_work = assign_responsibility(&store, "work-busy-queued", &membership.id);
    let queued_binding =
        execution_binding(&queued_work, &membership, &target, "binding-busy-queued");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", &queued_binding.id, 0),
            &runtime_binding,
            queued_binding.clone(),
        )
        .unwrap();
    store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-busy-active", 0),
            &active_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-busy-active",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-busy-active",
        )
        .unwrap();
    store
        .record_work_provider_receipt(
            &service_context("work.receipt", "receipt-busy-active", 0),
            &active_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-busy-active",
            "provider-receipt-busy-active",
            "t-receipt-busy-active",
        )
        .unwrap();
    store
        .start_work(
            &active_work.id,
            active_work.version,
            "member-run-busy-worker",
            firm_core::WorkCommandContext {
                event_id: "event-start-busy-active".into(),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                    id: "member-run-busy-worker".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-start-busy-active".into(),
                created_at: "t-start-busy-active".into(),
                duplicate_ok: false,
            },
        )
        .expect("provider-received delivery admits the first Start");

    assert_eq!(
        store
            .fabric_work_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.id == queued_binding.delivery_id)
            .unwrap()
            .status,
        WorkDeliveryStatus::Queued
    );
    let operations_before = store.canonical_operations().unwrap();
    let error = store
        .start_work(
            &queued_work.id,
            queued_work.version,
            "member-run-busy-worker",
            firm_core::WorkCommandContext {
                event_id: "event-start-busy-queued".into(),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                    id: "member-run-busy-worker".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-start-busy-queued".into(),
                created_at: "t-start-busy-queued".into(),
                duplicate_ok: false,
            },
        )
        .expect_err("a member with active Work cannot start a second Work");
    assert!(
        error.to_string().contains("MEMBER_BUSY")
            && error.to_string().contains("already has active Work"),
        "{error}"
    );
    assert!(
        !error.to_string().contains("DELIVERY_"),
        "member-plane busyness must not be reported as a delivery-plane fault: {error}"
    );
    assert_eq!(store.canonical_operations().unwrap(), operations_before);
}
