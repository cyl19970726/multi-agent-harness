use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_collaboration_proposal(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &crate::AgentFirmHttpCredential,
    idempotency_key: &str,
    request: &QueueCollaborationProposalRequest,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    if idempotency_key.trim().is_empty()
        || request.company_id.trim().is_empty()
        || request.source_work_id.trim().is_empty()
        || request.target_execution_space_id.trim().is_empty()
        || request.target_team_revision == 0
        || request.expires_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration proposal requires bounded identity, exact target revision, idempotency, and future expiry",
        ));
    }
    let work = store
        .latest_works()
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("source Work lookup failed: {error}"),
            )
        })?
        .into_iter()
        .find(|work| work.id == request.source_work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source Work does not exist",
            )
        })?;
    let team_id = work.accountable_team_id.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "source Work is not bound to a canonical AgentTeam",
        )
    })?;
    let teams = store.teams().map_err(|error| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            format!("source AgentTeam lookup failed: {error}"),
        )
    })?;
    let team_revision = teams.iter().filter(|team| team.id == team_id).count() as u64;
    let team = teams
        .iter()
        .rev()
        .find(|team| team.id == team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source AgentTeam does not exist",
            )
        })?;
    let actor_is_host = credential.actor.kind
        == harness_core::agentfirm_api::ActorKind::AgentMember
        && credential.actor.id == team.host_agent_id;
    let actor_is_owner = credential.actor.kind
        == harness_core::agentfirm_api::ActorKind::AgentMember
        && work.owner_member_id.as_deref() == Some(credential.actor.id.as_str());
    if !actor_is_host && !actor_is_owner {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "only the exact source Host or current Work owner may propose a Delegation",
        ));
    }
    if local_node_id.trim().is_empty()
        || team.node_id != local_node_id
        || request.target_node_id == local_node_id
    {
        return Err(FabricError::none(
            FabricErrorCode::TargetNotPlaced,
            "source Team must belong to this Node and target Team must be cross-node",
        ));
    }
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "source Node has no current authenticated Fabric gateway session",
        )
    })?;
    let latest_event = store
        .work_events()
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("source Work event lookup failed: {error}"),
            )
        })?
        .into_iter()
        .rev()
        .find(|event| event.work_id == work.id && event.resulting_version == work.version)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source Work has no exact current WorkEvent",
            )
        })?;
    let source_work_ref = harness_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: team.id.clone(),
        team_revision,
        placement_generation: 1,
        work_id: work.id.clone(),
        work_revision: work.version,
        work_event_id: latest_event.id,
        digest: harness_store::canonical_json_fingerprint(&serde_json::to_value(&work).map_err(
            |error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
        )?),
    };
    let source_host = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: team.host_agent_id.clone(),
    };
    let source_owner = work
        .owner_member_id
        .as_ref()
        .map(|id| harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: id.clone(),
        })
        .unwrap_or_else(|| source_host.clone());
    let work_service = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: format!("work-application:{execution_space_id}"),
    };
    let mut attestation = harness_core::collaboration::SourceWorkAttestation {
        id: source_work_attestation_id(
            &work.id,
            work.version,
            session.gateway_generation,
            idempotency_key,
        ),
        company_id: request.company_id.clone(),
        source_work_ref,
        source_owner_ref: source_owner,
        source_host_ref: source_host,
        work_application_service_ref: work_service.clone(),
        source_gateway_generation: session.gateway_generation,
        attestation_digest: String::new(),
        issued_at: format!("unix-ms:{now_unix_ms}"),
    };
    attestation.attestation_digest =
        harness_store::canonical_json_fingerprint(&serde_json::json!({
            "id": attestation.id,
            "company_id": attestation.company_id,
            "source_work_ref": attestation.source_work_ref,
            "source_owner_ref": attestation.source_owner_ref,
            "source_host_ref": attestation.source_host_ref,
            "work_application_service_ref": attestation.work_application_service_ref,
            "source_gateway_generation": attestation.source_gateway_generation,
            "issued_at": attestation.issued_at,
        }));
    store
        .put_source_work_attestation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: work_service.clone(),
                command_name: "source_work_attest".into(),
                idempotency_key: format!("attestation:{idempotency_key}"),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &attestation,
            &work_service,
            session.gateway_generation,
        )
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                format!("source Work attestation failed: {error}"),
            )
        })?;
    let target_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: request.target_team_id.clone(),
        team_revision: request.target_team_revision,
        node_id: request.target_node_id.clone(),
        placement_generation: 1,
    };
    let proposal = harness_store::ProposeDelegationRequest {
        delegation_id: format!("delegation:{idempotency_key}"),
        source_work_attestation_id: attestation.id,
        target_placement,
        requested_outcome: request.requested_outcome.clone(),
        outcome_class: request.outcome_class.clone(),
        acceptance_contract: request.acceptance_contract.clone(),
        operation_id: format!("collaboration-propose:{idempotency_key}"),
    };
    let business = store
        .delegation_propose_operation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: credential.actor.clone(),
                command_name: "delegation_propose".into(),
                idempotency_key: idempotency_key.into(),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &proposal,
            &request.policy_id,
        )
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("delegation proposal build failed: {error}"),
            )
        })?;
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: credential.actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.target_execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "delegation_id": proposal.delegation_id,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

