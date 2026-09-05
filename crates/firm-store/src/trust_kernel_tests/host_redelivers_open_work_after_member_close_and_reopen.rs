use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;

/// Reproduce the exact defect sequence: a Work is assigned, its delivery
/// reaches the provider, the member never starts it, and the Host then closes
/// and reopens the member runtime. The frozen delivery can never be claimed
/// again, so the Host needs an explicit redelivery verb.
pub(super) struct ReopenedMemberFixture {
    pub(super) store: HarnessStore,
    pub(super) session: AgentSession,
    pub(super) membership: TeamMembership,
    pub(super) runtime_binding: firm_core::agentfirm_api::RuntimeCommandBinding,
    pub(super) member_run_id: String,
}

pub(super) fn host_context(
    store: &HarnessStore,
    event: &str,
    key: &str,
) -> firm_core::WorkCommandContext {
    let actor = store.exact_team_run_host_actor("run-admission").unwrap();
    firm_core::WorkCommandContext {
        event_id: event.into(),
        performed_by_actor: actor.clone(),
        authority_actor: Some(actor),
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: "t-redeliver".into(),
        duplicate_ok: false,
    }
}

pub(super) fn member_context(
    member_run_id: &str,
    event: &str,
    key: &str,
) -> firm_core::WorkCommandContext {
    firm_core::WorkCommandContext {
        event_id: event.into(),
        performed_by_actor: firm_core::TeamActorRef {
            kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.into(),
            display_name: None,
            authn_source: Some("test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: "t-member".into(),
        duplicate_ok: false,
    }
}

pub(super) fn reopened_member_fixture(suffix: &str) -> (ReopenedMemberFixture, PathBuf) {
    let (store, root) = fabric_store();
    let member_id = format!("worker-{suffix}");
    let member_run_id = format!("member-run-{suffix}");
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                &format!("identity-{suffix}"),
                0,
            ),
            identity(&member_id),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        &format!("membership-{suffix}"),
        "team-admission",
        &member_id,
        TeamMembershipRole::Member,
    );
    let session = session(&format!("session-{suffix}"), &member_id);
    store
        .create_agent_session(
            &service_context("session.create", &format!("session-{suffix}"), 0),
            session.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run(&member_run_id, &member_id, "run-admission"),
    );
    let mut runtime_binding = runtime_command_fixture(
        &format!("runtime-{suffix}"),
        RuntimeCommandKind::StartCycle,
        &session,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some(member_run_id.clone());
    runtime_binding.target_member_run_generation = Some(1);
    (
        ReopenedMemberFixture {
            store,
            session,
            membership,
            runtime_binding,
            member_run_id,
        },
        root,
    )
}

/// Bind, dispatch, and settle one provider receipt for `work` without the
/// member ever starting it.
pub(super) fn deliver_to_provider(
    fixture: &ReopenedMemberFixture,
    work: &firm_core::Work,
    suffix: &str,
) -> WorkExecutionBinding {
    let binding = execution_binding(
        work,
        &fixture.membership,
        &fixture.session,
        &format!("binding-{suffix}"),
    );
    fixture
        .store
        .bind_responsible_work_execution(
            &service_context("work.bind", &format!("binding-{suffix}"), 0),
            &fixture.runtime_binding,
            binding.clone(),
        )
        .unwrap();
    fixture
        .store
        .claim_work_for_provider(
            &service_context("work.claim", &format!("claim-{suffix}"), 0),
            &binding.delivery_id,
            &fixture.session.node_id,
            &fixture.session.node_daemon_id,
            fixture.session.node_daemon_generation,
            &format!("claim-{suffix}"),
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim",
        )
        .unwrap();
    fixture
        .store
        .record_work_provider_receipt(
            &service_context("work.receipt", &format!("receipt-{suffix}"), 0),
            &binding.delivery_id,
            &fixture.session.node_id,
            &fixture.session.node_daemon_id,
            fixture.session.node_daemon_generation,
            &format!("claim-{suffix}"),
            &format!("provider-receipt-{suffix}"),
            "t-receipt",
        )
        .unwrap();
    binding
}

/// Run the exact Host Close then Reopen sequence, releasing the old
/// WorkExecutionBinding and advancing the MemberRun generation 1 -> 2.
fn close_and_reopen_member(
    fixture: &ReopenedMemberFixture,
    binding: &WorkExecutionBinding,
    suffix: &str,
) {
    let store = &fixture.store;
    let close = firm_core::TeamMemberCloseRequest {
        id: format!("close-{suffix}"),
        team_run_id: "run-admission".into(),
        member_run_id: fixture.member_run_id.clone(),
        requested_by: "host".into(),
        reason: "member never started the delivered Work".into(),
        status: firm_core::TeamMemberCloseStatus::Pending,
        requested_at: "t-close-request".into(),
        applied_at: None,
        detached_recovery_fence: None,
    };
    store.latch_team_member_close(&close).unwrap();
    let (mut close_command, mut close_admission) = runtime_command_fixture(
        &format!("close-command-{suffix}"),
        RuntimeCommandKind::CloseMember,
        &fixture.session,
        "close_member",
    );
    close_command.binding.target_member_run_id = Some(close.member_run_id.clone());
    close_command.binding.target_member_run_generation = Some(1);
    close_command.payload["delivery_id"] =
        serde_json::Value::String(format!("{}:idle:close-runtime", close.id));
    close_command.payload_fingerprint = canonical_json_fingerprint(&close_command.payload);
    close_command.postcondition.desired_ack_level =
        firm_core::agentfirm_api::RuntimeAcknowledgementLevel::ProviderReceipt;
    close_command.postcondition.desired_postcondition =
        firm_core::agentfirm_api::RuntimeDesiredPostcondition::RuntimeReleased;
    close_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&close_command).unwrap());
    let prepared = store
        .prepare_runtime_command(
            &close_admission,
            &close_command,
            current_unix_ms(),
            "t-close-accepted",
        )
        .unwrap();
    store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "runtime.closemember.settle",
                &format!("close-command-{suffix}:settle"),
                prepared.projection.version,
            ),
            &close_command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({"closed": true})),
            None,
            "t-close-applied",
        )
        .unwrap();
    let released = store
        .release_work_execution_binding_for_member_close(
            &service_context(
                "work.release.close",
                &format!("release-{suffix}"),
                binding.version,
            ),
            &binding.id,
            &close.id,
            &close_command.id,
            &close.member_run_id,
            1,
            &fixture.session.node_id,
            &fixture.session.node_daemon_id,
            fixture.session.node_daemon_generation,
            "t-close-release",
        )
        .unwrap();
    assert_eq!(
        released.projection.status,
        WorkExecutionBindingStatus::Released
    );
    let predecessor = store
        .member_runs()
        .unwrap()
        .into_iter()
        .find(|member| member.id == close.member_run_id)
        .unwrap();
    let mut closed = predecessor.clone();
    closed.coordination_status = firm_core::MemberCoordinationStatus::Closed;
    closed.status = MemberRunStatus::Stopped;
    closed.finished_at = Some("t-close-terminal".into());
    store
        .compare_and_append_member_run(&predecessor, &closed)
        .unwrap();
    store
        .complete_team_member_close(
            &close.team_run_id,
            &close.member_run_id,
            &close.id,
            "t-close-complete",
        )
        .unwrap();
    let mut reopened = closed.clone();
    reopened.runtime_generation += 1;
    reopened.coordination_status = firm_core::MemberCoordinationStatus::Active;
    reopened.status = MemberRunStatus::Queued;
    reopened.started_at = "t-reopen".into();
    reopened.finished_at = None;
    store
        .compare_and_advance_member_run_generation(&closed, &reopened)
        .unwrap();
}

