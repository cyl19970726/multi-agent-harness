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
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeDriverRef, RuntimeResidency};

    let run = latest_team_run(&ledger.store, &ledger.run_id)?;
    if run.host_control_mode != HostControlMode::Managed {
        return Ok(None);
    }
    let team = latest_teams(&ledger.store)?
        .remove(&run.agent_team_id)
        .ok_or_else(|| CliError::Usage("managed Host Team is missing".to_string()))?;
    if member.agent_member_id != team.host_agent_id || member.is_external_interactive() {
        return Ok(None);
    }
    let execution_space_id = team_run_execution_space_id(&ledger.store, &run)?;
    let expected_native_session = member
        .native_session
        .as_ref()
        .map(agentfirm_native_session_ref);
    let sessions = ledger
        .store
        .fabric_agent_sessions(&execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
                && session.provider_kind == member.provider
                && agentfirm_native_session_identity_matches(
                    session.native_session_ref.as_ref(),
                    expected_native_session.as_ref(),
                )
                && session.control_state.runtime_residency == RuntimeResidency::Attached
                && session.control_state.driver_ref
                    == RuntimeDriverRef::TeamSupervisor {
                        team_run_id: ledger.run_id.clone(),
                        team_supervisor_id: ledger.supervisor_id.clone(),
                        team_supervisor_generation: ledger.supervisor_generation,
                    }
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Err(CliError::Usage(format!(
            "AGENT_SESSION_AMBIGUOUS: managed Host {} has {} current exact-session bindings",
            member.agent_member_id,
            sessions.len()
        )));
    };
    Ok(Some((execution_space_id, (*session).clone())))
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
