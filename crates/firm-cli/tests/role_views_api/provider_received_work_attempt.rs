use super::*;

pub(super) struct ProviderReceivedWorkAttemptInput<'a> {
    pub(super) store: &'a HarnessStore,
    pub(super) space_id: &'a str,
    pub(super) node_id: &'a str,
    pub(super) daemon: &'a harness_core::NodeDaemonLease,
    pub(super) member_run_id: &'a str,
    pub(super) work: &'a harness_core::Work,
    pub(super) team: &'a harness_core::AgentTeam,
    pub(super) membership: &'a harness_core::agentfirm_api::TeamMembership,
    pub(super) worker_id: &'a str,
    pub(super) session: &'a AgentSession,
    pub(super) binding_generation: u64,
}

pub(super) struct ProviderReceivedWorkAttempt {
    pub(super) binding_id: String,
    pub(super) delivery_id: String,
    pub(super) provider_receipt_id: String,
}

pub(super) fn admit_provider_received_work_attempt(
    input: ProviderReceivedWorkAttemptInput<'_>,
) -> ProviderReceivedWorkAttempt {
    let binding_id = format!(
        "work-binding:{}:{}",
        input.work.id, input.binding_generation
    );
    let delivery_id = format!(
        "work-delivery:{}:{}",
        input.work.id, input.binding_generation
    );
    let claim_id = format!("claim:{delivery_id}");
    let provider_receipt_id = format!("provider-receipt:{delivery_id}");
    let daemon_context = |command_name: &str, idempotency_key: String| MutationContext {
        execution_space_id: input.space_id.into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: input.daemon.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: command_name.into(),
        idempotency_key,
        expected_version: 0,
        request_fingerprint: None,
    };
    let runtime_binding = RuntimeCommandBinding {
        target_member_run_id: Some(input.member_run_id.into()),
        target_member_run_generation: Some(1),
        target_session_id: Some(input.session.id.clone()),
        target_runtime_generation: Some(input.session.runtime_generation),
        target_driver_generation: Some(input.session.control_state.driver_generation),
        target_driver: input.session.control_state.driver_ref.clone(),
        native_session_ref: input.session.native_session_ref.clone(),
        composition_fingerprint: input.session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: input.session.control_state.capability_fingerprint.clone(),
        capability_profile_version: None,
        permission_envelope_ref: Some(input.session.permission_envelope_ref.clone()),
    };
    input
        .store
        .bind_responsible_work_execution(
            &daemon_context("test.work_execution.bind", binding_id.clone()),
            &runtime_binding,
            WorkExecutionBinding {
                id: binding_id.clone(),
                work_id: input.work.id.clone(),
                work_revision: input.work.version,
                team_id: input.team.id.clone(),
                team_membership_id: input.membership.id.clone(),
                agent_member_id: input.worker_id.into(),
                agent_session_id: input.session.id.clone(),
                agent_session_generation: input.session.runtime_generation,
                delivery_id: delivery_id.clone(),
                binding_generation: input.binding_generation,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: ActorRef {
                    kind: ActorKind::Service,
                    id: input.daemon.daemon_id.clone(),
                },
                bound_at: format!("unix-ms:bind:{}", input.binding_generation),
                ended_at: None,
            },
        )
        .expect("exact Work execution admission");
    input
        .store
        .claim_work_for_provider(
            &daemon_context("test.work.claim", claim_id.clone()),
            &delivery_id,
            input.node_id,
            &input.daemon.daemon_id,
            input.daemon.generation,
            &claim_id,
            RuntimeDispatchMode::QueueOnly,
            &format!("unix-ms:claim:{}", input.binding_generation),
        )
        .expect("exact daemon claims Work before member Result");
    input
        .store
        .record_work_provider_receipt(
            &daemon_context("test.work.receipt", provider_receipt_id.clone()),
            &delivery_id,
            input.node_id,
            &input.daemon.daemon_id,
            input.daemon.generation,
            &claim_id,
            &provider_receipt_id,
            &format!("unix-ms:receipt:{}", input.binding_generation),
        )
        .expect("provider receipt precedes semantic member Result");
    ProviderReceivedWorkAttempt {
        binding_id,
        delivery_id,
        provider_receipt_id,
    }
}

pub(super) fn assert_released_provider_received_attempt(
    store: &HarnessStore,
    space_id: &str,
    attempt: &ProviderReceivedWorkAttempt,
) {
    let binding = store
        .fabric_work_execution_bindings(space_id)
        .expect("bindings after member Result")
        .into_iter()
        .find(|binding| binding.id == attempt.binding_id)
        .expect("submitted Work binding remains durable");
    assert_eq!(binding.status, WorkExecutionBindingStatus::Released);
    let delivery = store
        .fabric_work_deliveries(space_id)
        .expect("deliveries after member Result")
        .into_iter()
        .find(|delivery| delivery.id == attempt.delivery_id)
        .expect("submitted Work delivery remains durable");
    assert_eq!(delivery.status, WorkDeliveryStatus::ProviderReceived);
    assert_eq!(
        delivery.provider_receipt_id.as_deref(),
        Some(attempt.provider_receipt_id.as_str()),
        "semantic Result must preserve provider receipt evidence"
    );
}
