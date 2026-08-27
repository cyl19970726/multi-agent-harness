use super::*;

fn rewrite_work_active_member_run(
    root: &std::path::Path,
    work_id: &str,
    member_run_id: Option<&str>,
) {
    let path = root.join("work_operations.jsonl");
    let rewritten = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["work"]["id"] == work_id {
                row["work"]["active_member_run_id"] = member_run_id
                    .map(|id| serde_json::Value::String(id.into()))
                    .unwrap_or(serde_json::Value::Null);
            }
            serde_json::to_string(&row).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, format!("{rewritten}\n")).unwrap();
}

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
    let (store, root) = fabric_store();
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

    rewrite_work_active_member_run(&root, &work.id, Some("member-run-admission"));
    let before_equal_legacy = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(&admission, &runtime_binding, binding.clone())
        .expect_err("even equal legacy runtime identity cannot authorize a new binding");
    assert!(
        error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"),
        "unexpected equal-legacy binding rejection: {error}"
    );
    assert_eq!(store.canonical_operations().unwrap(), before_equal_legacy);
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .is_empty());
    assert!(store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .is_empty());
    rewrite_work_active_member_run(&root, &work.id, None);

    for mut foreign in [
        context("foreign-human", "work.bind", "foreign-human-bind", 0),
        service_context("work.bind", "foreign-service-bind", 0),
    ] {
        if foreign.authenticated_actor.kind == ActorKind::Service {
            foreign.authenticated_actor.id = "foreign-daemon".into();
        }
        let before_operations = store.canonical_operations().unwrap();
        let before_bindings = store.fabric_work_execution_bindings("space-test").unwrap();
        let before_deliveries = store.fabric_work_deliveries("space-test").unwrap();
        let error = store
            .bind_responsible_work_execution(&foreign, &runtime_binding, binding.clone())
            .expect_err("only the exact current NodeDaemon may bind Work execution");
        assert!(error.to_string().contains("UNAUTHORIZED_ACTOR"));
        assert_eq!(store.canonical_operations().unwrap(), before_operations);
        assert_eq!(
            store.fabric_work_execution_bindings("space-test").unwrap(),
            before_bindings
        );
        assert_eq!(
            store.fabric_work_deliveries("space-test").unwrap(),
            before_deliveries
        );
    }

    let mut stale = runtime_binding.clone();
    stale.target_member_run_generation = Some(2);
    let before_stale = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(&admission, &stale, binding.clone())
        .expect_err("stale MemberRun generation must not bind Work");
    assert!(
        error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"),
        "unexpected legacy WorkReport rejection: {error}"
    );
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
fn terminal_member_runtime_cannot_bind_or_claim_provider_work() {
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
    let live_run =
        canonical_member_run("member-run-admission", "worker-admission", "run-admission");
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            live_run.clone(),
        )
        .unwrap();
    let mut runtime_binding = runtime_command_fixture(
        "runtime-terminal",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some(live_run.id.clone());
    runtime_binding.target_member_run_generation = Some(live_run.runtime_generation);
    let first_work = assign_responsibility(&store, "work-before-failure", &membership.id);
    let first_binding =
        execution_binding(&first_work, &membership, &target, "binding-before-failure");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-before-failure", 0),
            &runtime_binding,
            first_binding.clone(),
        )
        .expect("live runtime admits the exact binding");
    let side_released_work = assign_responsibility(&store, "work-side-released", &membership.id);
    let side_released_binding = execution_binding(
        &side_released_work,
        &membership,
        &target,
        "binding-side-released",
    );
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-side-released", 0),
            &runtime_binding,
            side_released_binding.clone(),
        )
        .expect("a second live binding is admitted for side-record release coverage");
    let uncertain_work = assign_responsibility(&store, "work-uncertain-claim", &membership.id);
    let uncertain_binding = execution_binding(
        &uncertain_work,
        &membership,
        &target,
        "binding-uncertain-claim",
    );
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-uncertain-claim", 0),
            &runtime_binding,
            uncertain_binding.clone(),
        )
        .unwrap();
    store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-uncertain", 0),
            &uncertain_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-uncertain",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-uncertain",
        )
        .expect("claim is admitted before the runtime becomes terminal");
    let reconcile_context = service_context("work.reconcile", "binding-current", 1);
    let operations_before_current = store.canonical_operations().unwrap();
    assert!(matches!(
        store
            .release_work_execution_binding_if_stale(
                &reconcile_context,
                &first_binding.id,
                &target.node_id,
                &target.node_daemon_id,
                target.node_daemon_generation,
                "t-current",
            )
            .unwrap(),
        WorkExecutionBindingReconciliation::Current
    ));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_current
    );
    let stale_generation_error = store
        .release_work_execution_binding_if_stale(
            &service_context("work.reconcile", "binding-stale-daemon", 1),
            &first_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation + 1,
            "t-stale",
        )
        .expect_err("a stale caller generation cannot reconcile the current binding");
    assert!(stale_generation_error
        .to_string()
        .contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_current
    );

    let corrupt_work = assign_responsibility(&store, "work-missing-delivery", &membership.id);
    let corrupt_binding = execution_binding(
        &corrupt_work,
        &membership,
        &target,
        "binding-missing-delivery",
    );
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &service_context("work.bind.corrupt", "binding-missing-delivery", 0),
                "work_execution_binding",
                &corrupt_binding.id,
                "bound",
                serde_json::json!({"runtime_binding": runtime_binding}),
                &corrupt_binding,
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
    }
    let before_missing_delivery = store.canonical_operations().unwrap();
    let missing_delivery_error = store
        .release_work_execution_binding_if_stale(
            &service_context("work.reconcile", "binding-missing-delivery", 1),
            &corrupt_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-missing-delivery",
        )
        .expect_err("missing canonical delivery evidence must fail closed");
    assert!(missing_delivery_error
        .to_string()
        .contains("missing its canonical WorkDelivery source fact"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_missing_delivery
    );

    let mut failed_run = live_run;
    failed_run.runtime_status = MemberRuntimeStatus::Failed;
    failed_run.version += 1;
    failed_run.finished_at = Some("t-failed".into());
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &context("host", "member_run.fail", "member-run-admission-failed", 1),
                "member_run",
                &failed_run.id,
                "runtime_failed",
                serde_json::json!({"runtime_status": "failed"}),
                &failed_run,
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
    }

    let terminal_work = assign_responsibility(&store, "work-after-failure", &membership.id);
    let before_terminal_bindings = store.fabric_work_execution_bindings("space-test").unwrap();
    let before_terminal_deliveries = store.fabric_work_deliveries("space-test").unwrap();
    let before_terminal_operations = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-after-failure", 0),
            &runtime_binding,
            execution_binding(
                &terminal_work,
                &membership,
                &target,
                "binding-after-failure",
            ),
        )
        .expect_err("failed runtime cannot create execution authority");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_terminal_operations
    );
    assert_eq!(
        store.fabric_work_execution_bindings("space-test").unwrap(),
        before_terminal_bindings
    );
    assert_eq!(
        store.fabric_work_deliveries("space-test").unwrap(),
        before_terminal_deliveries
    );

    let before_claim_operations = store.canonical_operations().unwrap();
    let before_claim_commands = store.runtime_commands("space-test").unwrap();
    let before_claim_delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == first_binding.delivery_id)
        .unwrap();
    let error = store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-after-failure", 0),
            &first_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-after-failure",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim",
        )
        .expect_err("runtime failure after bind must fence provider claim");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_claim_operations
    );
    assert_eq!(
        store.runtime_commands("space-test").unwrap(),
        before_claim_commands
    );
    let after_claim_delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == first_binding.delivery_id)
        .unwrap();
    assert_eq!(after_claim_delivery, before_claim_delivery);
    assert_eq!(after_claim_delivery.status, WorkDeliveryStatus::Queued);
    assert_eq!(after_claim_delivery.claim_id, None);

    let before_uncertain_release = store.canonical_operations().unwrap();
    let uncertain_delivery_before = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == uncertain_binding.delivery_id)
        .unwrap();
    let uncertain_error = store
        .release_work_execution_binding_if_stale(
            &service_context("work.reconcile", "binding-uncertain-release", 1),
            &uncertain_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-uncertain-release",
        )
        .expect_err("a claimed delivery without receipt requires reconciliation");
    assert!(uncertain_error
        .to_string()
        .contains("DELIVERY_RECOVERY_UNCERTAIN"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_uncertain_release
    );
    assert_eq!(
        store
            .fabric_work_deliveries("space-test")
            .unwrap()
            .into_iter()
            .find(|delivery| delivery.id == uncertain_binding.delivery_id)
            .unwrap(),
        uncertain_delivery_before
    );

    let release_context = service_context("work.reconcile", "binding-terminal-release", 1);
    let released = store
        .release_work_execution_binding_if_stale(
            &release_context,
            &first_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-release",
        )
        .expect("the exact daemon atomically rechecks and releases the stale binding");
    assert!(matches!(
        released,
        WorkExecutionBindingReconciliation::Released(result) if !result.replayed
    ));
    let operation_count_after_release = store.canonical_operations().unwrap().len();
    let stale_replay_error = store
        .release_work_execution_binding_if_stale(
            &release_context,
            &first_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation + 1,
            "t-release",
        )
        .expect_err("a stale daemon generation cannot replay a successful release");
    assert!(stale_replay_error
        .to_string()
        .contains("SUPERVISOR_GENERATION_FENCED"));
    let replay = store
        .release_work_execution_binding_if_stale(
            &release_context,
            &first_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-release",
        )
        .expect("the same exact release context replays idempotently");
    assert!(matches!(
        replay,
        WorkExecutionBindingReconciliation::Released(result) if result.replayed
    ));
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        operation_count_after_release
    );

    let mut stopped_side_projection = side_released_binding.clone();
    stopped_side_projection.status = WorkExecutionBindingStatus::Released;
    stopped_side_projection.version += 1;
    stopped_side_projection.ended_at = Some("t-session-stop".into());
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &service_context("runtime.stop", "runtime-stop-side-release", 0),
                "runtime_command",
                "runtime-stop-side-release",
                "applied",
                serde_json::json!({"session_id": target.id}),
                &serde_json::json!({"id": "runtime-stop-side-release", "version": 1}),
                vec![serde_json::to_value(&stopped_side_projection).unwrap()],
                Vec::new(),
            )
            .unwrap();
    }
    let before_settled_reconcile = store.canonical_operations().unwrap();
    let settled = store
        .release_work_execution_binding_if_stale(
            &service_context("work.reconcile", "binding-side-already-settled", 2),
            &side_released_binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-late-reconcile",
        )
        .expect("side-record release is observed as already settled");
    assert!(matches!(
        settled,
        WorkExecutionBindingReconciliation::AlreadySettled(ref binding)
            if binding.status == WorkExecutionBindingStatus::Released
                && binding.version == stopped_side_projection.version
    ));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_settled_reconcile
    );
}

