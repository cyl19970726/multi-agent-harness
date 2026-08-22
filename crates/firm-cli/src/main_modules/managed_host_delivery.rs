use super::*;

pub(super) fn claim_managed_host_attentions_for_member(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    include_low_value: bool,
) -> CliResult<Vec<HostAttention>> {
    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    if run.host_control_mode != HostControlMode::Managed {
        return Ok(Vec::new());
    }
    let team = latest_teams(&ledger.store)?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage("managed Host Team is missing".to_string()))?;
    if member.agent_member_id != team.host_agent_id || member.is_external_interactive() {
        return Ok(Vec::new());
    }
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    let sessions = ledger
        .store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.runtime_generation == member.runtime_generation
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_AMBIGUOUS: managed Host {} has {} current exact-generation sessions",
            member.agent_member_id,
            sessions.len()
        )));
    };
    let claim_id = format!(
        "managed-host-attention:{}:{}:{}",
        member.id,
        member.runtime_generation,
        generated_id("claim")
    );
    store_conflict_as_usage(ledger.store.claim_managed_host_attention_batch(
        &execution_space_id,
        &run.id,
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
