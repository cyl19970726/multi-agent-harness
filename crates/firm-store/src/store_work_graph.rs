use super::*;

use firm_core::agentfirm_api::WorkExecutionBindingStatus;
use firm_core::{
    derive_work_successor_ids, prepare_dependency_change, work_readiness, WorkReadiness,
};

/// One authoritative Work node plus values derived from the current DAG.
/// Successors and readiness are never persisted as a second graph authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreWorkGraphNode {
    pub work: Work,
    pub successor_work_ids: Vec<String>,
    pub readiness: WorkReadiness,
}

/// Current Work graph for one durable accountable AgentTeam. A graph may span
/// several TeamRuns; TeamRun is an execution attempt, not Work scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreWorkGraph {
    pub accountable_team_id: String,
    pub nodes: Vec<StoreWorkGraphNode>,
}

impl HarnessStore {
    pub(super) fn terminal_work_member_run_provenance_unlocked(
        &self,
        work: &Work,
    ) -> StoreResult<String> {
        let submitted_revision = work.version.saturating_sub(1);
        let submitted_attentions = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                attention.work_id == work.id
                    && attention.work_version == submitted_revision
                    && attention.kind == HostAttentionKind::WorkReviewRequested
            })
            .collect::<Vec<_>>();
        if !submitted_attentions.is_empty() {
            let [attention] = submitted_attentions.as_slice() else {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_RUN_GENERATION_FENCED: terminal member Work {} has ambiguous submitted execution provenance",
                    work.id
                )));
            };
            return attention.member_run_id.clone().ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_GENERATION_FENCED: terminal member Work {} has submitted execution provenance without an exact MemberRun",
                    work.id
                ))
            });
        }

        let run = self.require_team_run_unlocked(&work.team_run_id)?;
        let execution_space_id = self.current_team_run_execution_space_unlocked(&run)?;
        let bindings = self
            .fabric_work_execution_bindings(&execution_space_id)?
            .into_iter()
            .filter(|binding| {
                binding.work_id == work.id && binding.status == WorkExecutionBindingStatus::Active
            })
            .collect::<Vec<_>>();
        let [binding] = bindings.as_slice() else {
            return Err(StoreError::Conflict(format!(
                "WORK_EXECUTION_BINDING_ACTIVE: terminal member Work {} requires exactly one active execution binding",
                work.id
            )));
        };
        if binding.work_revision > work.version
            || self.work_responsibility_changed_after_revision_unlocked(
                &work.id,
                binding.work_revision,
            )?
            || work.assignee_membership_id.as_deref() != Some(binding.team_membership_id.as_str())
            || work.owner_member_id.as_deref() != Some(binding.agent_member_id.as_str())
        {
            return Err(StoreError::Conflict(format!(
                "WORK_EXECUTION_BINDING_RESPONSIBILITY_CONFLICT: terminal member Work {} does not match its exact admitted responsibility",
                work.id
            )));
        }
        let admission = self.work_execution_runtime_binding(&execution_space_id, &binding.id)?;
        let Some(member_run_id) = admission.target_member_run_id else {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_GENERATION_FENCED: terminal member Work {} has no exact admitted MemberRun provenance",
                work.id
            )));
        };
        if admission.target_member_run_generation.is_none()
            || admission.target_session_id.as_deref() != Some(binding.agent_session_id.as_str())
            || admission.target_runtime_generation != Some(binding.agent_session_generation)
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_GENERATION_FENCED: terminal member Work {} has incomplete exact runtime admission evidence",
                work.id
            )));
        }
        Ok(member_run_id)
    }

    pub(crate) fn canonical_work_command_context_unlocked(
        &self,
        work: &Work,
        expected_version: u64,
        command_name: &str,
        context: &WorkCommandContext,
        request_payload: &serde_json::Value,
    ) -> StoreResult<(firm_core::agentfirm_api::MutationContext, String)> {
        let run = self.require_team_run_unlocked(&work.team_run_id)?;
        let execution_space_id = self.current_team_run_execution_space_unlocked(&run)?;
        let request_fingerprint = canonical_json_fingerprint(request_payload);
        Ok((
            firm_core::agentfirm_api::MutationContext {
                execution_space_id,
                authenticated_actor: canonical_actor(&context.performed_by_actor),
                authority_actor: context.authority_actor.as_ref().map(canonical_actor),
                command_name: command_name.into(),
                idempotency_key: context.idempotency_key.clone(),
                expected_version,
                request_fingerprint: Some(request_fingerprint.clone()),
            },
            request_fingerprint,
        ))
    }

    pub(crate) fn canonical_terminal_work_outbox_unlocked(
        &self,
        work: &Work,
        event: &firm_core::agentfirm_api::CanonicalMutationEvent,
    ) -> StoreResult<Vec<HostAttention>> {
        let successor_kind = match work.resolution {
            Some(WorkResolution::Accepted) => HostAttentionKind::WorkPrerequisiteCompleted,
            Some(WorkResolution::Failed | WorkResolution::Cancelled) => {
                HostAttentionKind::WorkPrerequisiteNeedsReconciliation
            }
            None => return Ok(Vec::new()),
        };
        let Some(team_id) = work.accountable_team_id.as_deref() else {
            return Ok(Vec::new());
        };
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|candidate| candidate.accountable_team_id.as_deref() == Some(team_id))
            .collect::<Vec<_>>();
        let by_id = works
            .iter()
            .map(|candidate| (candidate.id.as_str(), candidate))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut attentions = Vec::new();
        let authored_by_exact_host = self
            .require_exact_team_run_host_actor(
                &TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: event.performed_by_actor.id.clone(),
                    display_name: None,
                    authn_source: Some("canonical_work_mutation".into()),
                },
                &work.team_run_id,
            )
            .is_ok();
        let primary_kind = if authored_by_exact_host {
            None
        } else {
            match work.resolution {
                Some(WorkResolution::Accepted) => Some(HostAttentionKind::WorkAccepted),
                Some(WorkResolution::Cancelled) => Some(HostAttentionKind::WorkCancelled),
                _ => None,
            }
        };
        if let Some(kind) = primary_kind {
            let member_run_id = self.terminal_work_member_run_provenance_unlocked(work)?;
            attentions.push(HostAttention {
                id: format!("host-attention-{}", event.id),
                team_run_id: work.team_run_id.clone(),
                kind,
                work_id: work.id.clone(),
                work_version: work.version,
                source_event_ref: event.id.clone(),
                member_run_id: Some(member_run_id),
                status: HostAttentionStatus::Actionable,
                attempt: 0,
                claim_id: None,
                claimed_host_surface: None,
                claimed_host_thread_id: None,
                claimed_host_lease_id: None,
                claimed_host_lease_generation: None,
                claimed_host_lease_owner_id: None,
                claimed_recipient_member_run_id: None,
                claimed_recipient_session_id: None,
                claimed_recipient_session_generation: None,
                claimed_node_daemon_id: None,
                claimed_node_daemon_generation: None,
                provider_receipt_id: None,
                last_failure_reason: None,
                created_at: event.created_at.clone(),
                updated_at: event.created_at.clone(),
            });
        }
        attentions.extend(
            derive_work_successor_ids(&work.id, &works)
                .into_iter()
                .filter_map(|id| by_id.get(id.as_str()))
                .filter(|dependent| !dependent.is_terminal())
                .map(|dependent| {
                    Ok(HostAttention {
                        id: format!("host-attention-work-graph:{}:{}", event.id, dependent.id),
                        team_run_id: dependent.team_run_id.clone(),
                        kind: successor_kind,
                        work_id: dependent.id.clone(),
                        work_version: dependent.version,
                        source_event_ref: event.id.clone(),
                        member_run_id: None,
                        status: HostAttentionStatus::Actionable,
                        attempt: 0,
                        claim_id: None,
                        claimed_host_surface: None,
                        claimed_host_thread_id: None,
                        claimed_host_lease_id: None,
                        claimed_host_lease_generation: None,
                        claimed_host_lease_owner_id: None,
                        claimed_recipient_member_run_id: None,
                        claimed_recipient_session_id: None,
                        claimed_recipient_session_generation: None,
                        claimed_node_daemon_id: None,
                        claimed_node_daemon_generation: None,
                        provider_receipt_id: None,
                        last_failure_reason: None,
                        created_at: event.created_at.clone(),
                        updated_at: event.created_at.clone(),
                    })
                })
                .collect::<StoreResult<Vec<_>>>()?,
        );
        Ok(attentions)
    }

    pub(super) fn work_graph_outbox_payload_unlocked(
        &self,
        work: &Work,
        kind: WorkEventKind,
        payload: serde_json::Value,
    ) -> StoreResult<serde_json::Value> {
        let outcome = match kind {
            WorkEventKind::Accepted => "accepted",
            WorkEventKind::Failed => "failed",
            WorkEventKind::Cancelled => "cancelled",
            _ => return Ok(payload),
        };
        let team_id = work.accountable_team_id.as_deref().ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_NOT_TEAM_SCOPED: Work {} cannot produce graph reconciliation",
                work.id
            ))
        })?;
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|candidate| candidate.accountable_team_id.as_deref() == Some(team_id))
            .collect::<Vec<_>>();
        let successors = derive_work_successor_ids(&work.id, &works);
        let by_id = works
            .iter()
            .map(|candidate| (candidate.id.as_str(), candidate))
            .collect::<std::collections::BTreeMap<_, _>>();
        let records = successors
            .into_iter()
            .filter_map(|id| by_id.get(id.as_str()))
            .filter(|dependent| !dependent.is_terminal())
            .map(|dependent| {
                serde_json::json!({
                    "dependent_work_id": dependent.id,
                    "dependent_work_version": dependent.version,
                    "dependent_team_run_id": dependent.team_run_id,
                    "outcome": outcome,
                })
            })
            .collect::<Vec<_>>();
        let mut object = match payload {
            serde_json::Value::Null => serde_json::Map::new(),
            serde_json::Value::Object(object) => object,
            other => {
                let mut object = serde_json::Map::new();
                object.insert("command_payload".to_string(), other);
                object
            }
        };
        object.insert(
            "work_graph_outbox".to_string(),
            serde_json::Value::Array(records),
        );
        Ok(serde_json::Value::Object(object))
    }

    pub(super) fn downstream_host_attentions_for_work_operation(
        operation: &WorkOperation,
    ) -> StoreResult<Vec<HostAttention>> {
        let Some(records) = operation
            .event
            .payload
            .get("work_graph_outbox")
            .and_then(serde_json::Value::as_array)
        else {
            return Ok(Vec::new());
        };
        records
            .iter()
            .map(|record| {
                let work_id = record
                    .get("dependent_work_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        StoreError::Conflict("invalid Work graph outbox work id".into())
                    })?;
                let work_version = record
                    .get("dependent_work_version")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        StoreError::Conflict("invalid Work graph outbox version".into())
                    })?;
                let team_run_id = record
                    .get("dependent_team_run_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        StoreError::Conflict("invalid Work graph outbox TeamRun".into())
                    })?;
                let outcome = record
                    .get("outcome")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        StoreError::Conflict("invalid Work graph outbox outcome".into())
                    })?;
                let kind = match outcome {
                    "accepted" => HostAttentionKind::WorkPrerequisiteCompleted,
                    "failed" | "cancelled" => {
                        HostAttentionKind::WorkPrerequisiteNeedsReconciliation
                    }
                    _ => {
                        return Err(StoreError::Conflict(format!(
                            "invalid Work graph outbox outcome {outcome}"
                        )))
                    }
                };
                Ok(HostAttention {
                    id: format!(
                        "host-attention-work-graph:{}:{}",
                        operation.event.id, work_id
                    ),
                    team_run_id: team_run_id.to_string(),
                    kind,
                    work_id: work_id.to_string(),
                    work_version,
                    source_event_ref: operation.event.id.clone(),
                    member_run_id: None,
                    status: HostAttentionStatus::Actionable,
                    attempt: 0,
                    claim_id: None,
                    claimed_host_surface: None,
                    claimed_host_thread_id: None,
                    claimed_host_lease_id: None,
                    claimed_host_lease_generation: None,
                    claimed_host_lease_owner_id: None,
                    claimed_recipient_member_run_id: None,
                    claimed_recipient_session_id: None,
                    claimed_recipient_session_generation: None,
                    claimed_node_daemon_id: None,
                    claimed_node_daemon_generation: None,
                    provider_receipt_id: None,
                    last_failure_reason: None,
                    created_at: operation.event.created_at.clone(),
                    updated_at: operation.event.created_at.clone(),
                })
            })
            .collect()
    }

    pub(super) fn ensure_downstream_host_attentions_for_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<Vec<HostAttention>> {
        Self::downstream_host_attentions_for_work_operation(operation)?
            .iter()
            .map(|attention| self.ensure_host_attention_unlocked(attention))
            .collect()
    }

    /// Replace the complete prerequisite set under the Store's global write
    /// lock. This is the only current dependency writer: the WorkOperation is
    /// CAS-fenced, append-only, and exact-request idempotent.
    pub fn replace_work_dependencies(
        &self,
        work_id: &str,
        expected_version: u64,
        prerequisite_work_ids: Vec<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        let mut requested = prerequisite_work_ids;
        requested.sort();
        let request_payload = serde_json::json!({
            "work_id": work_id,
            "expected_version": expected_version,
            "prerequisite_work_ids": requested,
            "performed_by_actor": context.performed_by_actor,
            "authority_actor": context.authority_actor,
            "causation_ref": context.causation_ref,
        });
        require_host_actor(&context.performed_by_actor)?;
        let works = self.latest_works_unlocked()?;
        let current = works
            .get(work_id)
            .cloned()
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        let (mutation_context, request_fingerprint) = self
            .canonical_work_command_context_unlocked(
                &current,
                expected_version,
                "work.dependencies.replace",
                &context,
                &request_payload,
            )?;
        if let Some(replay) = self.replay_current_work_mutation_unlocked(
            &mutation_context,
            work_id,
            &request_fingerprint,
        )? {
            return Ok(replay.projection);
        }
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "WORK_VERSION_CONFLICT: Work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        if current.phase != WorkPhase::Open {
            return Err(StoreError::Conflict(format!(
                "WORK_DEPENDENCIES_IMMUTABLE: Work {work_id} is {:?}; dependencies may change only while Open",
                current.phase
            )));
        }
        if self.work_has_active_execution_binding_unlocked(work_id)? {
            return Err(StoreError::Conflict(format!(
                "WORK_EXECUTION_BINDING_ACTIVE: release the active binding before changing dependencies for Work {work_id}"
            )));
        }

        let all_works = works.values().cloned().collect::<Vec<_>>();
        let change = prepare_dependency_change(&current, requested, &all_works)
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let mut next = current.clone();
        next.prerequisite_work_ids = change.prerequisite_work_ids.clone();
        next.version = next.version.saturating_add(1);
        next.updated_at = context.created_at.clone();
        let result = self.commit_current_work_mutation_unlocked(
            &mutation_context,
            "dependencies_changed",
            serde_json::json!({
                "request_fingerprint": request_fingerprint,
                "change": change,
            }),
            &next,
            Vec::new(),
            Vec::new(),
        )?;
        Ok(result.projection)
    }

    /// Derive the graph from current Work projections. No reverse edges or
    /// readiness flags are stored, so this read model cannot drift from Work.
    pub fn work_graph(&self, accountable_team_id: &str) -> StoreResult<StoreWorkGraph> {
        self.init()?;
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|work| work.accountable_team_id.as_deref() == Some(accountable_team_id))
            .collect::<Vec<_>>();
        let mut nodes = works
            .iter()
            .map(|work| StoreWorkGraphNode {
                work: work.clone(),
                successor_work_ids: derive_work_successor_ids(&work.id, &works),
                readiness: work_readiness(work, &works),
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.work.id.cmp(&right.work.id));
        Ok(StoreWorkGraph {
            accountable_team_id: accountable_team_id.to_string(),
            nodes,
        })
    }

    pub(super) fn work_has_active_execution_binding_unlocked(
        &self,
        work_id: &str,
    ) -> StoreResult<bool> {
        for execution_space_id in self.canonical_execution_space_ids()? {
            if self
                .fabric_work_execution_bindings(&execution_space_id)?
                .iter()
                .any(|binding| {
                    binding.work_id == work_id
                        && binding.status == WorkExecutionBindingStatus::Active
                })
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn require_work_ready_for_execution_unlocked(&self, work: &Work) -> StoreResult<()> {
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        let readiness = work_readiness(work, &works);
        if readiness.ready {
            return Ok(());
        }
        Err(StoreError::Conflict(format!(
            "WORK_NOT_READY: Work {} cannot enter provider execution: {}",
            work.id,
            serde_json::to_string(&readiness.reasons)?
        )))
    }
}

fn canonical_actor(actor: &firm_core::TeamActorRef) -> firm_core::agentfirm_api::ActorRef {
    use firm_core::agentfirm_api::{ActorKind, ActorRef};
    ActorRef {
        kind: match actor.kind {
            TeamActorKind::Service => ActorKind::Service,
            TeamActorKind::Host
            | TeamActorKind::AgentMember
            | TeamActorKind::ProviderRuntimeProjection => ActorKind::AgentMember,
            TeamActorKind::Operator => ActorKind::Human,
        },
        id: actor.id.clone(),
    }
}