pub(super) fn source_work_attestation_id(
    work_id: &str,
    work_revision: u64,
    gateway_generation: u64,
    idempotency_key: &str,
) -> String {
    format!(
        "source-work-attestation:{work_id}:{work_revision}:{gateway_generation}:{idempotency_key}"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_remote_fact_publication(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &crate::AgentFirmHttpCredential,
    idempotency_key: &str,
    expected_delegation_revision: u64,
    request: &QueueRemoteFactPublicationRequest,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    use harness_core::collaboration::{
        RemoteFactKind, RemoteFactPublication, RemoteFactSnapshot, RemoteWorkRef,
        TargetPlacementRef,
    };

    if idempotency_key.trim().is_empty()
        || expected_delegation_revision == 0
        || request.company_id.trim().is_empty()
        || request.delegation_id.trim().is_empty()
        || request.fact_id.trim().is_empty()
        || request.source_work_ref.execution_space_id.trim().is_empty()
        || request.source_work_ref.team_revision == 0
        || request.source_work_ref.placement_generation != 1
        || request.source_work_ref.node_id == local_node_id
        || request.expires_unix_ms <= now_unix_ms
        || request.retain_until.trim().is_empty()
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "remote fact publication requires exact Delegation/source placement, immutable fact identity, idempotency and future expiry",
        ));
    }

    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let target_work_result_id = format!("route-target-work-{}", request.delegation_id);
    let target_work_result = local
        .snapshot()?
        .results
        .get(&target_work_result_id)
        .cloned()
        .filter(|result| {
            result.result_schema == "agentfirm.collaboration.target_work_created.v1"
                && result.effect == harness_fabric::EffectCertainty::Applied
        })
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "remote fact requires the applied target Work receipt for this Delegation",
            )
        })?;
    let relationship_target_work_ref = serde_json::from_value::<RemoteWorkRef>(
        target_work_result
            .result
            .get("target_work_ref")
            .cloned()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "applied target Work receipt lacks target_work_ref",
                )
            })?,
    )
    .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;

    let (
        fact_work_id,
        fact_work_revision,
        fact_revision,
        result_report_id,
        created_by,
        summary,
        schema,
        fact,
        artifact_refs,
        evidence_refs,
    ) = match request.fact_kind {
        RemoteFactKind::Report => {
            let report = store
                .trust_work_reports(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|report| report.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native WorkReport does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "kind": report.kind,
                "summary": report.summary,
                "candidate_fingerprint": report.candidate_fingerprint,
                "artifact_refs": report.artifact_refs,
                "evidence_refs": report.evidence_refs,
                "known_risks": report.known_risks,
                "confidence": report.confidence,
                "recommended_next_action": report.recommended_next_action,
            });
            let artifact_refs = report.artifact_refs.clone();
            let evidence_refs = report.evidence_refs.clone();
            (
                report.work_id,
                report.work_revision,
                report.report_revision,
                (report.kind == harness_core::agentfirm_api::WorkReportKind::Result)
                    .then_some(report.id.clone()),
                report.authored_by,
                redacted["summary"].as_str().unwrap_or_default().to_string(),
                "agentfirm.remote-fact.work-report.v1",
                redacted,
                artifact_refs,
                evidence_refs,
            )
        }
        RemoteFactKind::Finding => {
            let finding = store
                .trust_work_findings(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|finding| finding.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native WorkFinding does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "kind": finding.kind,
                "summary": finding.summary,
                "affected_work_refs": finding.affected_work_refs,
                "reusable_asset_refs": finding.reusable_asset_refs,
                "invalidated_assumptions": finding.invalidated_assumptions,
                "evidence_refs": finding.evidence_refs,
                "confidence": finding.confidence,
            });
            (
                finding.work_id,
                finding.work_revision,
                1,
                None,
                finding.reported_by,
                redacted["summary"].as_str().unwrap_or_default().to_string(),
                "agentfirm.remote-fact.work-finding.v1",
                redacted,
                Vec::new(),
                finding.evidence_refs,
            )
        }
        RemoteFactKind::Failure => {
            let analysis = store
                .trust_failure_analyses(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|analysis| analysis.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native FailureAnalysis does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "observed_failure": analysis.observed_failure,
                "impact": analysis.impact,
                "primary_cause_status": analysis.primary_cause_status,
                "primary_cause": analysis.primary_cause,
                "retry_safety": analysis.retry_safety,
                "side_effect_summary": analysis.side_effect_summary,
                "recovery_options": analysis.recovery_options,
                "recommended_host_decision": analysis.recommended_host_decision,
                "evidence_refs": analysis.evidence_refs,
                "confidence": analysis.confidence,
            });
            (
                analysis.work_id,
                analysis.work_revision,
                1,
                None,
                analysis.reported_by,
                redacted["observed_failure"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                "agentfirm.remote-fact.failure-analysis.v1",
                redacted,
                Vec::new(),
                analysis.evidence_refs,
            )
        }
    };
    if created_by != credential.actor {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "credential is not the native fact author",
        ));
    }
    let current_work = store
        .latest_works()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|work| work.id == fact_work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "native fact target Work does not exist",
            )
        })?;
    let (work, work_event_id) = exact_work_projection_at_revision(
        store,
        execution_space_id,
        &fact_work_id,
        fact_work_revision,
    )?;
    let team_id = work.accountable_team_id.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "native target Work is not Team-bound",
        )
    })?;
    let teams = store
        .teams()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let team_revision = teams.iter().filter(|team| team.id == team_id).count() as u64;
    let team = teams
        .iter()
        .rev()
        .find(|team| team.id == team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "target AgentTeam does not exist",
            )
        })?;
    let exact_active_binding = store
        .fabric_work_execution_bindings(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|binding| {
            binding.work_id == current_work.id
                && binding.work_revision == current_work.version
                && binding.team_id == team.id
                && binding.agent_member_id == credential.actor.id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .count()
        == 1;
    if current_work.accountable_team_id.as_deref() != Some(team.id.as_str())
        || current_work.version < work.version
        || team.node_id != local_node_id
        || current_work.owner_member_id.as_deref() != Some(credential.actor.id.as_str())
        || !exact_active_binding
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "remote fact requires the exact current target Work owner and active WorkExecutionBinding",
        ));
    }
    let native_fact_work_ref = RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: team.id.clone(),
        team_revision,
        placement_generation: 1,
        work_id: work.id.clone(),
        work_revision: work.version,
        work_event_id,
        digest: harness_store::canonical_json_fingerprint(&serde_json::to_value(&work).map_err(
            |error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
        )?),
    };
    let operational_decision_ref = match result_report_id.as_deref() {
        Some(report_id) => accepted_work_decision_ref(
            store,
            execution_space_id,
            &work.id,
            report_id,
            &native_fact_work_ref,
            &team.host_agent_id,
        )?,
        None => None,
    };
    if current_work.version != work.version && operational_decision_ref.is_none() {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "superseded native fact requires the exact accepted result Work decision",
        ));
    }
    let publication_id = format!(
        "remote-fact:{}:{:?}:{}",
        request.delegation_id, request.fact_kind, request.fact_id
    )
    .to_ascii_lowercase();
    let fact_digest = harness_store::canonical_json_fingerprint(&fact);
    let publication = RemoteFactPublication {
        id: publication_id.clone(),
        company_id: request.company_id.clone(),
        delegation_id: request.delegation_id.clone(),
        origin_node_id: local_node_id.into(),
        origin_team_id: team.id.clone(),
        fact_work_ref: relationship_target_work_ref.clone(),
        native_fact_work_ref,
        delegation_source_work_ref: request.source_work_ref.clone(),
        fact_kind: request.fact_kind,
        fact_id: request.fact_id.clone(),
        fact_revision,
        fact_digest: fact_digest.clone(),
        summary,
        classification: "team".into(),
        snapshot: RemoteFactSnapshot {
            publication_id: publication_id.clone(),
            fact_schema: schema.into(),
            canonical_redacted_fact: fact,
            canonical_digest: fact_digest,
        },
        artifact_refs,
        evidence_refs,
        operational_decision_ref,
        created_by: credential.actor.clone(),
        created_at: format!("unix-ms:{now_unix_ms}"),
        retain_until: request.retain_until.clone(),
    };
    if publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
        || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
        || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
        || publication.native_fact_work_ref.placement_generation
            != publication.fact_work_ref.placement_generation
        || publication.fact_work_ref != relationship_target_work_ref
        || publication.origin_team_id != relationship_target_work_ref.team_id
        || publication.origin_node_id != relationship_target_work_ref.node_id
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "remote fact disagrees with the exact locally applied target Work receipt",
        ));
    }
    let source_placement = TargetPlacementRef {
        team_id: request.source_work_ref.team_id.clone(),
        team_revision: request.source_work_ref.team_revision,
        node_id: request.source_work_ref.node_id.clone(),
        placement_generation: request.source_work_ref.placement_generation,
    };
    let business = store
        .remote_fact_publish_operation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: credential.actor.clone(),
                command_name: "remote_fact_publish".into(),
                idempotency_key: idempotency_key.into(),
                expected_revision: expected_delegation_revision,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &publication,
            &source_placement,
            local_node_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "target Node has no current authenticated Fabric gateway session",
        )
    })?;
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: credential.actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.source_work_ref.execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    if let Some(existing) = local.snapshot()?.outboxes.get(&routed.id) {
        let existing_operation = existing.operation.as_ref().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::RecoveryRequired,
                "remote fact replay retained no operation body and requires operator recovery",
            )
        })?;
        let reference = match existing_operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "remote_fact_publish" =>
            {
                reference
            }
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "remote fact operation identity was reused by another business kind",
                ))
            }
        };
        let existing_publication = serde_json::from_value::<RemoteFactPublication>(
            reference
                .payload
                .get("publication")
                .cloned()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "remote fact replay lacks its frozen publication",
                    )
                })?,
        )
        .map_err(|error| {
            FabricError::none(FabricErrorCode::IdempotencyConflict, error.to_string())
        })?;
        let mut semantic_publication = publication.clone();
        semantic_publication.created_at = existing_publication.created_at.clone();
        if semantic_publication != existing_publication
            || existing_operation.idempotency_key != idempotency_key
            || existing_operation.expected_target_revision != Some(expected_delegation_revision)
            || existing_operation.expires_at_unix_ms != request.expires_unix_ms
            || existing_operation.target_execution_space_id.as_deref()
                != Some(request.source_work_ref.execution_space_id.as_str())
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "remote fact replay changed its actor, fact, Work/placement, revision, retention, or expiry",
            ));
        }
        return Ok(serde_json::json!({
            "delegation_id": request.delegation_id,
            "publication": existing_publication,
            "operation_id": existing_operation.id,
            "outbox_state": existing.local_state,
            "replayed": true,
        }));
    }
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "delegation_id": request.delegation_id,
        "publication": publication,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_collaboration_message(
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    idempotency_key: &str,
    message: &harness_core::agentfirm_api::Message,
    request: &QueueCollaborationMessageRequest,
    admission_authority: harness_core::collaboration::MessageAdmissionAuthority,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    let authority = match admission_authority {
        harness_core::collaboration::MessageAdmissionAuthority::WorkDelegation(authority) => {
            authority
        }
        harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(authority) => {
            return queue_peer_team_message(
                firm_home,
                execution_space_id,
                local_node_id,
                actor,
                idempotency_key,
                message,
                request,
                &authority,
                now_unix_ms,
            );
        }
    };
    let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "cross-node Message requires CollaborationScope",
        )
    })?;
    if message.sender_actor_ref != *actor
        || message.source_execution_space_id != execution_space_id
        || message.source_node_id != local_node_id
        || message.idempotency_key != idempotency_key
        || request.target_node_id == local_node_id
        || request.target_team_revision == 0
        || request.expected_delegation_revision == 0
        || request.expires_unix_ms <= now_unix_ms
        || scope.target_team_id != request.target_team_id
        || scope.expected_delegation_revision != Some(request.expected_delegation_revision)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Message route disagrees with server-authored Message, exact sender, Delegation revision, or target",
        ));
    }
    let target_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: request.target_team_id.clone(),
        team_revision: request.target_team_revision,
        node_id: request.target_node_id.clone(),
        placement_generation: 1,
    };
    let canonical_message_envelope = serde_json::to_value(message)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let reference = harness_fabric::MessageReference {
        message_id: message.id.clone(),
        body_digest: message.body_digest.clone(),
        canonical_message_envelope: Some(canonical_message_envelope),
        message_object_ref: None,
    };
    let payload = serde_json::json!({
        "message_reference": reference,
        "delegation_authority": authority,
    });
    let business = harness_core::collaboration::RoutedBusinessOperation {
        id: format!("collaboration-message:{}", message.id),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: request.company_id.clone(),
        kind: harness_core::collaboration::RoutedBusinessKind::TeamMessageDeliver,
        authenticated_actor: actor.clone(),
        source_node_id: local_node_id.into(),
        target_placement,
        expected_revision: request.expected_delegation_revision,
        idempotency_key: format!("route:{idempotency_key}"),
        payload_digest: harness_store::canonical_json_fingerprint(&payload),
        payload,
        required_capability: harness_core::collaboration::RoutedBusinessKind::TeamMessageDeliver
            .required_capability(),
        ordering_key: format!(
            "delegation:{}",
            scope.delegation_id.as_deref().unwrap_or("host-to-host")
        ),
        created_at: format!("unix-ms:{now_unix_ms}"),
    };
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "source Node has no current authenticated Fabric gateway session",
        )
    })?;
    if message.source_node_daemon_id != session.node_daemon_id
        || message.source_authority_generation != session.node_daemon_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "Message was not authored by the exact current NodeDaemon generation",
        ));
    }
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.target_execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "message_id": message.id,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

