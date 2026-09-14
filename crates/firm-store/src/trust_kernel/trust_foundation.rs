use super::*;
#[path = "trust_read_model.rs"]
mod read_model;
use read_model::TrustReadModel;

/// Reserved in a canonical idempotency key: a paired `work` transition's key
/// is the caller's with this separator and the transition name appended, so
/// every existing scan that resolves a command by its exact key still finds
/// exactly the caller's envelope.
pub(in crate::trust_kernel) const PAIRED_KEY_SEPARATOR: char = '#';

/// Refuse a caller idempotency key that could collide with a derived paired
/// key, at every entrance that appends a canonical trust envelope.
///
/// This is a request-shape refusal, not a replay: it uses the same code,
/// resource kind and id shape as `required`, the sibling argument check in
/// this module, so a caller is never told its key was "already used for a
/// different Work" when the key is simply malformed.
///
/// Scope: every entrance that appends a canonical envelope, which since the
/// W4 writer cutover is every Work command — the ported ledger writers reach
/// it through `commit_current_work_mutation_unlocked`, and the two trust
/// entrances that already owned their transitions reach it directly.
fn require_unreserved_idempotency_key(idempotency_key: &str) -> StoreResult<()> {
    if idempotency_key.contains(PAIRED_KEY_SEPARATOR) {
        return Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            format!(
                "idempotency_key must not contain {PAIRED_KEY_SEPARATOR:?}: the separator is \
                 reserved for the paired canonical Work transition a command may commit"
            ),
            "request",
            "idempotency_key",
            None,
        ));
    }
    Ok(())
}

/// One canonical Work transition, ready to be staged into a trust write.
pub(crate) struct CurrentWorkMutation {
    pub context: MutationContext,
    pub transition: String,
    pub request_payload: Value,
    pub work: Work,
    pub immutable_side_records: Vec<Value>,
    pub initial_outbox_records: Vec<Value>,
}

/// One `work` aggregate transition committed atomically with another
/// canonical projection that produced it.
pub(in crate::trust_kernel) struct PairedWorkTransition {
    pub transition: &'static str,
    pub kind: firm_core::WorkEventKind,
    pub expected_version: u64,
    pub work: Work,
}

/// One `member_run` projection committed atomically with the `agent_session`
/// that authorizes it.
///
/// This exists for exactly one reason: `AgentSession.native_session_ref` is the
/// authority for the provider-native session pointer (ADR 0071) and
/// `MemberRun.native_session` is a projection of it. A projection written in a
/// second transaction can be observed without its authority, or survive when
/// the authority write fails — which is how three copies of one pointer came to
/// need a validator whose only job was to reject their disagreement.
pub(in crate::trust_kernel) struct PairedMemberRunProjection {
    pub transition: &'static str,
    pub expected_version: u64,
    pub run: firm_core::agentfirm_api::MemberRun,
}

/// A second canonical aggregate written into the SAME atomic ledger rewrite as
/// the primary one.
///
/// Both arms share one event builder, so a paired envelope is constructed
/// identically whichever aggregate it carries; only the side records differ,
/// because a `work` revision must also carry the derived `WorkEvent` its
/// readers already bind to.
pub(in crate::trust_kernel) enum PairedAggregateTransition {
    Work(PairedWorkTransition),
    MemberRun(PairedMemberRunProjection),
}

impl HarnessStore {
    pub(crate) fn replay_current_work_mutation_unlocked(
        &self,
        context: &MutationContext,
        work_id: &str,
        request_fingerprint: &str,
    ) -> StoreResult<Option<CanonicalMutationResult<Work>>> {
        self.replay_trust_projection_unlocked(context, "work", work_id, request_fingerprint)
    }

    /// Commit a mutation to the current canonical Work aggregate while
    /// preserving the single Work revision sequence recovered across legacy
    /// history and current trust operations. Since W4 this is the ONLY seam a
    /// Work transition may be written through: there is no second Work writer
    /// and no ledger append left to fall back to.
    pub(crate) fn commit_current_work_mutation_unlocked(
        &self,
        context: &MutationContext,
        transition: &str,
        request_payload: Value,
        work: &Work,
        immutable_side_records: Vec<Value>,
        initial_outbox_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let result = self.stage_current_work_mutation_unlocked(
            &mut committed,
            &CurrentWorkMutation {
                context: context.clone(),
                transition: transition.to_string(),
                request_payload,
                work: work.clone(),
                immutable_side_records,
                initial_outbox_records,
            },
        )?;
        if !result.replayed {
            self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        }
        Ok(result)
    }

