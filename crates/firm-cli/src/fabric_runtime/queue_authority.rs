use super::*;


pub(crate) fn fabric_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError::Usage(
            "fabric requires control-plane|node-gateway".into(),
        ));
    };
    match command {
        "control-plane" => control_plane_command(resolved, &args[1..]),
        "node-gateway" => node_gateway_command(store, resolved, &args[1..]),
        "route" => route_command(store, resolved, &args[1..]),
        other => Err(CliError::Usage(format!("unknown fabric command: {other}"))),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueCollaborationProposalRequest {
    pub company_id: String,
    pub source_work_id: String,
    pub target_team_id: String,
    pub target_team_revision: u64,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    pub policy_id: String,
    pub requested_outcome: String,
    pub outcome_class: String,
    pub acceptance_contract: String,
    pub expires_unix_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueCollaborationMessageRequest {
    pub company_id: String,
    pub target_team_id: String,
    pub target_team_revision: u64,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    /// Caller-declared current revision of the durable target subscription.
    /// The server overrides it from the target Store whenever the target
    /// Execution Space is registered on this Node; otherwise it is required
    /// and the target revalidates it fail-closed before any delivery mutation.
    #[serde(default)]
    pub target_subscription_revision: Option<u64>,
    pub expected_delegation_revision: u64,
    pub expires_unix_ms: u64,
}

pub(crate) fn resolve_collaboration_message_authority(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &crate::AgentFirmHttpCredential,
    draft: &harness_core::agentfirm_api::MessageDraft,
    request: &QueueCollaborationMessageRequest,
) -> Result<harness_core::collaboration::CollaborationMessageAuthority, FabricError> {
    use harness_core::agentfirm_api::WorkExecutionBindingStatus;
    use harness_core::collaboration::{CollaborationMessageAuthority, DelegationState};

    let scope = draft.collaboration_scope.as_ref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires server-resolvable CollaborationScope",
        )
    })?;
    let delegation_id = scope.delegation_id.as_deref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires an exact central Delegation id",
        )
    })?;
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let collaboration = layout.open_collaboration_store(&request.company_id)?;
    let delegation = collaboration
        .collaboration_delegation(&request.company_id, delegation_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "cross-node Message references no central Delegation",
            )
        })?;
    let target_work_ref = delegation.target_work_ref.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires an accepted Delegation with target Work",
        )
    })?;
    let canonical_policy = collaboration
        .collaboration_inbound_policy(
            &request.company_id,
            &delegation.inbound_policy_snapshot.policy_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation inbound policy is not centrally available",
            )
        })?;
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": canonical_policy.id,
        "policy_revision": canonical_policy.revision,
        "mode": canonical_policy.mode,
        "allowed_outcome_classes": canonical_policy.allowed_outcome_classes,
        "max_active_delegations": canonical_policy.max_active_delegations,
    }));
    let source_work = store
        .latest_works()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|work| work.id == delegation.source_work_ref.work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation source Work is not current in this Execution Space",
            )
        })?;
    let source_team_id = source_work.accountable_team_id.as_deref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Delegation source Work is not Team-bound",
        )
    })?;
    let source_teams = store
        .teams()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let source_team = source_teams
        .iter()
        .rev()
        .find(|team| team.id == source_team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation source Team is not current",
            )
        })?;
    let source_team_revision = source_teams
        .iter()
        .filter(|team| team.id == source_team_id)
        .count() as u64;
    let (source_work_projection, source_work_event_id) = exact_work_projection_at_revision(
        store,
        execution_space_id,
        &source_work.id,
        source_work.version,
    )?;
    let current_source_ref = harness_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: source_team.id.clone(),
        team_revision: source_team_revision,
        placement_generation: 1,
        work_id: source_work.id.clone(),
        work_revision: source_work.version,
        work_event_id: source_work_event_id,
        digest: harness_store::canonical_json_fingerprint(
            &serde_json::to_value(&source_work_projection).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?,
        ),
    };
    let source_sessions = store
        .fabric_agent_sessions(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let active_binding = store
        .fabric_work_execution_bindings(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|binding| {
            binding.work_id == source_work.id
                && binding.work_revision == source_work.version
                && binding.team_id == source_team.id
                && binding.agent_member_id == delegation.source_owner_ref.id
                && binding.status == WorkExecutionBindingStatus::Active
                && source_sessions.iter().any(|session| {
                    session.id == binding.agent_session_id
                        && session.agent_member_id == binding.agent_member_id
                        && session.runtime_generation == binding.agent_session_generation
                        && session.node_id == local_node_id
                        && session.lifecycle
                            != harness_core::agentfirm_api::AgentSessionStatus::Closed
                })
        })
        .collect::<Vec<_>>();
    let source_attestation = collaboration
        .collaboration_source_work_attestation(
            &request.company_id,
            &delegation.source_work_attestation_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "source Work attestation is missing",
            )
        })?;
    let actor_authorized = credential.actor == source_attestation.source_host_ref
        || credential.actor == delegation.source_owner_ref;
    if delegation.company_id != request.company_id
        || delegation.state != DelegationState::Active
        || delegation.revision != request.expected_delegation_revision
        || delegation.source_team_id != source_team.id
        || delegation.source_node_id != local_node_id
        || delegation.source_work_ref != current_source_ref
        || source_team.node_id != local_node_id
        || request.target_team_id != delegation.target_placement.team_id
        || request.target_team_revision != delegation.target_placement.team_revision
        || request.target_node_id != delegation.target_placement.node_id
        || request.target_execution_space_id != target_work_ref.execution_space_id
        || canonical_policy.revoked_at.is_some()
        || canonical_policy.target_team_id != delegation.target_placement.team_id
        || canonical_policy.source_team_id != delegation.source_team_id
        || canonical_policy.created_by_target_host != delegation.target_host_ref
        || policy_digest != delegation.inbound_policy_snapshot.policy_digest
        || scope.source_team_id != delegation.source_team_id
        || scope.target_team_id != delegation.target_placement.team_id
        || scope.expected_delegation_revision != Some(delegation.revision)
        || scope.source_work_ref.as_ref() != Some(&current_source_ref)
        || scope.target_work_ref.as_ref() != Some(&target_work_ref)
        || draft.team_id.as_deref() != Some(delegation.source_team_id.as_str())
        || draft.work_id.as_deref() != Some(source_work.id.as_str())
        || !actor_authorized
        || active_binding.len() != 1
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message is outside the exact accepted Delegation, current Work binding, policy, or placement authority",
        ));
    }
    let mut authority = CollaborationMessageAuthority {
        company_id: request.company_id.clone(),
        delegation_id: delegation.id,
        delegation_revision: delegation.revision,
        source_work_ref: current_source_ref,
        target_work_ref,
        target_placement: delegation.target_placement,
        source_owner_ref: delegation.source_owner_ref,
        source_host_ref: source_attestation.source_host_ref,
        target_host_ref: delegation.target_host_ref,
        inbound_policy_snapshot: delegation.inbound_policy_snapshot,
        authority_digest: String::new(),
    };
    authority.authority_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "delegation_id": authority.delegation_id,
        "delegation_revision": authority.delegation_revision,
        "source_work_ref": authority.source_work_ref,
        "target_work_ref": authority.target_work_ref,
        "target_placement": authority.target_placement,
        "source_owner_ref": authority.source_owner_ref,
        "source_host_ref": authority.source_host_ref,
        "target_host_ref": authority.target_host_ref,
        "inbound_policy_snapshot": authority.inbound_policy_snapshot,
    }));
    Ok(authority)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueRemoteFactPublicationRequest {
    pub company_id: String,
    pub delegation_id: String,
    pub fact_kind: harness_core::collaboration::RemoteFactKind,
    pub fact_id: String,
    pub source_work_ref: harness_core::collaboration::RemoteWorkRef,
    pub expires_unix_ms: u64,
    pub retain_until: String,
}

