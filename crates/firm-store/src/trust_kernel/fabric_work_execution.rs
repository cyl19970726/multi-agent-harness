use super::fabric_foundation::RuntimeBindingAdmission;
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkExecutionBindingReconciliation {
    Current,
    Released(Box<CanonicalMutationResult<WorkExecutionBinding>>),
    AlreadySettled(Box<WorkExecutionBinding>),
}

impl HarnessStore {
    /// Return the immutable exact runtime authority captured when one
    /// WorkExecutionBinding was created. Later binding lifecycle projections
    /// cannot replace or infer this MemberRun/session generation evidence.
    pub fn work_execution_runtime_binding(
        &self,
        execution_space_id: &str,
        binding_id: &str,
    ) -> StoreResult<firm_core::agentfirm_api::RuntimeCommandBinding> {
        let matches = self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| {
                envelope.execution_space_id == execution_space_id
                    && envelope.operation.event.aggregate_kind == "work_execution_binding"
                    && envelope.operation.event.aggregate_id == binding_id
                    && envelope.operation.event.transition == "bound"
            })
            .collect::<Vec<_>>();
        let [envelope] = matches.as_slice() else {
            return Err(StoreError::Conflict(format!(
                "WORK_EXECUTION_RUNTIME_BINDING_NOT_PROVABLE: WorkExecutionBinding {binding_id} must have exactly one canonical bound source fact"
            )));
        };
        serde_json::from_value(envelope.operation.event.payload["runtime_binding"].clone())
            .map_err(|error| {
                StoreError::Conflict(format!(
                    "WORK_EXECUTION_RUNTIME_BINDING_INVALID: WorkExecutionBinding {binding_id}: {error}"
                ))
            })
    }

    pub fn fabric_work_execution_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkExecutionBinding>> {
        let mut latest = BTreeMap::<String, WorkExecutionBinding>::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id != execution_space_id {
                continue;
            }
            if envelope.operation.event.aggregate_kind == "work_execution_binding" {
                let binding = event_projection::<WorkExecutionBinding>(&envelope)?;
                latest.insert(binding.id.clone(), binding);
            }
            // StopSession atomically quiesces active Work bindings in the same
            // RuntimeCommand operation. Side records are full resulting
            // projections and participate in latest-version selection.
            for record in envelope.operation.immutable_side_records {
                if let Ok(binding) = serde_json::from_value::<WorkExecutionBinding>(record) {
                    let replace = latest
                        .get(&binding.id)
                        .is_none_or(|current| binding.version > current.version);
                    if replace {
                        latest.insert(binding.id.clone(), binding);
                    }
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn fabric_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalWorkDelivery>> {
        Ok(self
            .canonical_fabric_work_deliveries_unlocked(execution_space_id)?
            .into_values()
            .collect())
    }

    fn work_execution_binding_is_current_unlocked(
        &self,
        execution_space_id: &str,
        binding: &WorkExecutionBinding,
    ) -> StoreResult<bool> {
        if binding.status != WorkExecutionBindingStatus::Active {
            return Ok(false);
        }
        let work = self
            .latest_works_unlocked()?
            .remove(&binding.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing Work source fact",
                    "work_execution_binding",
                    &binding.id,
                    Some(binding.version),
                )
            })?;
        let responsibility_changed = self.work_responsibility_changed_after_revision_unlocked(
            &binding.work_id,
            binding.work_revision,
        )?;
        let memberships = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .filter(|membership| membership.id == binding.team_membership_id)
            .collect::<Vec<_>>();
        let [membership] = memberships.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkExecutionBinding must reference exactly one TeamMembership source fact",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        };
        let sessions = self
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .filter(|session| session.id == binding.agent_session_id)
            .collect::<Vec<_>>();
        let [session] = sessions.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkExecutionBinding must reference exactly one AgentSession source fact",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        };
        let runtime_binding =
            self.work_execution_runtime_binding(execution_space_id, &binding.id)?;
        let member_run_id = runtime_binding
            .target_member_run_id
            .as_deref()
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "WorkExecutionBinding source fact is missing the exact MemberRun id",
                    "work_execution_binding",
                    &binding.id,
                    Some(binding.version),
                )
            })?;
        let member_run_generation =
            runtime_binding
                .target_member_run_generation
                .ok_or_else(|| {
                    trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "WorkExecutionBinding source fact is missing the exact MemberRun generation",
                    "work_execution_binding",
                    &binding.id,
                    Some(binding.version),
                )
                })?;
        let member_runs = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .filter(|member| member.id == member_run_id)
            .collect::<Vec<_>>();
        let [member_run] = member_runs.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkExecutionBinding must resolve exactly one immutable MemberRun source fact",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        };
        let deliveries = self.canonical_fabric_work_deliveries_unlocked(execution_space_id)?;
        let delivery = deliveries.get(&binding.delivery_id).ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkExecutionBinding is missing its canonical WorkDelivery source fact",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            )
        })?;
        if delivery.work_id != binding.work_id
            || delivery.work_revision != binding.work_revision
            || delivery.work_execution_binding_id != binding.id
            || delivery.recipient_agent_member_id != binding.agent_member_id
            || delivery.recipient_session_id != binding.agent_session_id
            || delivery.recipient_session_generation != binding.agent_session_generation
            || delivery.target_node_id != session.node_id
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkExecutionBinding and canonical WorkDelivery source facts conflict",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let effect_is_frozen = matches!(
            delivery.status,
            WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
        );
        let revision_is_current = binding.work_revision == work.version
            || (binding.work_revision < work.version && effect_is_frozen);
        let stable_authority_matches = work.active_member_run_id.is_none()
            && !work.is_terminal()
            && !responsibility_changed
            && revision_is_current
            && work.accountable_team_id.as_deref() == Some(binding.team_id.as_str())
            && work.owner_member_id.as_deref() == Some(binding.agent_member_id.as_str())
            && work.assignee_membership_id.as_deref() == Some(binding.team_membership_id.as_str())
            && membership.state == TeamMembershipStatus::Active
            && membership.team_id == binding.team_id
            && membership.agent_member_id == binding.agent_member_id
            && session.agent_member_id == binding.agent_member_id
            && session.runtime_generation == binding.agent_session_generation
            && session.lifecycle != AgentSessionStatus::Closed
            && member_run.runtime_generation == member_run_generation
            && member_run.has_live_runtime_authority();
        if !stable_authority_matches {
            return Ok(false);
        }
        match self.require_live_runtime_binding_unlocked(
            session,
            &runtime_binding,
            RuntimeBindingAdmission::RuntimeCommand {
                // A Work may be bound before the first provider Open returns
                // the native session id.  Attaching that id to the same exact
                // AgentSession/runtime/driver generation is durable progress,
                // not a stale execution authority.
                allow_native_session_attachment: true,
            },
            "work_execution_binding",
            &binding.id,
            Some(work.version),
        ) {
            Ok(()) => Ok(true),
            Err(error)
                if error.trust_error().is_some_and(|error| {
                    matches!(
                        error.code,
                        TrustErrorCode::MemberRunGenerationFenced
                            | TrustErrorCode::SupervisorGenerationFenced
                            | TrustErrorCode::NativeSessionMissing
                            | TrustErrorCode::NativeSessionIncompatible
                    )
                }) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn canonical_fabric_work_deliveries_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, CanonicalWorkDelivery>> {
        let mut deliveries = BTreeMap::<String, CanonicalWorkDelivery>::new();
        for delivery in self.trust_side_records::<CanonicalWorkDelivery>(execution_space_id)? {
            let decision = firm_application::fold_canonical_work_delivery(
                deliveries.get(&delivery.id),
                &delivery,
            )
            .map_err(|error| {
                StoreError::Conflict(format!(
                    "CANONICAL_WORK_DELIVERY_FOLD_CONFLICT: delivery {}: {error}",
                    delivery.id
                ))
            })?;
            if decision != firm_application::ProjectionFoldDecision::Replay {
                deliveries.insert(delivery.id.clone(), delivery);
            }
        }
        Ok(deliveries)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_for_provider(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        dispatch_mode: firm_core::agentfirm_api::RuntimeDispatchMode,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<ProviderInvocation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        let binding = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .find(|binding| binding.id == delivery.work_execution_binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery binding is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == delivery.recipient_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "WorkDelivery session is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&delivery.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkDelivery Work is missing",
                    "work_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        // Provider admission is decided from the locked authoritative Work
        // graph, never from a stale RoleView or client-side readiness hint.
        self.require_work_ready_for_execution_unlocked(&work)?;
        if delivery.status != WorkDeliveryStatus::Queued
            || delivery.target_node_id != node_id
            || binding.status != WorkExecutionBindingStatus::Active
            || binding.work_revision != work.version
            || delivery.work_revision != work.version
            || session.agent_member_id != delivery.recipient_agent_member_id
            || session.runtime_generation != delivery.recipient_session_generation
            || session.node_daemon_id != daemon_id
            || session.node_daemon_generation != daemon_generation
            || session.lifecycle == AgentSessionStatus::Closed
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery no longer matches its exact active binding, Work revision, session, or NodeDaemon generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let invocation_binding =
            self.work_execution_runtime_binding(&context.execution_space_id, &binding.id)?;
        self.require_live_runtime_binding_unlocked(
            &session,
            &invocation_binding,
            RuntimeBindingAdmission::Invocation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_node_daemon_generation = Some(daemon_generation);
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        let content = serde_json::to_string(&serde_json::json!({
            "work_id": work.id,
            "work_revision": work.version,
            "title": work.title,
            "context_markdown": work.context_markdown,
            "completion_criteria_markdown": work.completion_criteria_markdown,
        }))?;
        let invocation = ProviderInvocation {
            id: format!("provider-invocation:{}:{}", delivery.id, delivery.attempt),
            source_plane: "work_delivery".into(),
            source_record_id: delivery.id.clone(),
            recipient_agent_member_id: delivery.recipient_agent_member_id.clone(),
            recipient_session_id: delivery.recipient_session_id.clone(),
            recipient_session_generation: delivery.recipient_session_generation,
            node_id: node_id.to_string(),
            node_daemon_id: daemon_id.to_string(),
            node_daemon_generation: daemon_generation,
            provider: session.provider_kind.clone(),
            dispatch_mode,
            binding: invocation_binding,
            permission_ceiling: session.effective_permission_ceiling,
            content_fingerprint: canonical_json_fingerprint(
                &serde_json::json!({"content": content}),
            ),
            content,
            created_at: updated_at.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "provider_invocation",
            &invocation.id,
            "prepared_from_work_delivery",
            serde_json::json!({"delivery_id": delivery_id, "claim_id": claim_id}),
            &invocation,
            vec![serde_json::to_value(delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_work_provider_receipt(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalWorkDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
            || delivery.target_node_id != node_id
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt does not match the exact WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery_receipt",
            delivery_id,
            "provider_received",
            serde_json::json!({"claim_id": claim_id, "provider_receipt_id": provider_receipt_id}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_work_provider_failure(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        failure_code: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<CanonicalWorkDelivery>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_delivery",
            delivery_id,
        )?;
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_node_daemon_generation != Some(daemon_generation)
            || delivery.target_node_id != node_id
            || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider failure does not match one exact unreceived WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some(failure_code.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery_failure",
            delivery_id,
            "provider_negative_ack",
            serde_json::json!({"claim_id": claim_id, "failure_code": failure_code}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn bind_work_execution(
        &self,
        _context: &MutationContext,
        binding: WorkExecutionBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            "WORK_EXECUTION_ADMISSION_REQUIRED: WorkExecutionBinding must resolve stable responsibility through exact runtime admission",
            "work_execution_binding",
            &binding.id,
            None,
        ))
    }

    #[cfg(test)]
    pub(crate) fn bind_work_execution_fixture(
        &self,
        context: &MutationContext,
        binding: WorkExecutionBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|session| session.id == binding.agent_session_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "test fixture AgentSession {} not found",
                    binding.agent_session_id
                ))
            })?;
        let work = self
            .latest_works()?
            .into_iter()
            .find(|work| work.id == binding.work_id)
            .ok_or_else(|| StoreError::Conflict("test fixture Work not found".into()))?;
        let current_runs = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .filter(|run| {
                run.agent_member_id == binding.agent_member_id
                    && run.team_run_id == work.team_run_id
                    && run.has_live_runtime_authority()
            })
            .collect::<Vec<_>>();
        let [current_run] = current_runs.as_slice() else {
            return Err(StoreError::Conflict(
                "test fixture binding requires exactly one active MemberRun".into(),
            ));
        };
        let mut runtime_binding = runtime_binding_for_session(&session);
        runtime_binding.target_member_run_id = Some(current_run.id.clone());
        runtime_binding.target_member_run_generation = Some(current_run.runtime_generation);
        let mut exact_context = context.clone();
        exact_context.authenticated_actor = ActorRef {
            kind: ActorKind::Service,
            id: session.node_daemon_id,
        };
        self.bind_responsible_work_execution(&exact_context, &runtime_binding, binding)
    }

    /// Resolve stable Work responsibility into one exact current runtime
    /// authority and commit the binding under the same Store writer lock.
    ///
    /// The Work continues to be owned by its TeamMembership/AgentMember.  The
    /// supplied runtime binding is only an admission fence: it proves the
    /// current MemberRun, AgentSession, NodeDaemon and TeamSupervisor
    /// generations without copying runtime ownership into the Work ledger.
    pub fn bind_responsible_work_execution(
        &self,
        context: &MutationContext,
        runtime_binding: &firm_core::agentfirm_api::RuntimeCommandBinding,
        binding: WorkExecutionBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "binding": binding,
            "runtime_binding": runtime_binding,
        });
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        let membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.team_membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing TeamMembership",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.agent_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing AgentSession",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "work_execution_binding",
            &binding.id,
        )?;
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            &fingerprint,
        )? {
            return Ok(replay);
        }
        let work = self
            .latest_works_unlocked()?
            .remove(&binding.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkExecutionBinding references a missing Work",
                    "work",
                    &binding.work_id,
                    None,
                )
            })?;
        if work.assignee_membership_id.as_deref() != Some(membership.id.as_str())
            || work.owner_member_id.as_deref() != Some(binding.agent_member_id.as_str())
            || membership.agent_member_id != binding.agent_member_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding does not match the Work's exact current TeamMembership responsibility",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        }
        let (Some(member_run_id), Some(member_run_generation)) = (
            runtime_binding.target_member_run_id.as_deref(),
            runtime_binding.target_member_run_generation,
        ) else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Work execution admission requires an exact MemberRun identity and generation",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        };
        let active_member_runs = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .filter(|member| {
                member.team_run_id == work.team_run_id
                    && member.agent_member_id == binding.agent_member_id
                    && member.has_live_runtime_authority()
            })
            .collect::<Vec<_>>();
        let [current_member_run] = active_member_runs.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Work responsibility does not resolve to exactly one active MemberRun",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        };
        if work.active_member_run_id.is_some() {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "legacy Work runtime authority is retired and cannot be admitted for current execution",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        }
        if current_member_run.id != member_run_id
            || current_member_run.runtime_generation != member_run_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Work execution admission used a stale or conflicting MemberRun generation",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        }
        self.require_live_runtime_binding_unlocked(
            &session,
            runtime_binding,
            RuntimeBindingAdmission::RuntimeCommand {
                allow_native_session_attachment: false,
            },
            "work_execution_binding",
            &binding.id,
            Some(work.version),
        )?;
        self.bind_work_execution_unlocked(context, binding, request_payload)
    }

    fn bind_work_execution_unlocked(
        &self,
        context: &MutationContext,
        binding: WorkExecutionBinding,
        request_payload: serde_json::Value,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        if binding.version != 1 || binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "new WorkExecutionBinding must be active at version 1",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.team_membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing TeamMembership",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let session = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .find(|row| row.id == binding.agent_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding references a missing AgentSession",
                    "work_execution_binding",
                    &binding.id,
                    None,
                )
            })?;
        let work = self
            .latest_works_unlocked()?
            .remove(&binding.work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkExecutionBinding references a missing Work",
                    "work",
                    &binding.work_id,
                    None,
                )
            })?;
        let expected_delivery_id = format!(
            "work-delivery:{}:{}",
            binding.work_id, binding.binding_generation
        );
        let expected_binding_generation = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .filter(|existing| existing.work_id == binding.work_id)
            .map(|existing| existing.binding_generation)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        if work.version != binding.work_revision {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkExecutionBinding must freeze the exact current Work revision",
                "work_execution_binding",
                &binding.id,
                Some(work.version),
            ));
        }
        if membership.state != TeamMembershipStatus::Active
            || membership.agent_member_id != binding.agent_member_id
            || session.agent_member_id != binding.agent_member_id
            || session.node_id != membership.node_id
            || session.runtime_generation != binding.agent_session_generation
            || session.lifecycle == AgentSessionStatus::Closed
            || work.accountable_team_id.as_deref() != Some(membership.team_id.as_str())
            || binding.team_id != membership.team_id
            || binding.binding_generation != expected_binding_generation
            || binding.delivery_id != expected_delivery_id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding identity, session generation, Team, Work revision, or monotonic binding generation mismatch",
                "work_execution_binding",
                &binding.id,
                None,
            ));
        }
        // Bind only a Work that is ready at this exact Store revision. The
        // provider claim repeats this check immediately before the effect.
        self.require_work_ready_for_execution_unlocked(&work)?;
        if self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .iter()
            .any(|row| {
                row.work_id == binding.work_id && row.status == WorkExecutionBindingStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work already has an active WorkExecutionBinding; explicit release is required",
                "work",
                &binding.work_id,
                Some(work.version),
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "bound",
            request_payload,
            &binding,
            vec![serde_json::to_value(CanonicalWorkDelivery {
                id: format!(
                    "work-delivery:{}:{}",
                    binding.work_id, binding.binding_generation
                ),
                work_id: binding.work_id.clone(),
                work_revision: binding.work_revision,
                work_execution_binding_id: binding.id.clone(),
                recipient_agent_member_id: binding.agent_member_id.clone(),
                recipient_session_id: binding.agent_session_id.clone(),
                recipient_session_generation: binding.agent_session_generation,
                target_node_id: session.node_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_node_daemon_generation: None,
                provider_receipt_id: None,
                failure_code: None,
                version: 1,
                created_at: binding.bound_at.clone(),
                updated_at: binding.bound_at.clone(),
            })?],
            Vec::new(),
        )
    }

    pub fn release_work_execution_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        member_run_id: &str,
        member_run_generation: u64,
        ended_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut binding = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .find(|binding| binding.id == binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding not found",
                    "work_execution_binding",
                    binding_id,
                    None,
                )
            })?;
        let exact_member = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == binding.agent_member_id;
        if !exact_member {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "direct WorkExecutionBinding release requires the exact AgentMember",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let runtime_binding =
            self.work_execution_runtime_binding(&context.execution_space_id, binding_id)?;
        if runtime_binding.target_member_run_id.as_deref() != Some(member_run_id)
            || runtime_binding.target_member_run_generation != Some(member_run_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "direct WorkExecutionBinding release does not bind the exact admitted MemberRun generation",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let current_member_runs = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .filter(|member| member.id == member_run_id)
            .collect::<Vec<_>>();
        let [current_member_run] = current_member_runs.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "direct WorkExecutionBinding release must resolve exactly one current MemberRun",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        };
        if current_member_run.agent_member_id != binding.agent_member_id
            || current_member_run.runtime_generation != member_run_generation
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "direct WorkExecutionBinding release is fenced by the current MemberRun generation",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let request_payload = serde_json::json!({"ended_at": ended_at});
        let fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "work_execution_binding",
            binding_id,
            &fingerprint,
        )? {
            return Ok(replay);
        }
        self.release_work_execution_binding_unlocked(context, &mut binding, ended_at, false)
    }

    pub fn release_work_execution_binding_if_stale(
        &self,
        context: &MutationContext,
        binding_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        ended_at: &str,
    ) -> StoreResult<WorkExecutionBindingReconciliation> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut binding = self
            .fabric_work_execution_bindings(&context.execution_space_id)?
            .into_iter()
            .find(|binding| binding.id == binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding not found during stale reconciliation",
                    "work_execution_binding",
                    binding_id,
                    None,
                )
            })?;
        self.require_exact_binding_node_daemon_unlocked(
            context,
            &binding,
            node_id,
            daemon_id,
            daemon_generation,
        )?;
        let request_payload = serde_json::json!({"ended_at": ended_at});
        let fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) = self.replay_trust_projection_unlocked(
            context,
            "work_execution_binding",
            binding_id,
            &fingerprint,
        )? {
            return Ok(WorkExecutionBindingReconciliation::Released(Box::new(
                replay,
            )));
        }
        if binding.status != WorkExecutionBindingStatus::Active {
            return Ok(WorkExecutionBindingReconciliation::AlreadySettled(
                Box::new(binding),
            ));
        }
        if self.work_execution_binding_is_current_unlocked(&context.execution_space_id, &binding)? {
            return Ok(WorkExecutionBindingReconciliation::Current);
        }
        Ok(WorkExecutionBindingReconciliation::Released(Box::new(
            self.release_work_execution_binding_unlocked(context, &mut binding, ended_at, true)?,
        )))
    }

    fn require_exact_binding_node_daemon_unlocked(
        &self,
        context: &MutationContext,
        binding: &WorkExecutionBinding,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
    ) -> StoreResult<()> {
        let sessions = self
            .fabric_agent_sessions(&context.execution_space_id)?
            .into_iter()
            .filter(|session| session.id == binding.agent_session_id)
            .collect::<Vec<_>>();
        let [session] = sessions.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkExecutionBinding release must resolve exactly one AgentSession source fact",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        };
        if session.node_id != node_id
            || session.node_daemon_id != daemon_id
            || session.node_daemon_generation != daemon_generation
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "WorkExecutionBinding reconciliation caller does not hold the exact bound NodeDaemon generation",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "work_execution_binding",
            &binding.id,
        )
    }

    fn release_work_execution_binding_unlocked(
        &self,
        context: &MutationContext,
        binding: &mut WorkExecutionBinding,
        ended_at: &str,
        exact_daemon_already_verified: bool,
    ) -> StoreResult<CanonicalMutationResult<WorkExecutionBinding>> {
        if binding.status != WorkExecutionBindingStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active WorkExecutionBinding can be released",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let exact_member = context.authenticated_actor.kind == ActorKind::AgentMember
            && context.authenticated_actor.id == binding.agent_member_id;
        if !exact_member && !exact_daemon_already_verified {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "direct WorkExecutionBinding release requires the exact AgentMember; NodeDaemon reconciliation must use the generation-fenced atomic API",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let mut delivery = self
            .canonical_fabric_work_deliveries_unlocked(&context.execution_space_id)?
            .remove(&binding.delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkExecutionBinding release requires its exact canonical WorkDelivery",
                    "work_execution_binding",
                    &binding.id,
                    Some(binding.version),
                )
            })?;
        if delivery.work_execution_binding_id != binding.id
            || delivery.work_id != binding.work_id
            || delivery.work_revision != binding.work_revision
            || delivery.recipient_agent_member_id != binding.agent_member_id
            || delivery.recipient_session_id != binding.agent_session_id
            || delivery.recipient_session_generation != binding.agent_session_generation
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkExecutionBinding release found conflicting canonical delivery evidence",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        if matches!(
            delivery.status,
            WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
        ) {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "claimed or provider-received WorkDelivery must reach an explicit terminal provider outcome before releasing its binding",
                "work_delivery",
                &delivery.id,
                Some(delivery.version),
            ));
        }
        let mut side_records = Vec::new();
        if delivery.status == WorkDeliveryStatus::Queued {
            delivery.status = WorkDeliveryStatus::Failed;
            delivery.failure_code =
                Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM".to_string());
            delivery.version += 1;
            delivery.updated_at = ended_at.to_string();
            side_records.push(serde_json::to_value(&delivery)?);
        }
        binding.status = WorkExecutionBindingStatus::Released;
        binding.version += 1;
        binding.ended_at = Some(ended_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "work_execution_binding",
            &binding.id,
            "released",
            serde_json::json!({"ended_at": ended_at}),
            binding,
            side_records,
            Vec::new(),
        )
    }

    pub(super) fn result_submission_released_binding_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        binding: &WorkExecutionBinding,
        ended_at: &str,
    ) -> StoreResult<WorkExecutionBinding> {
        if binding.status != WorkExecutionBindingStatus::Active
            || binding.work_id != work.id
            || binding.work_revision > work.version
        {
            return Err(trust_error(
                TrustErrorCode::WorkExecutionBindingActive,
                "Result submission requires the exact active WorkExecutionBinding",
                "work_execution_binding",
                &binding.id,
                Some(binding.version),
            ));
        }
        let delivery = self
            .canonical_fabric_work_deliveries_unlocked(execution_space_id)?
            .remove(&binding.delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::DeliveryRecoveryUncertain,
                    "Result submission requires the exact canonical WorkDelivery",
                    "work_delivery",
                    &binding.delivery_id,
                    None,
                )
            })?;
        if delivery.work_execution_binding_id != binding.id
            || delivery.work_id != binding.work_id
            || delivery.work_revision != binding.work_revision
            || delivery.recipient_agent_member_id != binding.agent_member_id
            || delivery.recipient_session_id != binding.agent_session_id
            || delivery.recipient_session_generation != binding.agent_session_generation
            || delivery.status != WorkDeliveryStatus::ProviderReceived
            || delivery
                .provider_receipt_id
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "Result submission requires exact provider-received delivery evidence",
                "work_delivery",
                &delivery.id,
                Some(delivery.version),
            ));
        }
        let mut released = binding.clone();
        released.status = WorkExecutionBindingStatus::Released;
        released.version += 1;
        released.ended_at = Some(ended_at.to_string());
        Ok(released)
    }
}