    /// Commit several canonical Work transitions in ONE atomic ledger write.
    ///
    /// A sweep that plans every write and then applies them is only honest if
    /// applying them is indivisible: a partial sweep leaves some Work migrated
    /// and some not, with no single revision to reconcile from. Each entry is
    /// still fenced and replay-checked on its own aggregate; an entry whose
    /// exact request already committed contributes its committed projection and
    /// appends nothing.
    pub(crate) fn commit_current_work_mutations_atomic_unlocked(
        &self,
        mutations: &[CurrentWorkMutation],
    ) -> StoreResult<Vec<Work>> {
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let mut wrote = false;
        let mut projections = Vec::new();
        for mutation in mutations {
            let result = self.stage_current_work_mutation_unlocked(&mut committed, mutation)?;
            wrote |= !result.replayed;
            projections.push(result.projection);
        }
        if wrote {
            self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        }
        Ok(projections)
    }

    /// Validate, fence and stage one canonical Work transition onto `committed`
    /// without writing. Staging against the in-progress vector — not the
    /// on-disk read — is what lets a batch derive correct `sequence` and
    /// `store_sequence` values for several transitions at once.
    fn stage_current_work_mutation_unlocked(
        &self,
        committed: &mut Vec<TrustOperationEnvelope>,
        mutation: &CurrentWorkMutation,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        let CurrentWorkMutation {
            context,
            transition,
            request_payload,
            work,
            immutable_side_records,
            initial_outbox_records,
        } = mutation;
        work.validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        require_unreserved_idempotency_key(&context.idempotency_key)?;
        if work.version != context.expected_version.saturating_add(1) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "canonical Work mutation must advance the exact expected revision once",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(request_payload));
        // Against the envelopes already in hand. A Work command's entrance
        // asked the same question before its guards ran and nothing can have
        // been written since — the store write lock is held across both — so
        // this is the check that covers the callers with no entrance of their
        // own (the trust entrances and the responsibility migration's batch),
        // and it costs no second read of the journal.
        if let Some(replay) =
            Self::replay_trust_projection_in(committed, context, "work", &work.id, &fingerprint)?
        {
            return Ok(replay);
        }
        let previous = committed
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "work"
                    && envelope.operation.event.aggregate_id == work.id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "work".into(),
            aggregate_id: work.id.clone(),
            sequence: previous
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: transition.clone(),
            expected_version: context.expected_version,
            resulting_version: work.version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload.clone(),
            created_at: now_string(),
        };
        let mut initial_outbox_records = initial_outbox_records.clone();
        initial_outbox_records.extend(
            self.canonical_terminal_work_outbox_unlocked(work, &event)?
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(work)?,
            immutable_side_records: immutable_side_records.clone(),
            initial_outbox_records,
        };
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        });
        Ok(CanonicalMutationResult {
            projection: work.clone(),
            event,
            replayed: false,
        })
    }

    #[cfg(any())]
    pub(super) fn require_current_trust_supervisor_unlocked(
        &self,
        context: &MutationContext,
        team_run_id: &str,
        supervisor_generation: u64,
        resource_kind: &str,
        resource_id: &str,
        current_version: Option<u64>,
    ) -> StoreResult<()> {
        let lease = self
            .latest_team_supervisor_lease(team_run_id)?
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::SupervisorGenerationFenced,
                    "Team Supervisor lease is missing",
                    resource_kind,
                    resource_id,
                    current_version,
                )
            })?;
        if context.authenticated_actor.kind != firm_core::agentfirm_api::ActorKind::Service
            || context.authenticated_actor.id != lease.supervisor_id
            || lease.generation != supervisor_generation
            || lease.execution_space_id != context.execution_space_id
            || lease.status != firm_core::TeamSupervisorLeaseStatus::Active
            || lease.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "delivery mutation used a stale or unauthorized Team Supervisor lease",
                resource_kind,
                resource_id,
                current_version,
            ));
        }
        let parent = self
            .latest_node_daemon_lease(&lease.node_id)?
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::SupervisorGenerationFenced,
                    "Team Supervisor parent NodeDaemon lease is missing",
                    resource_kind,
                    resource_id,
                    current_version,
                )
            })?;
        if parent.status != firm_core::NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= current_unix_ms()
        {
            return Err(trust_error(
                TrustErrorCode::SupervisorGenerationFenced,
                "delivery mutation used a Supervisor whose parent NodeDaemon lease is stale",
                resource_kind,
                resource_id,
                current_version,
            ));
        }
        Ok(())
    }

    #[cfg(any())]
    pub(super) fn trust_message_team_run_unlocked(
        &self,
        execution_space_id: &str,
        message_id: &str,
    ) -> StoreResult<String> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_message")?
            .remove(message_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery references a missing TeamMessage",
                    "team_message",
                    message_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<TeamMessage>(&envelope))
            .map(|message| message.team_run_id)
    }

    pub(super) fn trust_team_work_unlocked(
        &self,
        team_id: &str,
        work_id: &str,
        work_revision: u64,
    ) -> StoreResult<Work> {
        let work = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "Work not found in the selected Execution Space",
                    "work",
                    work_id,
                    None,
                )
            })?;
        if work.accountable_team_id.as_deref() != Some(team_id) || work.version != work_revision {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "Team-scoped Work authority or exact Work revision does not match",
                "work",
                work_id,
                Some(work.version),
            ));
        }
        Ok(work)
    }

    pub(super) fn require_exact_work_member_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
        actor_session_id: Option<&str>,
    ) -> StoreResult<MemberRun> {
        self.require_exact_work_member_binding_unlocked(
            execution_space_id,
            work,
            actor,
            actor_session_id,
        )
        .map(|(run, _binding)| run)
    }

    pub(super) fn require_exact_work_member_binding_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
        actor_session_id: Option<&str>,
    ) -> StoreResult<(MemberRun, WorkExecutionBinding)> {
        self.require_exact_work_member_binding_with_settlement_unlocked(
            execution_space_id,
            work,
            actor,
            actor_session_id,
            false,
        )
    }

    pub(super) fn require_exact_work_result_binding_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
        actor_session_id: Option<&str>,
    ) -> StoreResult<(MemberRun, WorkExecutionBinding)> {
        self.require_exact_work_member_binding_with_settlement_unlocked(
            execution_space_id,
            work,
            actor,
            actor_session_id,
            true,
        )
    }

    fn require_exact_work_member_binding_with_settlement_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
        actor_session_id: Option<&str>,
        allow_reopened_result_settlement: bool,
    ) -> StoreResult<(MemberRun, WorkExecutionBinding)> {
        if actor.kind != ActorKind::AgentMember
            || work.owner_member_id.as_deref() != Some(actor.id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "member-owned Work mutation requires the exact accountable AgentMember",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        if work.active_member_run_id.is_some() {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "legacy Work runtime authority is retired and cannot authorize current member mutations",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let work_bindings = self
            .fabric_work_execution_bindings(execution_space_id)?
            .into_iter()
            .filter(|binding| binding.work_id == work.id)
            .collect::<Vec<_>>();
        let active_bindings = work_bindings
            .iter()
            .filter(|binding| binding.status == WorkExecutionBindingStatus::Active)
            .collect::<Vec<_>>();
        let binding = match active_bindings.as_slice() {
            [binding] => (*binding).clone(),
            [] if allow_reopened_result_settlement => self
                .require_exact_released_result_binding_unlocked(
                    execution_space_id,
                    work,
                    actor,
                    &work_bindings,
                )?,
            _ => {
                return Err(trust_error(
                    TrustErrorCode::WorkExecutionBindingActive,
                    "member-owned Work mutation requires exactly one active WorkExecutionBinding",
                    "work",
                    &work.id,
                    Some(work.version),
                ));
            }
        };
        let responsibility_changed_after_binding = self
            .work_responsibility_changed_after_revision_unlocked(&work.id, binding.work_revision)?;
        if binding.work_revision > work.version
            || responsibility_changed_after_binding
            || binding.team_id != work.accountable_team_id.as_deref().unwrap_or_default()
            || Some(binding.team_membership_id.as_str()) != work.assignee_membership_id.as_deref()
            || binding.agent_member_id != actor.id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "active WorkExecutionBinding does not match current Work responsibility",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let membership = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .find(|membership| membership.id == binding.team_membership_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "WorkExecutionBinding references a missing TeamMembership",
                    "work",
                    &work.id,
                    Some(work.version),
                )
            })?;
        if membership.state != TeamMembershipStatus::Active
            || membership.team_id != binding.team_id
            || membership.agent_member_id != actor.id
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding membership is not current Work responsibility",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let session = self
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .find(|session| session.id == binding.agent_session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::NativeSessionMissing,
                    "WorkExecutionBinding references a missing AgentSession",
                    "work",
                    &work.id,
                    Some(work.version),
                )
            })?;
        if session.agent_member_id != actor.id
            || session.runtime_generation != binding.agent_session_generation
            || session.lifecycle == AgentSessionStatus::Closed
            || actor_session_id.is_some_and(|id| id != session.id)
        {
            return Err(trust_error(
                TrustErrorCode::NativeSessionIncompatible,
                "WorkExecutionBinding does not reference the exact current AgentSession generation",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let active_runs = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .filter(|run| {
                run.agent_member_id == actor.id
                    && run.team_run_id == work.team_run_id
                    && run.has_live_runtime_authority()
            })
            .collect::<Vec<_>>();
        let [run] = active_runs.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "Work responsibility does not resolve to exactly one current active MemberRun",
                "work",
                &work.id,
                Some(work.version),
            ));
        };
        let admission = self.work_execution_runtime_binding(execution_space_id, &binding.id)?;
        let exact_runtime_generation = admission.target_member_run_id.as_deref()
            == Some(run.id.as_str())
            && admission.target_member_run_generation == Some(run.runtime_generation);
        let reopened_result_settlement = if allow_reopened_result_settlement
            && admission.target_member_run_id.as_deref() == Some(run.id.as_str())
        {
            if let Some(generation) = admission.target_member_run_generation {
                self.member_run_has_exact_close_reopen_lineage_unlocked(
                    execution_space_id,
                    &run.id,
                    generation,
                    run.runtime_generation,
                )?
            } else {
                false
            }
        } else {
            false
        };
        if (!exact_runtime_generation && !reopened_result_settlement)
            || admission.target_session_id.as_deref() != Some(session.id.as_str())
            || admission.target_runtime_generation != Some(session.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkExecutionBinding does not carry the exact current MemberRun and AgentSession generations",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        self.require_provider_received_work_delivery_unlocked(execution_space_id, &binding)?;
        Ok((run.clone(), binding))
    }

    fn require_exact_released_result_binding_unlocked(
        &self,
        execution_space_id: &str,
        work: &Work,
        actor: &ActorRef,
        work_bindings: &[WorkExecutionBinding],
    ) -> StoreResult<WorkExecutionBinding> {
        let current_runs = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .filter(|run| {
                run.agent_member_id == actor.id
                    && run.team_run_id == work.team_run_id
                    && run.has_live_runtime_authority()
            })
            .collect::<Vec<_>>();
        let [current_run] = current_runs.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "reopened Result settlement requires exactly one current active MemberRun",
                "work",
                &work.id,
                Some(work.version),
            ));
        };
        let mut candidates = Vec::new();
        for binding in work_bindings
            .iter()
            .filter(|binding| binding.status == WorkExecutionBindingStatus::Released)
        {
            let admission = self.work_execution_runtime_binding(execution_space_id, &binding.id)?;
            let exact_close_reopen_lineage = match admission.target_member_run_generation {
                Some(generation) => {
                    self.released_binding_has_exact_member_close_evidence_unlocked(
                        execution_space_id,
                        binding,
                        &current_run.id,
                        generation,
                    )? && self.member_run_has_exact_close_reopen_lineage_unlocked(
                        execution_space_id,
                        &current_run.id,
                        generation,
                        current_run.runtime_generation,
                    )?
                }
                None => false,
            };
            if admission.target_member_run_id.as_deref() != Some(current_run.id.as_str())
                || admission.target_session_id.as_deref() != Some(binding.agent_session_id.as_str())
                || admission.target_runtime_generation != Some(binding.agent_session_generation)
                || !exact_close_reopen_lineage
            {
                continue;
            }
            candidates.push(binding.clone());
        }
        let [binding] = candidates.as_slice() else {
            return Err(trust_error(
                TrustErrorCode::WorkExecutionBindingActive,
                "reopened Result settlement requires exactly one exact released predecessor WorkExecutionBinding",
                "work",
                &work.id,
                Some(work.version),
            ));
        };
        Ok(binding.clone())
    }

    fn released_binding_has_exact_member_close_evidence_unlocked(
        &self,
        execution_space_id: &str,
        binding: &WorkExecutionBinding,
        member_run_id: &str,
        member_run_generation: u64,
    ) -> StoreResult<bool> {
        let releases = self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| {
                envelope.execution_space_id == execution_space_id
                    && envelope.operation.event.aggregate_kind == "work_execution_binding"
                    && envelope.operation.event.aggregate_id == binding.id
                    && envelope.operation.event.transition == "released"
            })
            .collect::<Vec<_>>();
        let [release] = releases.as_slice() else {
            return Ok(false);
        };
        let Some(close_request_id) = release.operation.event.payload["close_request_id"].as_str()
        else {
            return Ok(false);
        };
        let Some(close_runtime_command_id) =
            release.operation.event.payload["close_runtime_command_id"].as_str()
        else {
            return Ok(false);
        };
        let exact_close_request = self
            .latest_team_member_close_request(member_run_id)?
            .is_some_and(|request| {
                request.id == close_request_id
                    && request.status == firm_core::TeamMemberCloseStatus::Applied
            });
        let exact_close_command = self
            .runtime_commands(execution_space_id)?
            .into_iter()
            .find(|command| command.id == close_runtime_command_id)
            .is_some_and(|command| {
                command.command == RuntimeCommandKind::CloseMember
                    && command.binding.target_member_run_id.as_deref() == Some(member_run_id)
                    && command.binding.target_member_run_generation == Some(member_run_generation)
                    && command.binding.target_session_id.as_deref()
                        == Some(binding.agent_session_id.as_str())
                    && command.binding.target_runtime_generation
                        == Some(binding.agent_session_generation)
                    && command
                        .source_record_id
                        .as_deref()
                        .is_some_and(|source| source.starts_with(&format!("{close_request_id}:")))
                    && command.phase == RuntimeCommandPhase::Settled
                    && command.effect_certainty == RuntimeEffectCertainty::Applied
                    && command.postcondition_status == RuntimePostconditionStatus::Satisfied
            });
        Ok(exact_close_request && exact_close_command)
    }

    fn member_run_has_exact_close_reopen_lineage_unlocked(
        &self,
        execution_space_id: &str,
        member_run_id: &str,
        predecessor_generation: u64,
        current_generation: u64,
    ) -> StoreResult<bool> {
        if current_generation != predecessor_generation.saturating_add(1) {
            return Ok(false);
        }
        let history = self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| {
                envelope.execution_space_id == execution_space_id
                    && envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == member_run_id
            })
            .collect::<Vec<_>>();
        for pair in history.windows(2) {
            let closed_event = &pair[0].operation.event;
            let reopened_event = &pair[1].operation.event;
            let formal_close = matches!(
                closed_event.transition.as_str(),
                "closed" | "runtime_projection_synchronized"
            );
            let formal_reopen = reopened_event.transition == "reopened";
            if !formal_close || !formal_reopen {
                continue;
            }
            let closed = event_projection::<MemberRun>(&pair[0])?;
            let reopened = event_projection::<MemberRun>(&pair[1])?;
            if closed.runtime_generation == predecessor_generation
                && closed.coordination_status == MemberCoordinationStatus::Closed
                && closed.runtime_status == MemberRuntimeStatus::Stopped
                && reopened.runtime_generation == current_generation
                && reopened.coordination_status == MemberCoordinationStatus::Active
                && reopened.runtime_status == MemberRuntimeStatus::Queued
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn trust_operation_envelopes_unlocked(
        &self,
    ) -> StoreResult<Vec<TrustOperationEnvelope>> {
        let path = self.root.join(TRUST_OPERATIONS_LEDGER);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(path)?;
        let durable_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut envelopes = Vec::new();
        for row in bytes[..durable_len].split(|byte| *byte == b'\n') {
            if row.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            // A complete malformed frame is corruption and remains fail-closed.
            // Only a non-newline-terminated tail can be the residue of an old
            // append-style crash and is intentionally ignored above.
            envelopes.push(serde_json::from_slice(row)?);
        }
        self.record_jsonl_read(
            TRUST_OPERATIONS_LEDGER,
            bytes.len() as u64,
            bytes.len() as u64,
            durable_len as u64,
            envelopes.len() as u64,
            "full_history",
        );
        Ok(envelopes)
    }

    pub(super) fn write_trust_operation_envelopes_atomic_unlocked(
        &self,
        envelopes: &[TrustOperationEnvelope],
    ) -> StoreResult<()> {
        let path = self.root.join(TRUST_OPERATIONS_LEDGER);
        let next_path = self.root.join("agentfirm_trust_operations.jsonl.next");
        let mut next = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next_path)?;
        for envelope in envelopes {
            serde_json::to_writer(&mut next, envelope)?;
            next.write_all(b"\n")?;
        }
        next.flush()?;
        next.sync_all()?;
        std::fs::rename(&next_path, &path)?;
        std::fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    pub fn canonical_operations(&self) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub fn canonical_execution_space_ids(&self) -> StoreResult<Vec<String>> {
        Ok(self
            .cached_latest_jsonl_derived(
                TRUST_OPERATIONS_LEDGER,
                trust_cache_key,
                |e| e.execution_space_id.clone(),
                crate::store_read_cache::CacheSelection::Groups,
                TrustReadModel::observe,
            )?
            .1)
    }

    /// Scope-preserving canonical operation read for server-built RoleViews.
    /// A physical Store may temporarily contain more than one Execution Space
    /// during recovery/import; callers must never fold another scope's truth.
    ///
    /// Typed like the scoped Work readers, so the boundary is one boundary: a
    /// caller cannot narrow the Work journal by a checked scope and this
    /// journal by a bare string three lines later.
    pub fn canonical_operations_for_space(
        &self,
        execution_space_id: &firm_core::ExecutionSpaceId,
    ) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id.as_str())
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub(crate) fn canonical_host_attention_outbox_unlocked(
        &self,
    ) -> StoreResult<Vec<HostAttention>> {
        self.cached_host_attention_outbox()
    }

    /// Select by kind before cloning: settled RuntimeCommand history must not
    /// inflate every current Session/MemberRun observation.
    pub(super) fn cached_latest_trust_envelopes_for_kind(
        &self,
        aggregate_kind: &str,
    ) -> StoreResult<Vec<TrustOperationEnvelope>> {
        let prefix = trust_cache_prefix(&[aggregate_kind]);
        Ok(self
            .cached_latest_jsonl_derived(
                TRUST_OPERATIONS_LEDGER,
                trust_cache_key,
                |e| e.execution_space_id.clone(),
                crate::store_read_cache::CacheSelection::Prefix(&prefix),
                TrustReadModel::observe,
            )?
            .0)
    }

    pub(super) fn latest_trust_envelopes_unlocked(
        &self,
        execution_space_id: &str,
        aggregate_kind: &str,
    ) -> StoreResult<BTreeMap<String, TrustOperationEnvelope>> {
        let prefix = trust_cache_prefix(&[aggregate_kind, execution_space_id]);
        Ok(self
            .cached_latest_jsonl_derived(
                TRUST_OPERATIONS_LEDGER,
                trust_cache_key,
                |e| e.execution_space_id.clone(),
                crate::store_read_cache::CacheSelection::Prefix(&prefix),
                TrustReadModel::observe,
            )?
            .0
            .into_iter()
            .map(|e| (e.operation.event.aggregate_id.clone(), e))
            .collect())
    }

    pub(super) fn replay_trust_projection_unlocked<T: for<'de> Deserialize<'de> + Clone>(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        fingerprint: &str,
    ) -> StoreResult<Option<CanonicalMutationResult<T>>> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        Self::replay_trust_projection_in(
            &existing,
            context,
            aggregate_kind,
            aggregate_id,
            fingerprint,
        )
    }

    /// The same replay check against envelopes the caller already holds.
    ///
    /// The whole journal is read uncached, so a caller that has just read it
    /// must not pay for a second full parse to ask this question — on a real
    /// store that is megabytes per Work write. In a batch it is also the more
    /// correct form: entries staged earlier in the same write are visible to
    /// the ones after them.
    fn replay_trust_projection_in<T: for<'de> Deserialize<'de> + Clone>(
        existing: &[TrustOperationEnvelope],
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        fingerprint: &str,
    ) -> StoreResult<Option<CanonicalMutationResult<T>>> {
        let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) else {
            return Ok(None);
        };
        if replay.operation.event.canonical_request_fingerprint != fingerprint {
            return Err(trust_error(
                TrustErrorCode::IdempotencyKeyReused,
                "idempotency key was already used with a different canonical payload",
                aggregate_kind,
                aggregate_id,
                Some(replay.operation.event.resulting_version),
            ));
        }
        if replay.operation.event.aggregate_kind != aggregate_kind
            || replay.operation.event.aggregate_id != aggregate_id
        {
            return Err(trust_error(
                TrustErrorCode::IdempotencyKeyReused,
                "idempotent replay changed aggregate identity",
                aggregate_kind,
                aggregate_id,
                None,
            ));
        }
        Ok(Some(CanonicalMutationResult {
            projection: event_projection(replay)?,
            event: replay.operation.event.clone(),
            replayed: true,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn commit_trust_projection_unlocked<
        T: Serialize + for<'de> Deserialize<'de> + Clone,
    >(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        transition: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
        initial_outbox_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<T>> {
        self.commit_trust_projection_with_paired_aggregate_unlocked(
            context,
            aggregate_kind,
            aggregate_id,
            transition,
            request_payload,
            resulting_projection,
            immutable_side_records,
            initial_outbox_records,
            None,
        )
    }

    /// Commit one canonical projection and, atomically with it, a second
    /// canonical aggregate that belongs to the same decision.
    ///
    /// A Result submission advances its Work to Review as a side effect of
    /// publishing an immutable report. Before W3 that Work revision existed
    /// only as an immutable side record of the `work_report` envelope, so the
    /// Work journal could not name the transition that produced it and
    /// `WorkEventKind::Submitted` had no writer at all. The paired `work`
    /// envelope is written in the SAME atomic ledger rewrite as the report, so
    /// a crash can never leave one without the other, and it carries the
    /// WorkEvent the ledger would have carried.
    ///
    /// The paired envelope derives its idempotency key from the caller's, so
    /// every existing scan that resolves a command by its exact key still
    /// finds exactly the primary envelope; an exact replay returns at the
    /// primary's replay check above and re-appends neither.
    ///
    /// The second use is the provider-native session pointer: the
    /// `agent_session` that owns it and the `member_run` that projects it are
    /// written in one rewrite, so a projection can never be observed without
    /// its authority (ADR 0071).
    ///
    /// Passing `None` takes the single-aggregate path, which is unchanged: it
    /// appends exactly one envelope, byte-for-byte as before.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::trust_kernel) fn commit_trust_projection_with_paired_aggregate_unlocked<
        T: Serialize + for<'de> Deserialize<'de> + Clone,
    >(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        transition: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
        initial_outbox_records: Vec<Value>,
        paired: Option<PairedAggregateTransition>,
    ) -> StoreResult<CanonicalMutationResult<T>> {
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        required(aggregate_kind, "aggregate_kind")?;
        required(aggregate_id, "aggregate_id")?;
        require_unreserved_idempotency_key(&context.idempotency_key)?;
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        // Against the envelopes just read; asking through the re-reading form
        // would parse the whole journal a second time.
        if let Some(replay) = Self::replay_trust_projection_in(
            &existing,
            context,
            aggregate_kind,
            aggregate_id,
            &fingerprint,
        )? {
            return Ok(replay);
        }

        let latest = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == aggregate_kind
                    && envelope.operation.event.aggregate_id == aggregate_id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let current_version = latest
            .map(|envelope| envelope.operation.event.resulting_version)
            .unwrap_or(0);
        if context.expected_version != current_version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                format!(
                    "expected version {}, current version is {current_version}",
                    context.expected_version
                ),
                aggregate_kind,
                aggregate_id,
                Some(current_version),
            ));
        }
        let store_sequence = existing
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let resulting_version = current_version + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: aggregate_kind.to_string(),
            aggregate_id: aggregate_id.to_string(),
            sequence: latest
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: transition.to_string(),
            expected_version: current_version,
            resulting_version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(resulting_projection)?,
            immutable_side_records,
            initial_outbox_records,
        };
        let mut committed = existing;
        let envelope = |operation| TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        };
        if let Some(paired) = paired {
            // One builder for both arms: a paired envelope is constructed
            // identically whichever aggregate it carries.
            let (
                paired_kind,
                paired_id,
                paired_transition,
                paired_expected,
                paired_resulting,
                paired_projection,
            ) = match &paired {
                PairedAggregateTransition::Work(work) => (
                    "work",
                    work.work.id.clone(),
                    work.transition,
                    work.expected_version,
                    work.work.version,
                    serde_json::to_value(&work.work)?,
                ),
                PairedAggregateTransition::MemberRun(run) => (
                    "member_run",
                    run.run.id.clone(),
                    run.transition,
                    run.expected_version,
                    run.run.version,
                    serde_json::to_value(&run.run)?,
                ),
            };
            let previous = committed
                .iter()
                .filter(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.operation.event.aggregate_kind == paired_kind
                        && envelope.operation.event.aggregate_id == paired_id
                })
                .max_by_key(|envelope| envelope.operation.event.sequence);
            let paired_store_sequence = store_sequence + 1;
            let paired_event = CanonicalMutationEvent {
                id: format!("trust-event-{paired_store_sequence}"),
                aggregate_kind: paired_kind.into(),
                aggregate_id: paired_id,
                sequence: previous
                    .map(|envelope| envelope.operation.event.sequence)
                    .unwrap_or(0)
                    + 1,
                store_sequence: paired_store_sequence,
                transition: paired_transition.to_string(),
                expected_version: paired_expected,
                resulting_version: paired_resulting,
                performed_by_actor: event.performed_by_actor.clone(),
                authority_actor: event.authority_actor.clone(),
                causation_ref: None,
                idempotency_key: format!(
                    "{}{PAIRED_KEY_SEPARATOR}{}",
                    context.idempotency_key, paired_transition
                ),
                canonical_request_fingerprint: event.canonical_request_fingerprint.clone(),
                payload: event.payload.clone(),
                created_at: event.created_at.clone(),
            };
            let mut paired_operation = CanonicalOperation {
                event: paired_event,
                resulting_projection: paired_projection,
                immutable_side_records: Vec::new(),
                initial_outbox_records: Vec::new(),
            };
            if let PairedAggregateTransition::Work(work) = &paired {
                // The committed WorkEvent is built by the same function the
                // reader uses for pre-W3 rows, so a store written before and
                // after this slice reports one identical Work event for this
                // revision. A paired `member_run` projection carries no side
                // record: it is a projection of the primary aggregate, not a
                // journalled transition of its own.
                paired_operation.immutable_side_records =
                    vec![serde_json::to_value(read_model::derived_work_event(
                        &paired_operation,
                        &work.work,
                        work.kind,
                        work.expected_version,
                    ))?];
            }
            committed.push(envelope(operation));
            committed.push(envelope(paired_operation));
        } else {
            committed.push(envelope(operation));
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(CanonicalMutationResult {
            projection: resulting_projection.clone(),
            event,
            replayed: false,
        })
    }

    pub(super) fn commit_trust_work_acceptance_unlocked(
        &self,
        context: &MutationContext,
        request_payload: Value,
        work: &Work,
        immutable_side_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.commit_current_work_mutation_unlocked(
            context,
            "accepted",
            request_payload,
            work,
            immutable_side_records,
            Vec::new(),
        )
    }
}

// Only volatile index keys use this order. Persisted records are unchanged.
fn trust_cache_key(e: &TrustOperationEnvelope) -> String {
    serde_json::to_string(&(
        &e.operation.event.aggregate_kind,
        &e.execution_space_id,
        &e.operation.event.aggregate_id,
    ))
    .expect("string tuple serializes")
}
fn trust_cache_prefix(parts: &[&str]) -> String {
    let mut prefix = serde_json::to_string(parts).expect("string array serializes");
    prefix.pop(); // Closing array delimiter, after fully escaped string values.
    prefix.push(',');
    prefix
}
