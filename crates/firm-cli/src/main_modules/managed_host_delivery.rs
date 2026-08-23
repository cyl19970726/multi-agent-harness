use super::*;

pub(super) fn claim_managed_host_attentions_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    include_low_value: bool,
) -> CliResult<Vec<HostAttention>> {
    let Some((execution_space_id, session)) =
        managed_host_agent_session_for_member(ledger, member)?
    else {
        return Ok(Vec::new());
    };
    let claim_id = format!(
        "managed-host-attention:{}:{}:{}",
        member.id,
        member.runtime_generation,
        generated_id("claim")
    );
    store_conflict_as_usage(ledger.store.claim_managed_host_attention_batch(
        &execution_space_id,
        &ledger.run_id,
        &member.id,
        &session.id,
        session.runtime_generation,
        &session.node_daemon_id,
        session.node_daemon_generation,
        &claim_id,
        32,
        include_low_value,
        current_unix_ms_u64(),
        &now_string(),
    ))
}

pub(super) fn managed_host_agent_session_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
) -> CliResult<Option<(String, harness_core::agentfirm_api::AgentSession)>> {
    match store_conflict_as_usage(
        ledger
            .store
            .host_runtime_binding(&ledger.run_id, current_unix_ms_u64()),
    )? {
        harness_application::HostRuntimeBinding::ExternalInteractive(_) => Ok(None),
        harness_application::HostRuntimeBinding::Managed(binding) => {
            if binding.runtime.id != member.id
                || binding.team_supervisor.supervisor_id != ledger.supervisor_id
                || binding.team_supervisor.generation != ledger.supervisor_generation
                || binding.agent_session.control_state.runtime_residency
                    != harness_core::agentfirm_api::RuntimeResidency::Attached
            {
                return Err(CliError::Usage(
                    "HOST_RUNTIME_BINDING_FENCED: managed Host delivery used a foreign MemberRun or Supervisor generation"
                        .into(),
                ));
            }
            Ok(Some((
                binding.agent_session.execution_space_id.clone(),
                binding.agent_session,
            )))
        }
    }
}

pub(super) fn settle_managed_host_attentions(
    ledger: &TeamRunLedger,
    attentions: &[HostAttention],
    provider_receipt_id: &str,
) -> CliResult<()> {
    for attention in attentions {
        let claim_id = attention.claim_id.as_deref().ok_or_else(|| {
            CliError::Usage(format!(
                "managed HostAttention {} has no claim",
                attention.id
            ))
        })?;
        store_conflict_as_usage(ledger.store.complete_host_attention_claim(
            &attention.id,
            claim_id,
            provider_receipt_id,
            &now_string(),
        ))?;
    }
    Ok(())
}

pub(super) fn requeue_managed_host_attentions(
    ledger: &TeamRunLedger,
    attentions: &[HostAttention],
    reason: &str,
) -> CliResult<()> {
    for attention in attentions {
        if let Some(claim_id) = attention.claim_id.as_deref() {
            store_conflict_as_usage(ledger.store.fail_host_attention_claim(
                &attention.id,
                claim_id,
                reason,
                &now_string(),
            ))?;
        }
    }
    Ok(())
}
