use super::*;

fn wait_for_write_ticket(store: &HarnessStore, expected_next_ticket: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let next_ticket = store
            .process_write_lock
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .next_ticket;
        if next_ticket >= expected_next_ticket {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "writer did not enter the Store FIFO queue"
        );
        std::thread::yield_now();
    }
}

fn canonical_member_run(id: &str, agent_member_id: &str, team_run_id: &str) -> MemberRun {
    MemberRun {
        id: id.into(),
        agent_member_id: agent_member_id.into(),
        team_run_id: team_run_id.into(),
        role_snapshot: "member".into(),
        provider_profile_snapshot: None,
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: MemberCoordinationStatus::Active,
        runtime_status: MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: None,
        native_session: None,
        version: 1,
        started_at: "t-member".into(),
        last_event_at: None,
        finished_at: None,
    }
}

fn legacy_member_run(
    id: &str,
    agent_member_id: &str,
    team_run_id: &str,
) -> ProviderRuntimeProjection {
    ProviderRuntimeProjection {
        id: id.into(),
        team_run_id: team_run_id.into(),
        slot_id: None,
        agent_member_id: agent_member_id.into(),
        name: agent_member_id.into(),
        role: "member".into(),
        provider: "codex".into(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: LegacyMemberCoordinationStatus::Active,
        runtime_generation: 1,
        status: MemberRunStatus::Idle,
        native_session: None,
        provider_cwd_hint: None,
        provider_environment_observation: None,
        owned_paths: Vec::new(),
        zero_output_streak: 0,
        last_consumed_work_version: None,
        started_at: "t-member".into(),
        last_event_at: None,
        finished_at: None,
    }
}

fn admit_member_run(store: &HarnessStore, run: MemberRun) {
    let current_team_run = store
        .team_runs()
        .unwrap()
        .into_iter()
        .rev()
        .find(|candidate| candidate.id == run.team_run_id)
        .unwrap();
    let mut next_team_run = current_team_run.clone();
    next_team_run.member_run_ids.push(run.id.clone());
    next_team_run.updated_at = format!("t-admit-{}", next_team_run.member_run_ids.len());
    store
        .admit_member_run_with_canonical(
            &current_team_run,
            &next_team_run,
            &legacy_member_run(&run.id, &run.agent_member_id, &run.team_run_id),
            "space-test",
            &CanonicalMemberRunAdmission {
                context: context("host", "member_run.create", &run.id, 0),
                run,
            },
        )
        .unwrap();
}

fn assign_responsibility(
    store: &HarnessStore,
    work_id: &str,
    membership_id: &str,
) -> firm_core::Work {
    let work = insert_runtime_work(store, work_id, "team-admission", "run-admission");
    store
        .assign_work_to_membership(
            &work.id,
            work.version,
            membership_id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: format!("event-assign-{work_id}"),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("command-assign-{work_id}"),
                created_at: "t-assign".into(),
                duplicate_ok: false,
            },
        )
        .unwrap()
}

fn execution_binding(
    work: &firm_core::Work,
    membership: &TeamMembership,
    session: &AgentSession,
    id: &str,
) -> WorkExecutionBinding {
    WorkExecutionBinding {
        id: id.into(),
        work_id: work.id.clone(),
        work_revision: work.version,
        team_id: membership.team_id.clone(),
        team_membership_id: membership.id.clone(),
        agent_member_id: membership.agent_member_id.clone(),
        agent_session_id: session.id.clone(),
        agent_session_generation: session.runtime_generation,
        delivery_id: format!("work-delivery:{}:1", work.id),
        binding_generation: 1,
        status: WorkExecutionBindingStatus::Active,
        version: 1,
        created_by: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        bound_at: "t-bind".into(),
        ended_at: None,
    }
}

fn member_context(member_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::AgentMember,
            id: member_id.into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version: expected,
        request_fingerprint: None,
    }
}

