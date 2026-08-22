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
    ensure_test_runtime_fabric(&store, &created, &lease);
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

    let host_actor = created.team_run.host_actor.clone().expect("Host actor");
    let work = store
        .insert_work(
            Work {
                id: "managed-host-attention-work".into(),
                team_run_id: created.team_run.id.clone(),
                accountable_team_id: Some(created.team_run.agent_team_id.clone()),
                assignee_membership_id: None,
                created_by_member_id: None,
                legacy_containment_ref: None,
                title: "Prove managed Host status delivery".into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "Host receives exact fenced status".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: Some(member.agent_member_id.clone()),
                active_member_run_id: Some(member.id.clone()),
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: host_actor.clone(),
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: "managed-host-attention-created".into(),
                performed_by_actor: host_actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "managed-host-attention-created".into(),
                created_at: now_string(),
                duplicate_ok: false,
            },
        )
        .expect("create Host-assigned Work");
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
    assert!(error.to_string().contains("NODE_DAEMON_GENERATION_FENCED"));
    std::fs::remove_dir_all(root).expect("cleanup");
}