#[test]
fn host_redelivers_open_work_after_member_close_and_reopen() {
    let (fixture, root) = reopened_member_fixture("redeliver");
    let store = &fixture.store;
    let work = assign_responsibility(store, "work-redeliver", &fixture.membership.id);
    let binding = deliver_to_provider(&fixture, &work, "redeliver");
    assert_eq!(work.phase, firm_core::WorkPhase::Open);

    let wrong_space = store
        .redeliver_work_to_current_session(
            &work.id,
            work.version,
            "foreign-space",
            Some("wrong scope must not hide the live binding"),
            host_context(store, "redeliver-wrong-space", "redeliver-wrong-space"),
        )
        .expect_err("caller scope must match the Work's canonical TeamRun scope");
    assert!(
        wrong_space
            .to_string()
            .contains("EXECUTION_SPACE_SCOPE_MISMATCH"),
        "{wrong_space}"
    );

    // A live execution binding is not a redelivery case: the delivery can
    // still reach the provider on this exact generation.
    let live = store
        .redeliver_work_to_current_session(
            &work.id,
            work.version,
            "space-test",
            Some("too early"),
            host_context(store, "redeliver-live", "redeliver-live"),
        )
        .expect_err("a live execution binding is not a stale delivery");
    let live = live.to_string();
    assert!(live.contains("WORK_DELIVERY_LIVE"), "{live}");
    assert!(live.contains(&binding.id), "{live}");
    assert!(
        live.contains(&format!("generation {}", binding.binding_generation)),
        "{live}"
    );
    assert!(live.contains("GitHub #734"), "{live}");

    close_and_reopen_member(&fixture, &binding, "redeliver");

    // The exact defect: the provider receipt is immutable, so the ordinary
    // binding path refuses to replay the same revision and the Host has no
    // reassignment path either.
    assert!(store
        .provider_received_work_requires_host_reauthorization("space-test", &work.id, work.version)
        .unwrap());
    let reassign = store
        .assign_work_to_membership(
            &work.id,
            work.version,
            &fixture.membership.id,
            "space-test",
            host_context(store, "redeliver-reassign", "redeliver-reassign"),
        )
        .expect_err("reassigning the same membership is not an exit");
    assert!(
        reassign.to_string().contains("WORK_ALREADY_ASSIGNED"),
        "{reassign}"
    );
    let release = store
        .release_work_as_host(
            &work.id,
            work.version,
            host_context(store, "redeliver-release", "redeliver-release"),
        )
        .expect_err("releasing an already-accepted delivery is not an exit either");
    assert!(
        release.to_string().contains("RECONCILIATION_REQUIRED"),
        "{release}"
    );

    let before_deliveries = store.fabric_work_deliveries("space-test").unwrap();
    let redelivered = store
        .redeliver_work_to_current_session(
            &work.id,
            work.version,
            "space-test",
            Some("member reopened without starting the Work"),
            host_context(store, "redeliver-apply", "redeliver-apply"),
        )
        .expect("Host redelivery is the explicit new Work authority");
    assert_eq!(redelivered.version, work.version + 1);
    assert_eq!(redelivered.phase, firm_core::WorkPhase::Open);
    assert_eq!(
        redelivered.assignee_membership_id.as_deref(),
        Some(fixture.membership.id.as_str()),
        "redelivery never moves responsibility"
    );
    assert_eq!(
        store.fabric_work_deliveries("space-test").unwrap(),
        before_deliveries,
        "redelivery must never rewrite or delete provider delivery evidence"
    );

    let event = store
        .work_events()
        .unwrap()
        .into_iter()
        .find(|event| event.work_id == work.id && event.resulting_version == redelivered.version)
        .expect("redelivery appends one Work operation");
    assert_eq!(event.kind, firm_core::WorkEventKind::Rebound);
    assert_eq!(event.payload["redelivery"], serde_json::json!(true));
    let superseded = event.payload["superseded_deliveries"].as_array().unwrap();
    assert_eq!(superseded.len(), 1);
    assert_eq!(
        superseded[0]["delivery_id"].as_str(),
        Some(binding.delivery_id.as_str())
    );
    assert_eq!(
        superseded[0]["status"].as_str(),
        Some("provider_received"),
        "the superseded delivery keeps its exact provider evidence"
    );
    assert_eq!(
        superseded[0]["stale_because"].as_str(),
        Some("work_execution_binding_released")
    );
    assert_eq!(
        superseded[0]["provider_receipt_id"].as_str(),
        Some("provider-receipt-redeliver")
    );

    // The ordinary delivery path can now bind the reopened generation and
    // produce a new WorkDelivery, exactly as it does after `work assign`.
    assert!(!store
        .provider_received_work_requires_host_reauthorization(
            "space-test",
            &work.id,
            redelivered.version
        )
        .unwrap());
    let mut successor_runtime = fixture.runtime_binding.clone();
    successor_runtime.target_member_run_generation = Some(2);
    let mut successor = binding.clone();
    successor.id = "binding-redeliver-2".into();
    successor.binding_generation = 2;
    successor.delivery_id = format!("work-delivery:{}:2", work.id);
    successor.work_revision = redelivered.version;
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-redeliver-2", 0),
            &successor_runtime,
            successor.clone(),
        )
        .expect("the reopened generation binds the redelivered revision");
    let fresh = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == successor.delivery_id)
        .expect("redelivery produces a new WorkDelivery through the ordinary path");
    assert_eq!(fresh.status, WorkDeliveryStatus::Queued);
    assert_eq!(fresh.work_revision, redelivered.version);
    assert_eq!(
        fresh.recipient_session_generation,
        fixture.session.runtime_generation
    );

    // The stale row is still readable, unchanged, next to the new one.
    let stale = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == binding.delivery_id)
        .expect("the superseded delivery is preserved, never deleted");
    assert_eq!(stale.status, WorkDeliveryStatus::ProviderReceived);

    let replay = store
        .redeliver_work_to_current_session(
            &work.id,
            work.version,
            "space-test",
            Some("member reopened without starting the Work"),
            host_context(store, "redeliver-apply", "redeliver-apply"),
        )
        .expect("the exact retry is idempotent");
    assert_eq!(replay, redelivered);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn delivery_staleness_emits_only_the_reachable_token_set() {
    let (fixture, root) = reopened_member_fixture("staleness-tokens");
    let work = assign_responsibility(
        &fixture.store,
        "work-staleness-tokens",
        &fixture.membership.id,
    );
    let binding = execution_binding(
        &work,
        &fixture.membership,
        &fixture.session,
        "binding-staleness-tokens",
    );
    let cases = [
        (None, "work_execution_binding_missing"),
        (
            Some(WorkExecutionBindingStatus::Released),
            "work_execution_binding_released",
        ),
        (
            Some(WorkExecutionBindingStatus::Completed),
            "work_execution_binding_completed",
        ),
        (
            Some(WorkExecutionBindingStatus::Invalidated),
            "work_execution_binding_invalidated",
        ),
    ];
    for (status, expected) in cases {
        let candidate = status.map(|status| WorkExecutionBinding {
            status,
            ..binding.clone()
        });
        assert_eq!(
            crate::store_work_redelivery::delivery_staleness(candidate.as_ref()),
            expected
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn delivery_staleness_is_non_panicking_for_an_unexpected_live_binding() {
    let (fixture, root) = reopened_member_fixture("staleness-live-fallback");
    let work = assign_responsibility(
        &fixture.store,
        "work-staleness-live-fallback",
        &fixture.membership.id,
    );
    let binding = execution_binding(
        &work,
        &fixture.membership,
        &fixture.session,
        "binding-staleness-live-fallback",
    );

    assert_eq!(
        crate::store_work_redelivery::delivery_staleness(Some(&binding)),
        "work_execution_binding_live_unexpected"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn redeliver_refuses_started_and_terminal_work() {
    let (fixture, root) = reopened_member_fixture("guard");
    let store = &fixture.store;
    let work = assign_responsibility(store, "work-guard", &fixture.membership.id);
    let binding = deliver_to_provider(&fixture, &work, "guard");
    let started = store
        .start_work(
            &work.id,
            work.version,
            &fixture.member_run_id,
            member_context(&fixture.member_run_id, "guard-start", "guard-start"),
        )
        .unwrap();
    close_and_reopen_member(&fixture, &binding, "guard");

    let started_error = store
        .redeliver_work_to_current_session(
            &work.id,
            started.version,
            "space-test",
            None,
            host_context(store, "guard-started", "guard-started"),
        )
        .expect_err("a Work already in progress is not redeliverable");
    assert!(
        started_error.to_string().contains("WORK_ALREADY_STARTED"),
        "{started_error}"
    );

    let cancelled = store
        .cancel_work(
            &work.id,
            started.version,
            "abandoned",
            host_context(store, "guard-cancel", "guard-cancel"),
        )
        .unwrap();
    let terminal_error = store
        .redeliver_work_to_current_session(
            &work.id,
            cancelled.version,
            "space-test",
            None,
            host_context(store, "guard-terminal", "guard-terminal"),
        )
        .expect_err("a terminal Work is not redeliverable");
    assert!(
        terminal_error
            .to_string()
            .contains("WORK_TERMINAL_NOT_REDELIVERABLE"),
        "{terminal_error}"
    );

    let unassigned = assign_responsibility(store, "work-guard-open", &fixture.membership.id);
    let no_delivery = store
        .redeliver_work_to_current_session(
            &unassigned.id,
            unassigned.version,
            "space-test",
            None,
            host_context(store, "guard-no-delivery", "guard-no-delivery"),
        )
        .expect_err("a Work that was never delivered has nothing to supersede");
    assert!(
        no_delivery
            .to_string()
            .contains("WORK_HAS_NO_UNSTARTED_DELIVERY"),
        "{no_delivery}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