fn work_message(
    id: &str,
    work: &firm_core::Work,
    sender_id: &str,
    sender_session_id: &str,
    recipient_id: &str,
) -> Message {
    let recipient = firm_core::agentfirm_api::MessageRecipientRef {
        kind: MessageRecipientKind::AgentMember,
        id: recipient_id.into(),
    };
    let body = format!("Work-linked coordination for {}", work.id);
    let mut message = Message {
        id: id.into(),
        source_execution_space_id: "space-test".into(),
        source_node_id: "11111111-1111-4111-8111-111111111111".into(),
        source_node_daemon_id: "daemon-1".into(),
        source_authority_generation: 1,
        sender_actor_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: sender_id.into(),
        },
        sender_agent_member_id: Some(sender_id.into()),
        sender_session_id: Some(sender_session_id.into()),
        address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
        target_ref: recipient.clone(),
        recipients: vec![recipient],
        team_id: work.accountable_team_id.clone(),
        team_run_id: Some(work.team_run_id.clone()),
        work_id: Some(work.id.clone()),
        collaboration_scope: None,
        kind: firm_core::agentfirm_api::MessageKind::Message,
        body_digest: format!("sha256:{:x}", Sha256::digest(body.as_bytes())),
        body,
        correlation_id: format!("correlation-{id}"),
        causation_id: None,
        response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: id.into(),
        created_at: "t-message".into(),
    };
    message.content_fingerprint = message_content_fingerprint(&message);
    message
}

fn create_direct_subscription(store: &HarnessStore, sender_id: &str, recipient: &TeamMembership) {
    let subscription = MessageSubscription {
        id: format!("subscription-{}", recipient.id),
        subscriber_kind: MessageSubjectKind::AgentMember,
        subscriber_ref: recipient.agent_member_id.clone(),
        execution_space_id: "space-test".into(),
        target_team_id: Some(recipient.team_id.clone()),
        target_node_id: recipient.node_id.clone(),
        source_kind: MessageSubscriptionKind::Agent,
        source_ref: sender_id.into(),
        delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: Some(recipient.id.clone()),
        authorization_policy_ref: "direct.test".into(),
        policy_revision: 1,
        policy_digest: canonical_json_fingerprint(&serde_json::json!({"direct": true})),
        status: MessageSubscriptionStatus::Active,
        revision: 1,
        created_by: actor("host"),
        created_at: "t-subscription".into(),
        revoked_at: None,
    };
    store
        .create_message_subscription(
            &context("host", "message_subscription.create", &subscription.id, 0),
            subscription,
        )
        .unwrap();
}

#[test]
fn responsibility_resolves_one_current_member_run_and_repeated_admission_replays() {
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
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        )
        .unwrap();
    let work = assign_responsibility(&store, "work-admission", &membership.id);
    assert_eq!(work.active_member_run_id, None);

    let mut runtime_binding = runtime_command_fixture(
        "runtime-admission",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let binding = execution_binding(&work, &membership, &target, "binding-admission");
    let admission = service_context("work.bind", "binding-admission", 0);

    let before_legacy_writer = store.canonical_operations().unwrap();
    let error = store
        .bind_work_execution(&admission, binding.clone())
        .expect_err("unfenced public binding writer must be retired");
    assert!(error
        .to_string()
        .contains("WORK_EXECUTION_ADMISSION_REQUIRED"));
    assert_eq!(store.canonical_operations().unwrap(), before_legacy_writer);

    let mut stale = runtime_binding.clone();
    stale.target_member_run_generation = Some(2);
    let before_stale = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(&admission, &stale, binding.clone())
        .expect_err("stale MemberRun generation must not bind Work");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_stale);

    let accepted = store
        .bind_responsible_work_execution(&admission, &runtime_binding, binding.clone())
        .expect("exact current runtime admission");
    assert!(!accepted.replayed);
    let replay = store
        .bind_responsible_work_execution(&admission, &runtime_binding, binding)
        .expect("same scheduler admission is idempotent");
    assert!(replay.replayed);
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.fabric_work_deliveries("space-test").unwrap().len(), 1);
}