#[test]
fn membership_work_binding_authorizes_message_and_result_without_accepting_work() {
    let (store, root) = fabric_store();
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
    store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-report-message", 0),
            &binding.delivery_id,
            &worker_session.node_id,
            &worker_session.node_daemon_id,
            worker_session.node_daemon_generation,
            "claim-report-message",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-report-message",
        )
        .expect("claim exact delivery before starting provider execution");
    store
        .record_work_provider_receipt(
            &service_context("work.receipt", "receipt-report-message", 0),
            &binding.delivery_id,
            &worker_session.node_id,
            &worker_session.node_daemon_id,
            worker_session.node_daemon_generation,
            "claim-report-message",
            "provider-receipt-report-message",
            "t-receipt-report-message",
        )
        .expect("record exact provider receipt before semantic Result submission");
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

    let mut terminal_projection = active.clone();
    terminal_projection.phase = firm_core::WorkPhase::Closed;
    terminal_projection.resolution = Some(firm_core::WorkResolution::Cancelled);
    terminal_projection.version += 1;
    terminal_projection.active_member_run_id = Some("forged-reopened-runtime".into());
    let terminal_event = firm_core::agentfirm_api::CanonicalMutationEvent {
        id: "terminal-member-event".into(),
        aggregate_kind: "work".into(),
        aggregate_id: terminal_projection.id.clone(),
        sequence: terminal_projection.version,
        store_sequence: 999,
        transition: "cancelled".into(),
        expected_version: terminal_projection.version - 1,
        resulting_version: terminal_projection.version,
        performed_by_actor: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: "terminal-member-event".into(),
        canonical_request_fingerprint: "sha256:test".into(),
        payload: serde_json::json!({}),
        created_at: "t-terminal".into(),
    };
    let terminal_attention = store
        .canonical_terminal_work_outbox_unlocked(&terminal_projection, &terminal_event)
        .expect("terminal provenance resolves from exact admission evidence");
    assert_eq!(terminal_attention.len(), 1);
    assert_eq!(
        terminal_attention[0].member_run_id.as_deref(),
        Some("member-run-admission"),
        "legacy runtime identity and a later same-member generation cannot replace immutable admission provenance"
    );

    create_direct_subscription(&store, "worker-admission", &reviewer_membership);
    rewrite_work_active_member_run(&root, &active.id, Some("member-run-admission"));
    let before_equal_legacy = store.canonical_operations().unwrap();
    let legacy_progress = WorkReport {
        id: "report-equal-legacy".into(),
        work_id: active.id.clone(),
        work_revision: active.version,
        report_revision: 1,
        kind: WorkReportKind::Progress,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        summary: "must reject legacy runtime authority".into(),
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
        created_at: "t-equal-legacy".into(),
    };
    let error = store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &legacy_progress.id, 0),
            "team-admission",
            legacy_progress,
        )
        .expect_err("equal legacy runtime identity cannot authorize Work evidence");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    let error = store
        .author_message(
            &service_context("message.author", "message-equal-legacy", 0),
            work_message(
                "message-equal-legacy",
                &active,
                "worker-admission",
                &worker_session.id,
                "reviewer-admission",
            ),
        )
        .expect_err("equal legacy runtime identity cannot authorize a Work-linked Message");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_equal_legacy);
    let error = store
        .current_work_deliveries("space-test")
        .expect_err("mixed legacy Work cannot project a current delivery");
    assert!(error
        .to_string()
        .contains("CURRENT_WORK_DELIVERY_CANONICAL_JOIN_CONFLICT"));
    rewrite_work_active_member_run(&root, &active.id, None);
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
            report.clone(),
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
    let released_binding = store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == binding.id)
        .unwrap();
    assert_eq!(
        released_binding.status,
        WorkExecutionBindingStatus::Released,
        "Result submission atomically releases exact execution authority"
    );
    let provider_received_delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == binding.delivery_id)
        .unwrap();
    assert_eq!(
        provider_received_delivery.status,
        WorkDeliveryStatus::ProviderReceived,
        "Result submission preserves provider receipt evidence"
    );
    let current_deliveries = store
        .current_work_deliveries("space-test")
        .expect("ordinary Work lifecycle revisions keep the delivery readable");
    assert!(current_deliveries.iter().any(|delivery| {
        delivery.work_id == submitted.id
            && delivery.work_revision == binding.work_revision
            && delivery.work_execution_binding_id.as_deref() == Some(binding.id.as_str())
    }));

    let operation_count_after_release = store.canonical_operations().unwrap().len();
    let exact_report_replay = store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &report.id, 0),
            "team-admission",
            report.clone(),
        )
        .expect("the exact member replays Result after atomic binding release");
    assert!(exact_report_replay.replayed);
    let mut foreign_replay_context =
        member_context("worker-admission", "report.create", &report.id, 0);
    foreign_replay_context.authenticated_actor.id = "reviewer-admission".into();
    let foreign_replay_error = store
        .create_trust_work_report(&foreign_replay_context, "team-admission", report)
        .expect_err("a foreign member cannot replay another member's Result");
    assert!(foreign_replay_error
        .to_string()
        .contains("UNAUTHORIZED_ACTOR"));
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        operation_count_after_release
    );
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
fn submitted_attention_provenance_missing_or_duplicated_fails_closed() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-provenance", "run-provenance");
    let work = insert_runtime_work(
        &store,
        "work-provenance",
        "team-provenance",
        "run-provenance",
    );
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-provenance",
                0,
            ),
            identity("worker-provenance"),
        )
        .unwrap();
    join_runtime_membership(
        &store,
        "membership-worker-provenance",
        "team-provenance",
        "worker-provenance",
        TeamMembershipRole::Member,
    );
    admit_member_run(
        &store,
        canonical_member_run(
            "member-run-provenance",
            "worker-provenance",
            "run-provenance",
        ),
    );
    let mut terminal = work.clone();
    terminal.phase = firm_core::WorkPhase::Closed;
    terminal.resolution = Some(firm_core::WorkResolution::Accepted);
    terminal.version = work.version + 1;
    let attention = |id: &str, member_run_id: Option<&str>| HostAttention {
        id: id.into(),
        team_run_id: work.team_run_id.clone(),
        kind: HostAttentionKind::WorkReviewRequested,
        work_id: work.id.clone(),
        work_version: work.version,
        source_event_ref: format!("source-{id}"),
        member_run_id: member_run_id.map(str::to_string),
        status: HostAttentionStatus::Actionable,
        attempt: 0,
        claim_id: None,
        claimed_host_surface: None,
        claimed_host_thread_id: None,
        claimed_host_lease_id: None,
        claimed_host_lease_generation: None,
        claimed_host_lease_owner_id: None,
        claimed_recipient_member_run_id: None,
        claimed_recipient_session_id: None,
        claimed_recipient_session_generation: None,
        claimed_node_daemon_id: None,
        claimed_node_daemon_generation: None,
        provider_receipt_id: None,
        last_failure_reason: None,
        created_at: "t-attention".into(),
        updated_at: "t-attention".into(),
    };
    store
        .ensure_host_attention(&attention("attention-missing", None))
        .unwrap();
    let error = store
        .terminal_work_member_run_provenance_unlocked(&terminal)
        .expect_err("submitted attention without provenance must not fall back to a binding");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));

    store
        .ensure_host_attention(&attention("attention-valid", Some("member-run-provenance")))
        .unwrap();
    let error = store
        .terminal_work_member_run_provenance_unlocked(&terminal)
        .expect_err("mixed valid and missing submitted attentions are ambiguous");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
}

#[path = "work_responsibility_execution_admission_edge_tests.rs"]
mod edge_tests;