/// Queue one ordinary peer-Team Message for cross-node delivery. Peer
/// admission is the non-Delegation path: the frozen
/// `PeerTeamMessageAdmissionAuthority` proves that one exact active source
/// TeamMembership and one exact local AgentSession/NodeDaemon generation
/// authored the Message, and the route delivers exactly one Team-addressed
/// (or direct TeamMembership-bound) CanonicalMessageDelivery under the durable
/// target subscription. No WorkDelegation is required or consulted.
#[allow(clippy::too_many_arguments)]
pub(super) fn queue_peer_team_message(
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    idempotency_key: &str,
    message: &harness_core::agentfirm_api::Message,
    request: &QueueCollaborationMessageRequest,
    authority: &harness_core::collaboration::PeerTeamMessageAdmissionAuthority,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "cross-node Message requires CollaborationScope",
        )
    })?;
    let member_target = authority.target_membership_id.is_some()
        || authority.target_membership_generation.is_some()
        || authority.target_agent_member_id.is_some();
    let exact_recipient = if member_target {
        authority.target_membership_id.is_some()
            && authority.target_membership_generation.is_some()
            && message.recipients.len() == 1
            && message.recipients[0].kind
                == harness_core::agentfirm_api::MessageRecipientKind::AgentMember
            && Some(message.recipients[0].id.as_str())
                == authority.target_agent_member_id.as_deref()
            && message.target_ref == message.recipients[0]
    } else {
        message.recipients.len() == 1
            && message.recipients[0].kind == harness_core::agentfirm_api::MessageRecipientKind::Team
            && message.recipients[0].id == authority.target_team_id
            && message.target_ref == message.recipients[0]
    };
    if message.sender_actor_ref != *actor
        || message.sender_actor_ref.kind != harness_core::agentfirm_api::ActorKind::AgentMember
        || message.source_execution_space_id != execution_space_id
        || message.source_node_id != local_node_id
        || message.idempotency_key != idempotency_key
        || request.target_node_id == local_node_id
        || request.expected_delegation_revision != 0
        || request.expires_unix_ms <= now_unix_ms
        || !exact_recipient
        || message.sender_agent_member_id.as_deref()
            != Some(authority.source_agent_member_id.as_str())
        || message.sender_session_id.as_deref() != Some(authority.source_session_id.as_str())
        || message.team_id.as_deref() != Some(authority.source_team_id.as_str())
        || scope.source_team_id != authority.source_team_id
        || scope.target_team_id != request.target_team_id
        || scope.delegation_id.is_some()
        || scope.expected_delegation_revision.is_some()
        || scope.source_work_ref.is_some()
        || scope.target_work_ref.is_some()
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "ordinary peer-Team Message route disagrees with the exact sender, non-Delegation scope, or single Team recipient",
        ));
    }
    if authority.company_id != request.company_id
        || authority.source_execution_space_id != execution_space_id
        || authority.source_node_id != local_node_id
        || authority.source_agent_member_id != actor.id
        || authority.source_team_revision == 0
        || authority.source_membership_generation == 0
        || authority.source_session_generation == 0
        || authority.source_node_daemon_generation == 0
        || authority.target_team_revision == 0
        || authority.target_subscription_revision == 0
        || authority.target_execution_space_id != request.target_execution_space_id
        || authority.target_team_id != request.target_team_id
        || authority.target_team_revision != request.target_team_revision
        || authority.target_node_id != request.target_node_id
        || authority.source_required_capability != "message.peer_team.author"
        || authority.target_required_capability != "collaboration.peer_message_deliver"
        || authority.source_policy_digest
            != harness_store::peer_team_source_policy_digest(authority)
        || authority.target_policy_digest
            != harness_store::peer_team_target_policy_digest(authority)
        || authority.authority_digest
            != harness_store::peer_team_message_authority_digest(authority)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "peer-Team Message admission authority is stale, widened, or cross-wired",
        ));
    }
    let target_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: request.target_team_id.clone(),
        team_revision: request.target_team_revision,
        node_id: request.target_node_id.clone(),
        placement_generation: 1,
    };
    let canonical_message_envelope = serde_json::to_value(message)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let reference = harness_fabric::MessageReference {
        message_id: message.id.clone(),
        body_digest: message.body_digest.clone(),
        canonical_message_envelope: Some(canonical_message_envelope),
        message_object_ref: None,
    };
    let payload = serde_json::json!({
        "message_reference": reference,
        "message_admission_authority":
            harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(authority.clone()),
    });
    let business = harness_core::collaboration::RoutedBusinessOperation {
        id: format!("collaboration-message:{}", message.id),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: request.company_id.clone(),
        kind: harness_core::collaboration::RoutedBusinessKind::PeerMessageDeliver,
        authenticated_actor: actor.clone(),
        source_node_id: local_node_id.into(),
        target_placement,
        expected_revision: authority.target_subscription_revision,
        idempotency_key: format!("route:{idempotency_key}"),
        payload_digest: harness_store::canonical_json_fingerprint(&payload),
        payload,
        required_capability: harness_core::collaboration::RoutedBusinessKind::PeerMessageDeliver
            .required_capability(),
        ordering_key: format!("team:{}", request.target_team_id),
        created_at: format!("unix-ms:{now_unix_ms}"),
    };
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "source Node has no current authenticated Fabric gateway session",
        )
    })?;
    if message.source_node_daemon_id != session.node_daemon_id
        || message.source_authority_generation != session.node_daemon_generation
        || authority.source_node_daemon_id != session.node_daemon_id
        || authority.source_node_daemon_generation != session.node_daemon_generation
        || session.gateway_generation == 0
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "peer-Team Message or its admission authority is not bound to the exact current NodeDaemon generation",
        ));
    }
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    // The generic store route adapter cannot express the peer session facts
    // (source AgentSession runtime generation and business session id), so the
    // peer route freezes the closed RoutedOperation directly. The target
    // independently revalidates every field before any Message mutation.
    let body = serde_json::to_value(harness_fabric::CollaborationBusinessReference {
        business_kind: business.kind.wire_name().into(),
        required_capability: business.required_capability.clone(),
        business_actor_kind: "agent_member".into(),
        business_actor_id: authority.source_agent_member_id.clone(),
        target_team_id: business.target_placement.team_id.clone(),
        target_team_revision: business.target_placement.team_revision,
        placement_generation: business.target_placement.placement_generation,
        expected_revision: business.expected_revision,
        payload_digest: business.payload_digest.clone(),
        payload: business.payload.clone(),
    })
    .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let routed = RoutedOperation {
        id: business.id.clone(),
        company_id: business.company_id.clone(),
        kind: COLLABORATION_BUSINESS_OPERATION_KIND.into(),
        source_authority: harness_fabric::OperationSourceAuthority::Node,
        source_node_id: Some(local_node_id.into()),
        target_node_id: request.target_node_id.clone(),
        source_gateway_generation: Some(session.gateway_generation),
        source_node_daemon_id: Some(session.node_daemon_id.clone()),
        source_node_daemon_generation: Some(session.node_daemon_generation),
        control_plane_generation: session.control_plane_generation,
        source_execution_space_id: Some(execution_space_id.into()),
        target_execution_space_id: Some(request.target_execution_space_id.clone()),
        actor: node_actor.clone(),
        actor_runtime_generation: Some(authority.source_session_generation),
        authorization_context: std::collections::BTreeMap::from([
            (
                "target_team_id".into(),
                business.target_placement.team_id.clone(),
            ),
            (
                "target_team_revision".into(),
                business.target_placement.team_revision.to_string(),
            ),
            (
                "placement_generation".into(),
                business.target_placement.placement_generation.to_string(),
            ),
            (
                "required_capability".into(),
                business.required_capability.clone(),
            ),
            ("business_actor_kind".into(), "agent_member".into()),
            (
                "business_actor_id".into(),
                authority.source_agent_member_id.clone(),
            ),
            (
                "business_actor_session_id".into(),
                authority.source_session_id.clone(),
            ),
        ]),
        idempotency_key: business.idempotency_key.clone(),
        ordering_key: business.ordering_key.clone(),
        correlation_id: business.id.clone(),
        causation_id: None,
        expected_target_revision: Some(business.expected_revision),
        body_schema: harness_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
        body_digest: harness_fabric::json_digest(&body)?,
        body,
        priority: harness_fabric::OperationPriority::Normal,
        created_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: harness_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
    };
    routed.closed_body()?;
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "message_id": message.id,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}