#[test]
fn membership_work_binding_authorizes_message_and_result_without_accepting_work() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    for member_id in ["worker-admission", "reviewer-admission"] {
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
    let worker_membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let reviewer_membership = join_runtime_membership(
        &store,
        "membership-reviewer-admission",
        "team-admission",
        "reviewer-admission",
        TeamMembershipRole::Member,
    );
    let worker_session = session("session-worker-admission", "worker-admission");
    let reviewer_session = session("session-reviewer-admission", "reviewer-admission");
    for session in [&worker_session, &reviewer_session] {
        store
            .create_agent_session(
                &service_context("session.create", &session.id, 0),
                session.clone(),
            )
            .unwrap();
    }
    admit_member_run(
        &store,
        canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
    );
    let assigned = assign_responsibility(&store, "work-report-message", &worker_membership.id);
    assert_eq!(assigned.active_member_run_id, None);

    let mut runtime_binding = runtime_command_fixture(
        "runtime-report-message",
        RuntimeCommandKind::StartCycle,
        &worker_session,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let binding = execution_binding(
        &assigned,
        &worker_membership,
        &worker_session,
        "binding-report-message",
    );
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-report-message", 0),
            &runtime_binding,
            binding.clone(),
        )
        .unwrap();
    let active = store
        .start_work(
            &assigned.id,
            assigned.version,
            "member-run-admission",
            firm_core::WorkCommandContext {
                event_id: "event-start-report-message".into(),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                    id: "member-run-admission".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-start-report-message".into(),
                created_at: "t-start".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    assert_eq!(active.phase, firm_core::WorkPhase::Active);
    assert_eq!(binding.work_revision + 1, active.version);

    create_direct_subscription(&store, "worker-admission", &reviewer_membership);
    let work_before_message = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == active.id)
        .unwrap();
    store
        .author_message(
            &service_context("message.author", "message-report-work", 0),
            work_message(
                "message-report-work",
                &active,
                "worker-admission",
                &worker_session.id,
                "reviewer-admission",
            ),
        )
        .expect("exact owner session may author a Work-linked Message");
    let work_after_message = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == active.id)
        .unwrap();
    assert_eq!(work_after_message, work_before_message);

    let before_foreign = store.canonical_operations().unwrap();
    let error = store
        .author_message(
            &service_context("message.author", "message-foreign-work", 0),
            work_message(
                "message-foreign-work",
                &active,
                "reviewer-admission",
                &reviewer_session.id,
                "worker-admission",
            ),
        )
        .expect_err("foreign member cannot use another member's Work binding");
    assert!(error.to_string().contains("UNAUTHORIZED_ACTOR"));
    assert_eq!(store.canonical_operations().unwrap(), before_foreign);

    let candidate = firm_core::agentfirm_api::CandidateRef {
        kind: firm_core::agentfirm_api::CandidateKind::GitCommit,
        value: "abcdef0123456789".into(),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(&candidate).unwrap());
    let report = WorkReport {
        id: "report-membership-work".into(),
        work_id: active.id.clone(),
        work_revision: active.version + 1,
        report_revision: 1,
        kind: WorkReportKind::Result,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        summary: "bounded result".into(),
        base_revision: None,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        evidence_refs: vec!["evidence://membership-work".into()],
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: "t-report".into(),
    };
    store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &report.id, 0),
            "team-admission",
            report,
        )
        .expect("exact owner binding may submit Result evidence");
    let submitted = store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == active.id)
        .unwrap();
    assert_eq!(submitted.phase, firm_core::WorkPhase::Review);
    assert_eq!(
        submitted.resolution, None,
        "WorkReport is not Host acceptance"
    );
    let current_deliveries = store
        .current_work_deliveries("space-test")
        .expect("ordinary Work lifecycle revisions keep the delivery readable");
    assert!(current_deliveries.iter().any(|delivery| {
        delivery.work_id == submitted.id
            && delivery.work_revision == binding.work_revision
            && delivery.work_execution_binding_id.as_deref() == Some(binding.id.as_str())
    }));

    store
        .release_work_execution_binding(
            &service_context("work_binding.release", "release-report-message", 1),
            &binding.id,
            "t-release",
        )
        .unwrap();
    let before_released = store.canonical_operations().unwrap();
    let progress = WorkReport {
        id: "report-after-release".into(),
        work_id: submitted.id.clone(),
        work_revision: submitted.version,
        report_revision: 1,
        kind: WorkReportKind::Progress,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        summary: "must reject".into(),
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
        created_at: "t-after-release".into(),
    };
    let error = store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &progress.id, 0),
            "team-admission",
            progress.clone(),
        )
        .expect_err("released binding cannot authorize more Work evidence");
    assert!(error.to_string().contains("WORK_EXECUTION_BINDING_ACTIVE"));
    assert_eq!(store.canonical_operations().unwrap(), before_released);
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
    let before_stale_session = store.canonical_operations().unwrap();
    let error = store
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
        .expect_err("stale binding generation cannot authorize a Work-linked Message");
    assert!(error.to_string().contains("NATIVE_SESSION_INCOMPATIBLE"));
    assert_eq!(store.canonical_operations().unwrap(), before_stale_session);
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