pub(super) fn exact_work_projection_at_revision(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
    work_revision: u64,
) -> Result<(harness_core::Work, String), FabricError> {
    let projection_matches = |value: &serde_json::Value| {
        serde_json::from_value::<harness_core::Work>(value.clone())
            .ok()
            .filter(|work| work.id == work_id && work.version == work_revision)
    };
    for operation in store
        .canonical_operations_for_space(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
    {
        if let Some(work) = projection_matches(&operation.resulting_projection) {
            return Ok((work, operation.event.id));
        }
        for record in operation.immutable_side_records.into_iter().rev() {
            if let Some(work) = projection_matches(&record) {
                return Ok((work, operation.event.id));
            }
        }
    }
    for operation in store
        .work_operations()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
    {
        if operation.work.id == work_id && operation.work.version == work_revision {
            return Ok((operation.work, operation.event.id));
        }
    }
    Err(FabricError::none(
        FabricErrorCode::ExpectedRevisionConflict,
        "native fact does not bind an exact durable target Work revision",
    ))
}

pub(super) fn accepted_work_decision_ref(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
    report_id: &str,
    work_ref: &harness_core::collaboration::RemoteWorkRef,
    target_host_id: &str,
) -> Result<Option<harness_core::collaboration::WorkOperationalDecisionRef>, FabricError> {
    let operation = store
        .canonical_operations_for_space(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
        .find(|operation| {
            operation.event.aggregate_kind == "work"
                && operation.event.aggregate_id == work_id
                && operation.event.transition == "accepted"
                && operation.event.performed_by_actor.kind
                    == harness_core::agentfirm_api::ActorKind::AgentMember
                && operation.event.performed_by_actor.id == target_host_id
                && operation
                    .event
                    .payload
                    .get("work_report_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(report_id)
        });
    let Some(operation) = operation else {
        return Ok(None);
    };
    let accepted_work =
        serde_json::from_value::<harness_core::Work>(operation.resulting_projection.clone())
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
    if accepted_work.version != work_ref.work_revision + 1
        || accepted_work.phase != harness_core::WorkPhase::Closed
        || accepted_work.resolution != Some(harness_core::WorkResolution::Accepted)
    {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "accepted target Work decision does not bind the published submitted revision",
        ));
    }
    Ok(Some(
        harness_core::collaboration::WorkOperationalDecisionRef {
            decision_id: operation.event.id.clone(),
            work_ref: work_ref.clone(),
            decision_revision: operation.event.resulting_version,
            digest: harness_store::canonical_json_fingerprint(
                &serde_json::to_value(&operation.event).map_err(|error| {
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
                })?,
            ),
        },
    ))
}
