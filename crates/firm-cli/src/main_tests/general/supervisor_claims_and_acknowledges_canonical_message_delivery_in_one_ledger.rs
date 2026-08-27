use super::*;

#[test]
fn supervisor_claims_and_acknowledges_canonical_message_delivery_in_one_ledger() {
    let (store, root) = temp_store("canonical-supervisor-message-delivery");
    let created = create_two_member_team_run(&store);
    let host = created
        .member_runs
        .iter()
        .find(|member| {
            created
                .team_run
                .host_actor
                .as_ref()
                .is_some_and(|actor| actor.id == member.agent_member_id)
        })
        .expect("exact Host MemberRun")
        .clone();
    let member = created.member_runs[0].clone();
    assert_eq!(
        created
            .team_run
            .host_actor
            .as_ref()
            .map(|actor| actor.id.as_str()),
        Some(host.agent_member_id.as_str()),
        "the recipient is the exact managed Host AgentMember"
    );
    assert_eq!(created.team_run.host_control_mode, HostControlMode::Managed);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-supervisor",
            std::process::id(),
            "test://canonical-supervisor",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire supervisor lease");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    assert_ne!(member.id, host.id, "ordinary Member and Host are distinct");
    assert!(
        claim_managed_host_attentions_for_member(&ledger, &member, true)
            .expect("ordinary Member is a no-op before the Host session exists")
            .is_empty(),
        "ordinary Member must not resolve or claim the managed Host session"
    );
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("read TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("read members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    bind_team_runtime_supervisor(
        &store,
        &PreparedTeamRunBody {
            run_id: run.id.clone(),
            objective: run.objective.clone(),
            run,
            members,
        },
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind exact managed Host Supervisor driver");
    transition_provider_session_runtime_control(
        &ledger,
        &host,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        harness_core::agentfirm_api::RuntimeActivity::Idle,
    )
    .expect("attach managed Host provider handle");
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "canonical-supervisor-message",
        &member.agent_member_id,
        &host.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "deliver through the NodeDaemon supervisor",
        "canonical-supervisor-correlation",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );

    let claimed = claim_canonical_messages_for_member(&ledger, &host)
        .expect("supervisor claim")
        .pop()
        .expect("one claimed message");
    let after_claim = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("delivery after claim")
        .into_iter()
        .find(|delivery| delivery.message_id == claimed.id)
        .expect("claimed delivery");
    assert_eq!(
        after_claim.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Claimed
    );
    assert_eq!(
        after_claim.claimed_node_daemon_generation,
        Some(lease.node_daemon_generation)
    );
    assert!(after_claim.recipient_session_id.is_some());
    assert_eq!(
        after_claim.recipient_session_generation,
        Some(member.runtime_generation)
    );

    mark_message_delivered(
        &ledger,
        &claimed,
        &host.id,
        &host.name,
        "provider-receipt-canonical",
    )
    .expect("provider receipt and acknowledgement");
    let acknowledged = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("delivery after acknowledgement")
        .into_iter()
        .find(|delivery| delivery.message_id == claimed.id)
        .expect("acknowledged delivery");
    assert_eq!(
        acknowledged.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Acknowledged
    );
    assert_eq!(
        acknowledged.provider_receipt_id.as_deref(),
        Some("provider-receipt-canonical")
    );
    assert!(store
        .legacy_team_messages()
        .expect("legacy TeamMessages")
        .iter()
        .all(|message| message.id != claimed.id));

    author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "canonical-host-provider-request",
        &member.agent_member_id,
        &host.agent_member_id,
        harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest,
        "{\"interaction_type\":\"question\"}",
        "canonical-host-provider-request-correlation",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    let provider_request = canonical_team_messages_for_run(&store, &created.team_run.id)
        .expect("canonical messages")
        .into_iter()
        .find(|message| message.id == "canonical-host-provider-request")
        .expect("provider request projection");
    let missing_receipt =
        acknowledge_provider_request_as_host(&store, &created.team_run.id, &provider_request)
            .expect_err("HTTP Host answer cannot fabricate provider receipt");
    assert!(missing_receipt
        .to_string()
        .contains("HOST_PROVIDER_RECEIPT_REQUIRED"));
    let still_queued = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("queued Host delivery")
        .into_iter()
        .find(|delivery| delivery.message_id == provider_request.id)
        .expect("Host delivery exists");
    assert_eq!(
        still_queued.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
    );
    assert!(still_queued.provider_receipt_id.is_none());

    claim_canonical_messages_for_member(&ledger, &host)
        .expect("real Host provider claim")
        .into_iter()
        .find(|message| message.id == provider_request.id)
        .expect("provider request claimed for exact Host session");
    let claimed_request = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("claimed Host delivery")
        .into_iter()
        .find(|delivery| delivery.message_id == provider_request.id)
        .expect("claimed provider request delivery");
    let host_binding = store
        .host_runtime_binding(&created.team_run.id, current_unix_ms_u64())
        .expect("exact managed Host binding");
    let harness_application::HostRuntimeBinding::Managed(host_binding) = host_binding else {
        panic!("test Team uses managed Host")
    };
    store
        .record_message_provider_receipt(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "node_daemon.test.host_provider_receipt",
                "canonical-host-provider-request:receipt".into(),
                0,
            ),
            &claimed_request.id,
            &host_binding.agent_session.node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            claimed_request.claim_id.as_deref().expect("claim id"),
            "provider-native-host-request-receipt",
            &now_string(),
        )
        .expect("record genuine provider receipt");
    acknowledge_provider_request_as_host(&store, &created.team_run.id, &provider_request)
        .expect("exact Host acknowledges genuine provider receipt");
    let acknowledged_request = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("acknowledged Host delivery")
        .into_iter()
        .find(|delivery| delivery.message_id == provider_request.id)
        .expect("acknowledged provider request delivery");
    assert_eq!(
        acknowledged_request.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Acknowledged
    );
    assert_eq!(
        acknowledged_request.provider_receipt_id.as_deref(),
        Some("provider-native-host-request-receipt")
    );

    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: "managed-host-attention-work".into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: "Prove managed Host status delivery".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "Host receives exact fenced status".into(),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: WorkCommandContext {
                event_id: "managed-host-attention-created".into(),
                performed_by_actor: created.team_run.host_actor.clone().expect("Host actor"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "managed-host-attention-created".into(),
                created_at: now_string(),
                duplicate_ok: false,
            },
        })
        .expect("create Work");
    let work =
        assign_test_work_to_member(&store, &lease.execution_space_id, &created, &member, &work);
    bind_test_responsible_work_execution(&store, &lease, &member, &work);
    let member_context = |event: &str| WorkCommandContext {
        event_id: event.into(),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member.id.clone(),
            display_name: Some(member.name.clone()),
            authn_source: Some("test:managed-host-attention".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: event.into(),
        created_at: now_string(),
        duplicate_ok: false,
    };
    let started = store
        .start_work(
            &work.id,
            work.version,
            &member.id,
            member_context("managed-host-attention-started"),
        )
        .expect("member start emits batched Host status");
    assert!(
        claim_managed_host_attentions_for_member(&ledger, &member, true)
            .expect("ordinary Member does not claim pending Host attention")
            .is_empty(),
        "pending HostAttention remains exclusive to the exact Host MemberRun"
    );
    let claimed_attention = claim_managed_host_attentions_for_member(&ledger, &host, true)
        .expect("managed Host claims status attention")
        .pop()
        .expect("one managed Host attention");
    assert_eq!(
        claimed_attention.kind,
        harness_core::HostAttentionKind::WorkChanged
    );
    assert_eq!(claimed_attention.work_id, work.id);
    assert_eq!(
        claimed_attention.claimed_recipient_member_run_id.as_deref(),
        Some(host.id.as_str())
    );
    assert!(claimed_attention.claimed_recipient_session_id.is_some());
    assert_eq!(
        claimed_attention.claimed_node_daemon_generation,
        Some(lease.node_daemon_generation)
    );
    claimed_attention
        .validate()
        .expect("managed claim is contract-valid");
    settle_managed_host_attentions(
        &ledger,
        std::slice::from_ref(&claimed_attention),
        "provider-receipt-managed-attention",
    )
    .expect("exact managed Host receipt settles");

    store
        .block_work(
            &started.id,
            started.version,
            &member.id,
            "wait for dependency",
            member_context("managed-host-attention-blocked"),
        )
        .expect("member block emits urgent Host status");
    let stale_claim = claim_managed_host_attentions_for_member(&ledger, &host, false)
        .expect("claim before daemon succession")
        .pop()
        .expect("one stale-generation proof claim");
    assert_eq!(
        stale_claim.kind,
        harness_core::HostAttentionKind::WorkBlocked
    );
    let node_lease = store
        .latest_node_daemon_lease(&created.team_run.execution_node_id)
        .expect("read NodeDaemon lease")
        .expect("NodeDaemon lease exists");
    store
        .release_node_daemon_lease(
            &node_lease.node_id,
            &node_lease.daemon_id,
            node_lease.generation,
            &node_lease.instance_id,
            current_unix_ms_u64(),
        )
        .expect("release old daemon generation");
    store
        .acquire_node_daemon_lease(
            &node_lease.node_id,
            "successor-daemon",
            "successor-instance",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire successor daemon generation");
    let error = store
        .complete_host_attention_claim(
            &stale_claim.id,
            stale_claim.claim_id.as_deref().expect("claim id"),
            "forbidden-stale-receipt",
            &now_string(),
        )
        .expect_err("successor daemon must fence stale Host delivery settlement");
    assert!(
        error.to_string().contains("HOST_RUNTIME_SUPERVISOR_FENCED"),
        "central HostRuntimeBinding must fence the stale daemon/supervisor pair: {error}"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
