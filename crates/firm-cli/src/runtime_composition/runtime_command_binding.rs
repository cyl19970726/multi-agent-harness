use super::*;

pub(crate) fn runtime_command_binding_for_session(
    session: &harness_core::agentfirm_api::AgentSession,
) -> harness_core::agentfirm_api::RuntimeCommandBinding {
    harness_core::agentfirm_api::RuntimeCommandBinding {
        target_session_id: Some(session.id.clone()),
        target_runtime_generation: Some(session.runtime_generation),
        target_driver_generation: Some(session.control_state.driver_generation),
        target_driver: session.control_state.driver_ref.clone(),
        native_session_ref: session.native_session_ref.clone(),
        composition_fingerprint: session.control_state.composition_fingerprint.clone(),
        capability_fingerprint: session.control_state.capability_fingerprint.clone(),
        permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
        ..Default::default()
    }
}

pub(crate) fn runtime_command_binding_for_member_session(
    member_run: &harness_core::agentfirm_api::MemberRun,
    session: &harness_core::agentfirm_api::AgentSession,
) -> harness_core::agentfirm_api::RuntimeCommandBinding {
    harness_core::agentfirm_api::RuntimeCommandBinding {
        target_member_run_id: Some(member_run.id.clone()),
        target_member_run_generation: Some(member_run.runtime_generation),
        ..runtime_command_binding_for_session(session)
    }
}

pub(crate) fn runtime_command_binding_for_current_session(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    session: &harness_core::agentfirm_api::AgentSession,
) -> CliResult<harness_core::agentfirm_api::RuntimeCommandBinding> {
    let harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor { team_run_id, .. } =
        &session.control_state.driver_ref
    else {
        return Ok(runtime_command_binding_for_session(session));
    };
    let members = store
        .trust_member_runs(execution_space_id)?
        .into_iter()
        .filter(|member| {
            member.team_run_id == *team_run_id
                && member.agent_member_id == session.agent_member_id
                && member.coordination_status
                    == harness_core::agentfirm_api::MemberCoordinationStatus::Active
        })
        .collect::<Vec<_>>();
    let [member] = members.as_slice() else {
        return Err(CliError::Usage(
            "MEMBER_RUN_GENERATION_FENCED: TeamSupervisor runtime command requires exactly one active MemberRun for the target AgentSession".into(),
        ));
    };
    Ok(runtime_command_binding_for_member_session(member, session))
}

pub(crate) fn runtime_binding_fence_for_admission(
    ledger: &TeamRunLedger,
    admission: &harness_store::CanonicalMutationResult<
        harness_core::agentfirm_api::RuntimeCommandRecord,
    >,
    session: &harness_core::agentfirm_api::AgentSession,
    member_run: &harness_core::agentfirm_api::MemberRun,
    node_daemon: &harness_core::NodeDaemonLease,
) -> CliResult<crate::runtime_adapter_contract::RuntimeBindingFence> {
    let team_supervisor = ledger.store.latest_team_supervisor_lease(&ledger.run_id)?;
    crate::runtime_adapter_contract::RuntimeBindingFence::from_admitted_command(
        &admission.projection,
        session,
        member_run,
        node_daemon,
        team_supervisor.as_ref(),
        current_unix_ms_u64(),
    )
    .map_err(|error| CliError::Usage(format!("RUNTIME_BINDING_FENCE_REJECTED: {error}")))
}
