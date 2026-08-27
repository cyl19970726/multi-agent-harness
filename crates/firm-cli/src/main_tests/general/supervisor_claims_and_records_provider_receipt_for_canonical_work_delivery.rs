use super::*;
use harness_core::CurrentWorkDraft;

#[test]
fn supervisor_claims_and_records_provider_receipt_for_canonical_work_delivery() {
    let (store, root) = temp_store("canonical-supervisor-work-delivery");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let work = store
        .insert_work(
            {
                let mut draft = CurrentWorkDraft::new(
                    "canonical-supervisor-work".into(),
                    created.team_run.id.clone(),
                    created.team_run.agent_team_id.clone(),
                    "Deliver canonical Work".into(),
                    "Exercise NodeDaemon canonical delivery wiring".into(),
                    "Provider receipt is canonical".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    compatibility_team_actor("host", "test"),
                    "unix-ms:3".into(),
                );
                draft.eligible_member_ids = vec![member.agent_member_id.clone()];
                draft.into_work()
            },
            WorkCommandContext {
                event_id: "canonical-supervisor-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-create".into(),
                created_at: "unix-ms:3".into(),
                duplicate_ok: false,
            },
        )
        .expect("create unassigned Work");
    let membership = store
        .fabric_team_memberships("unit-test-space")
        .expect("Team memberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == created.team_run.agent_team_id
                && membership.agent_member_id == member.agent_member_id
        })
        .expect("exact member TeamMembership");
    let work = store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &membership.id,
            "unit-test-space",
            WorkCommandContext {
                event_id: "canonical-supervisor-work-assigned".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-assign".into(),
                created_at: "unix-ms:4".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign stable TeamMembership responsibility");
    assert_eq!(work.active_member_run_id, None);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-work-supervisor",
            std::process::id(),
            "test://canonical-work-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("claim canonical Work")
        .expect("one canonical Work claim");
    let bindings = store
        .fabric_work_execution_bindings("unit-test-space")
        .expect("canonical WorkExecutionBinding");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].team_membership_id, membership.id);
    assert_eq!(bindings[0].agent_member_id, member.agent_member_id);
    ledger
        .complete_work_delivery(&claimed, "provider-work-receipt")
        .expect("record canonical provider receipt");
    assert!(
        claim_canonical_work_for_member(&ledger, &member)
            .expect("repeat scheduler scan is safe")
            .is_none(),
        "provider-received Work must not create or claim a second delivery"
    );
    let delivery = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical WorkDelivery fabric")
        .into_iter()
        .find(|delivery| delivery.work_id == work.id)
        .expect("canonical delivery");
    assert_eq!(
        delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        delivery.provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    let current = store
        .current_work_deliveries_for_team_run(&created.team_run.id)
        .expect("current canonical WorkDelivery view");
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].delivery_id, delivery.id);
    assert_eq!(
        current[0].status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        current[0].provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    assert_eq!(current[0].attempt, 1);
    assert_eq!(
        store
            .fabric_work_execution_bindings("unit-test-space")
            .expect("bindings after repeat scan")
            .len(),
        1,
        "repeat scheduling is idempotent"
    );
    assert_eq!(
        current[0].authority,
        harness_application::CurrentWorkDeliveryAuthority::CanonicalTrust
    );
    let started = store
        .start_work(
            &work.id,
            work.version,
            &member.id,
            WorkCommandContext {
                event_id: "canonical-supervisor-work-started".into(),
                performed_by_actor: compatibility_team_actor(&member.id, "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-work-start".into(),
                created_at: "unix-ms:5".into(),
                duplicate_ok: false,
            },
        )
        .expect("current MemberRun starts stable responsibility Work");
    assert_eq!(started.active_member_run_id, None);
    let candidate = harness_core::agentfirm_api::CandidateRef {
        kind: harness_core::agentfirm_api::CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let candidate_fingerprint = harness_store::canonical_json_fingerprint(
        &serde_json::to_value(&candidate).expect("serialize exact candidate"),
    );
    let result_report = harness_core::agentfirm_api::WorkReport {
        id: "canonical-supervisor-work-result".into(),
        work_id: started.id.clone(),
        work_revision: started.version + 1,
        report_revision: 1,
        kind: harness_core::agentfirm_api::WorkReportKind::Result,
        authored_by: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: member.agent_member_id.clone(),
        },
        summary: "canonical stable-responsibility result".into(),
        base_revision: None,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint.clone()),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: vec!["artifact:canonical-work".into()],
        check_refs: vec!["check:canonical-work".into()],
        github_links: Vec::new(),
        evidence_refs: vec!["evidence:canonical-work".into()],
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: "unix-ms:6".into(),
    };
    store
        .create_trust_work_report(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: result_report.authored_by.clone(),
                authority_actor: None,
                command_name: "work_report.create".into(),
                idempotency_key: "canonical-supervisor-work-result".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &created.team_run.agent_team_id,
            result_report,
        )
        .expect("ProviderReceived execution submits exact semantic Result");
    let submitted = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == started.id)
        .unwrap();
    assert_eq!(submitted.phase, WorkPhase::Review);
    assert_eq!(submitted.active_member_run_id, None);
    let released = store
        .fabric_work_execution_bindings("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|binding| binding.work_id == submitted.id)
        .unwrap();
    assert_eq!(
        released.status,
        harness_core::agentfirm_api::WorkExecutionBindingStatus::Released
    );
    let preserved_delivery = store
        .fabric_work_deliveries("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == delivery.id)
        .unwrap();
    assert_eq!(
        preserved_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );
    assert_eq!(
        preserved_delivery.provider_receipt_id.as_deref(),
        Some("provider-work-receipt")
    );
    let accepted = store
        .accept_trust_work(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: "unit-test-space".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: "host".into(),
                },
                authority_actor: None,
                command_name: "work.accept".into(),
                idempotency_key: "canonical-supervisor-work-accept".into(),
                expected_version: submitted.version,
                request_fingerprint: None,
            },
            &created.team_run.agent_team_id,
            &submitted.id,
            "canonical-supervisor-work-result",
            &candidate_fingerprint,
            "unix-ms:7",
        )
        .expect("Host acceptance remains independent from provider receipt and Result");
    assert_eq!(accepted.projection.phase, WorkPhase::Closed);

    let next_work = store
        .insert_work(
            CurrentWorkDraft::new(
                "canonical-supervisor-next-work".into(),
                created.team_run.id.clone(),
                created.team_run.agent_team_id.clone(),
                "Next canonical Work".into(),
                "Prove released execution does not wedge scheduling".into(),
                "The same Member receives one new exact admission".into(),
                WorkClaimMode::HostAssign,
                WorkPriority::Normal,
                compatibility_team_actor("host", "test"),
                "unix-ms:8".into(),
            )
            .into_work(),
            WorkCommandContext {
                event_id: "canonical-supervisor-next-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-next-work-create".into(),
                created_at: "unix-ms:8".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let next_work = store
        .assign_work_to_membership(
            &next_work.id,
            next_work.version,
            &membership.id,
            "unit-test-space",
            WorkCommandContext {
                event_id: "canonical-supervisor-next-work-assigned".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "canonical-supervisor-next-work-assign".into(),
                created_at: "unix-ms:9".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let next_claim = claim_canonical_work_for_member(&ledger, &member)
        .expect("scheduler remains live after semantic Result")
        .expect("same Member receives next canonical Work");
    assert_eq!(next_claim.work.id, next_work.id);
    assert_eq!(
        store
            .fabric_work_deliveries("unit-test-space")
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.work_id == submitted.id)
            .count(),
        1,
        "completed Work is never replayed as a new delivery"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn responsibility_cutover_releases_only_stale_binding_and_rebinds_monotonically() {
    let (store, root) = temp_store("canonical-responsibility-cutover");
    let created = create_two_member_team_run(&store);
    let member_a = created.member_runs[0].clone();
    let member_b = created.member_runs[1].clone();
    let memberships = store
        .fabric_team_memberships("unit-test-space")
        .expect("Team memberships");
    let membership_a = memberships
        .iter()
        .find(|membership| membership.agent_member_id == member_a.agent_member_id)
        .unwrap()
        .clone();
    let membership_b = memberships
        .iter()
        .find(|membership| membership.agent_member_id == member_b.agent_member_id)
        .unwrap()
        .clone();
    let create_assigned = |id: &str, membership_id: &str| {
        let work = store
            .insert_work(
                CurrentWorkDraft::new(
                    id.into(),
                    created.team_run.id.clone(),
                    created.team_run.agent_team_id.clone(),
                    id.into(),
                    "Exercise stale-binding reconciliation".into(),
                    "Only the exact current responsibility may execute".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    compatibility_team_actor("host", "test"),
                    "unix-ms:3".into(),
                )
                .into_work(),
                WorkCommandContext {
                    event_id: format!("{id}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: "unix-ms:3".into(),
                    duplicate_ok: false,
                },
            )
            .unwrap();
        store
            .assign_work_to_membership(
                &work.id,
                work.version,
                membership_id,
                "unit-test-space",
                WorkCommandContext {
                    event_id: format!("{id}-assigned"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-assign"),
                    created_at: "unix-ms:4".into(),
                    duplicate_ok: false,
                },
            )
            .unwrap()
    };
    let target = create_assigned("work-cutover-target", &membership_a.id);
    let unrelated = create_assigned("work-unrelated-a", &membership_a.id);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "cutover-supervisor",
            std::process::id(),
            "test://cutover-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let sessions = store.fabric_agent_sessions("unit-test-space").unwrap();
    let session_a = sessions
        .iter()
        .find(|session| session.agent_member_id == member_a.agent_member_id)
        .unwrap();
    let current_runs = store.trust_member_runs("unit-test-space").unwrap();
    let run_a = current_runs
        .iter()
        .find(|run| run.id == member_a.id)
        .unwrap();
    for work in [&target, &unrelated] {
        let binding_id = format!("binding:{}:1", work.id);
        store
            .bind_responsible_work_execution(
                &canonical_delivery_context(
                    "unit-test-space",
                    &session_a.node_daemon_id,
                    "node_daemon.work_execution_binding.bind",
                    binding_id.clone(),
                    0,
                ),
                &runtime_command_binding_for_member_session(run_a, session_a),
                harness_core::agentfirm_api::WorkExecutionBinding {
                    id: binding_id,
                    work_id: work.id.clone(),
                    work_revision: work.version,
                    team_id: created.team_run.agent_team_id.clone(),
                    team_membership_id: membership_a.id.clone(),
                    agent_member_id: member_a.agent_member_id.clone(),
                    agent_session_id: session_a.id.clone(),
                    agent_session_generation: session_a.runtime_generation,
                    delivery_id: format!("work-delivery:{}:1", work.id),
                    binding_generation: 1,
                    status: harness_core::agentfirm_api::WorkExecutionBindingStatus::Active,
                    version: 1,
                    created_by: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: session_a.node_daemon_id.clone(),
                    },
                    bound_at: "unix-ms:5".into(),
                    ended_at: None,
                },
            )
            .unwrap();
    }
    let unrelated_delivery = store
        .fabric_work_deliveries("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.work_id == unrelated.id)
        .unwrap();
    store
        .claim_work_for_provider(
            &canonical_delivery_context(
                "unit-test-space",
                &session_a.node_daemon_id,
                "node_daemon.work_delivery.claim",
                "unrelated-a-claim".into(),
                0,
            ),
            &unrelated_delivery.id,
            &session_a.node_id,
            &session_a.node_daemon_id,
            session_a.node_daemon_generation,
            "unrelated-a-claim",
            harness_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "unix-ms:5.25",
        )
        .expect("freeze unrelated A provider effect before lifecycle revision");
    let unrelated_started = store
        .start_work(
            &unrelated.id,
            unrelated.version,
            &member_a.id,
            WorkCommandContext {
                event_id: "work-unrelated-a-started".into(),
                performed_by_actor: compatibility_team_actor(&member_a.id, "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "work-unrelated-a-start".into(),
                created_at: "unix-ms:5.5".into(),
                duplicate_ok: false,
            },
        )
        .expect("ordinary lifecycle revision keeps the exact binding current");
    assert!(unrelated_started.version > unrelated.version);
    let reassigned = store
        .assign_work_to_membership(
            &target.id,
            target.version,
            &membership_b.id,
            "unit-test-space",
            WorkCommandContext {
                event_id: "work-cutover-target-reassigned".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "work-cutover-target-reassign".into(),
                created_at: "unix-ms:6".into(),
                duplicate_ok: false,
            },
        )
        .expect("responsibility changes before execution-plane reconciliation");

    let before_stale_claim = store.canonical_operations().unwrap();
    let old_delivery = store
        .fabric_work_deliveries("unit-test-space")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.work_id == target.id)
        .unwrap();
    let error = store
        .claim_work_for_provider(
            &canonical_delivery_context(
                "unit-test-space",
                &session_a.node_daemon_id,
                "node_daemon.work_delivery.claim",
                "stale-g1-claim".into(),
                0,
            ),
            &old_delivery.id,
            &session_a.node_id,
            &session_a.node_daemon_id,
            session_a.node_daemon_generation,
            "stale-g1-claim",
            harness_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "unix-ms:6.5",
        )
        .expect_err("stale queued binding is inert before daemon reconciliation");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_stale_claim);
    assert_eq!(
        store
            .fabric_work_deliveries("unit-test-space")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.id == old_delivery.id)
            .unwrap(),
        old_delivery
    );

    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let claimed = claim_canonical_work_for_member(&ledger, &member_b)
        .expect("B scheduler reconciles stale A binding")
        .expect("B receives the reassigned Work");
    assert_eq!(claimed.work.id, reassigned.id);
    let bindings = store
        .fabric_work_execution_bindings("unit-test-space")
        .unwrap();
    let old_target = bindings
        .iter()
        .find(|binding| binding.work_id == target.id && binding.binding_generation == 1)
        .unwrap();
    let new_target = bindings
        .iter()
        .find(|binding| binding.work_id == target.id && binding.binding_generation == 2)
        .unwrap();
    assert_eq!(
        old_target.status,
        harness_core::agentfirm_api::WorkExecutionBindingStatus::Released
    );
    assert_eq!(
        new_target.status,
        harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
    );
    assert_eq!(new_target.agent_member_id, member_b.agent_member_id);
    assert!(bindings.iter().any(|binding| {
        binding.work_id == unrelated.id
            && binding.binding_generation == 1
            && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
    }));
    let deliveries = store.fabric_work_deliveries("unit-test-space").unwrap();
    let delivery_ids = deliveries
        .iter()
        .filter(|delivery| delivery.work_id == target.id)
        .map(|delivery| delivery.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(delivery_ids.len(), 2);
    assert!(delivery_ids.contains("work-delivery:work-cutover-target:1"));
    assert!(delivery_ids.contains("work-delivery:work-cutover-target:2"));
    let old_delivery = deliveries
        .iter()
        .find(|delivery| delivery.id == "work-delivery:work-cutover-target:1")
        .unwrap();
    assert_eq!(
        old_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Failed
    );
    assert_eq!(
        old_delivery.failure_code.as_deref(),
        Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM")
    );
    let current_views = store
        .current_work_deliveries("unit-test-space")
        .expect("released g1 and active g2 remain readable canonical evidence");
    let historical_g1 = current_views
        .iter()
        .find(|delivery| delivery.delivery_id == old_delivery.id)
        .unwrap();
    assert_eq!(historical_g1.recipient_member_run_id, None);
    let current_g2 = current_views
        .iter()
        .find(|delivery| delivery.delivery_id == claimed.delivery.id)
        .unwrap();
    assert_eq!(
        current_g2.recipient_member_run_id.as_deref(),
        Some(member_b.id.as_str())
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn driver_generation_drift_releases_queued_binding_before_monotonic_rebind() {
    let (store, root) = temp_store("driver-generation-stale-work-binding");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "driver-drift-supervisor",
            std::process::id(),
            "test://driver-drift",
            current_unix_ms_u64(),
            60_000,
        )
        .unwrap();
    ensure_test_runtime_fabric(&store, &created, &lease);
    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: "work-driver-drift".into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: "Driver drift".into(),
            context_markdown: "Reconcile exact control authority".into(),
            completion_criteria_markdown: "Only g2 reaches provider claim".into(),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: WorkCommandContext {
                event_id: "work-driver-drift-create".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "work-driver-drift-create".into(),
                created_at: "unix-ms:1".into(),
                duplicate_ok: false,
            },
        })
        .unwrap();
    let work =
        assign_test_work_to_member(&store, &lease.execution_space_id, &created, &member, &work);
    bind_test_responsible_work_execution(&store, &lease, &member, &work);
    let session = store
        .fabric_agent_sessions(&lease.execution_space_id)
        .unwrap()
        .into_iter()
        .find(|session| session.agent_member_id == member.agent_member_id)
        .unwrap();
    let mut next_control = session.control_state.clone();
    next_control.driver_generation += 1;
    store
        .bind_agent_session_control_state(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "node_daemon.agent_session.control.bind",
                "driver-drift-control".into(),
                session.version,
            ),
            &session.id,
            session.runtime_generation,
            next_control,
            "unix-ms:2",
        )
        .unwrap();
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("exact daemon reconciles control drift")
        .expect("g2 delivery is claimable");
    assert_eq!(claimed.work.id, work.id);
    let bindings = store
        .fabric_work_execution_bindings(&lease.execution_space_id)
        .unwrap();
    assert!(bindings.iter().any(|binding| {
        binding.work_id == work.id
            && binding.binding_generation == 1
            && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Released
    }));
    assert!(bindings.iter().any(|binding| {
        binding.work_id == work.id
            && binding.binding_generation == 2
            && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
    }));
    let deliveries = store
        .fabric_work_deliveries(&lease.execution_space_id)
        .unwrap();
    assert_eq!(
        deliveries
            .iter()
            .find(|delivery| delivery.id == "work-delivery:work-driver-drift:1")
            .unwrap()
            .status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Failed
    );
    assert_eq!(claimed.delivery.id, "work-delivery:work-driver-drift:2");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn supervisor_skips_not_ready_delivery_and_claims_ready_predecessor() {
    let (store, root) = temp_store("canonical-supervisor-ready-work-delivery");
    let created = create_two_member_team_run(&store);
    let member = created.member_runs[0].clone();
    let make_work = |id: &str, created_at: &str, prerequisites: Vec<String>| {
        let mut draft = CurrentWorkDraft::new(
            id.into(),
            created.team_run.id.clone(),
            created.team_run.agent_team_id.clone(),
            id.into(),
            "Exercise readiness-aware delivery selection".into(),
            "Only authoritative-ready Work reaches provider claim".into(),
            WorkClaimMode::HostAssign,
            WorkPriority::Normal,
            compatibility_team_actor("host", "test"),
            created_at.into(),
        );
        draft.eligible_member_ids = vec![member.agent_member_id.clone()];
        draft.prerequisite_work_ids = prerequisites;
        let created_work = store
            .insert_work(
                draft.into_work(),
                WorkCommandContext {
                    event_id: format!("{id}-created"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: created_at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("create unassigned Work");
        let membership = store
            .fabric_team_memberships("unit-test-space")
            .expect("read TeamMemberships")
            .into_iter()
            .find(|membership| {
                membership.team_id == created.team_run.agent_team_id
                    && membership.agent_member_id == member.agent_member_id
            })
            .expect("exact responsible TeamMembership");
        store
            .assign_work_to_membership(
                &created_work.id,
                created_work.version,
                &membership.id,
                "unit-test-space",
                WorkCommandContext {
                    event_id: format!("{id}-assigned"),
                    performed_by_actor: compatibility_team_actor("host", "test"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-assign"),
                    created_at: created_at.into(),
                    duplicate_ok: false,
                },
            )
            .expect("assign stable TeamMembership responsibility")
    };
    let predecessor = make_work("z-ready-predecessor", "unix-ms:3", Vec::new());
    let dependent = make_work(
        "a-not-ready-dependent",
        "unix-ms:4",
        vec![predecessor.id.clone()],
    );
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "ready-selection-supervisor",
            std::process::id(),
            "test://ready-selection-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );

    let ready_current = ledger
        .queued_works_for(&member.id)
        .expect("readiness-filtered current queue");
    assert!(
        ready_current.is_empty(),
        "no current delivery exists before a canonical binding is created"
    );

    let claimed = claim_canonical_work_for_member(&ledger, &member)
        .expect("select one ready Work")
        .expect("ready predecessor is claimable");
    assert_eq!(claimed.work.id, predecessor.id);
    ledger
        .fail_unreceived_work_claims_for(&member.id, "focused-negative-ack")
        .expect("settle exact canonical claim as failed");
    let deliveries = store
        .fabric_work_deliveries("unit-test-space")
        .expect("canonical deliveries");
    let predecessor_delivery = deliveries
        .iter()
        .find(|delivery| delivery.work_id == predecessor.id)
        .expect("predecessor canonical delivery");
    assert_eq!(
        predecessor_delivery.status,
        harness_core::agentfirm_api::WorkDeliveryStatus::Failed
    );
    assert_eq!(
        predecessor_delivery.failure_code.as_deref(),
        Some("provider-negative-ack:focused-negative-ack")
    );
    assert!(
        deliveries
            .iter()
            .all(|delivery| delivery.work_id != dependent.id),
        "not-ready Work must not receive an execution binding or fabric delivery"
    );
    let current_views = store
        .current_work_deliveries_for_team_run(&created.team_run.id)
        .expect("current canonical delivery views");
    assert!(
        current_views
            .iter()
            .all(|delivery| delivery.work_id != dependent.id),
        "an unready Work must not appear in the canonical delivery projection"
    );
    let ready_after_claim = ledger
        .queued_works_for(&member.id)
        .expect("readiness-filtered current queue");
    assert!(ready_after_claim.is_empty());
    std::fs::remove_dir_all(root).expect("cleanup");
}
