use super::*;

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
    /// history and current trust operations. New current mutations must use
    /// this seam rather than appending another legacy WorkOperation writer.
    pub(crate) fn commit_current_work_mutation_unlocked(
        &self,
        context: &MutationContext,
        transition: &str,
        request_payload: Value,
        work: &Work,
        immutable_side_records: Vec<Value>,
        mut initial_outbox_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        work.validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        if work.version != context.expected_version.saturating_add(1) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "canonical Work mutation must advance the exact expected revision once",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) =
            self.replay_current_work_mutation_unlocked(context, &work.id, &fingerprint)?
        {
            return Ok(replay);
        }
        let previous = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "work"
                    && envelope.operation.event.aggregate_id == work.id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let store_sequence = existing
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
            transition: transition.into(),
            expected_version: context.expected_version,
            resulting_version: work.version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        initial_outbox_records.extend(
            self.canonical_terminal_work_outbox_unlocked(work, &event)?
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(work)?,
            immutable_side_records,
            initial_outbox_records,
        };
        let mut committed = existing;
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        });
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
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
                    && command.status == RuntimeCommandStatus::Applied
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
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.execution_space_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    /// Scope-preserving canonical operation read for server-built RoleViews.
    /// A physical Store may temporarily contain more than one Execution Space
    /// during recovery/import; callers must never fold another scope's truth.
    pub fn canonical_operations_for_space(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub(crate) fn trust_work_projections_unlocked(&self) -> StoreResult<Vec<Work>> {
        let mut works = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "work" {
                works.push(event_projection::<Work>(&envelope)?);
            }
            for record in envelope.operation.immutable_side_records {
                if let Ok(work) = serde_json::from_value::<Work>(record) {
                    works.push(work);
                }
            }
        }
        Ok(works)
    }

    pub(crate) fn canonical_host_attention_outbox_unlocked(
        &self,
    ) -> StoreResult<Vec<HostAttention>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .flat_map(|envelope| envelope.operation.initial_outbox_records)
            .filter_map(|record| serde_json::from_value::<HostAttention>(record).ok())
            .collect())
    }

    /// Decode-only compatibility view for callers that still resolve a
    /// historical/native WorkEvent reference. Current Work state is never
    /// reconstructed from these records; the canonical operation projection
    /// remains the sole writer and authority.
    pub(crate) fn trust_work_events_unlocked(&self) -> StoreResult<Vec<firm_core::WorkEvent>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.operation.event.aggregate_kind == "work")
            .flat_map(|envelope| envelope.operation.immutable_side_records)
            .filter_map(|record| serde_json::from_value::<firm_core::WorkEvent>(record).ok())
            .collect())
    }

    pub(crate) fn trust_work_events_for_ids_unlocked(
        &self,
        work_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<firm_core::WorkEvent>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| {
                envelope.operation.event.aggregate_kind == "work"
                    && work_ids.contains(&envelope.operation.event.aggregate_id)
            })
            .flat_map(|envelope| envelope.operation.immutable_side_records)
            .filter(|record| {
                record["work_id"]
                    .as_str()
                    .is_some_and(|id| work_ids.contains(id))
            })
            .filter_map(|record| serde_json::from_value::<firm_core::WorkEvent>(record).ok())
            .collect())
    }

    pub(crate) fn trust_work_delegation_revisions_for_team_run_unlocked(
        &self,
        team_run_id: Option<&str>,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let mut revisions = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            for record in envelope.operation.immutable_side_records {
                let belongs_to_run = team_run_id.is_none_or(|id| {
                    record["delegation"]["source_work_ref"]["team_run_id"].as_str() == Some(id)
                        || record["delegation"]["target_work_ref"]["team_run_id"].as_str()
                            == Some(id)
                });
                if !belongs_to_run {
                    continue;
                }
                if let Ok(revision) = serde_json::from_value::<WorkDelegationRevision>(record) {
                    revisions.push(revision);
                }
            }
        }
        Ok(revisions)
    }

    pub(super) fn latest_trust_envelopes_unlocked(
        &self,
        execution_space_id: &str,
        aggregate_kind: &str,
    ) -> StoreResult<BTreeMap<String, TrustOperationEnvelope>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id == execution_space_id
                && envelope.operation.event.aggregate_kind == aggregate_kind
            {
                latest.insert(envelope.operation.event.aggregate_id.clone(), envelope);
            }
        }
        Ok(latest)
    }

    pub(super) fn replay_trust_projection_unlocked<T: for<'de> Deserialize<'de> + Clone>(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        fingerprint: &str,
    ) -> StoreResult<Option<CanonicalMutationResult<T>>> {
        let existing = self.trust_operation_envelopes_unlocked()?;
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
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        required(aggregate_kind, "aggregate_kind")?;
        required(aggregate_id, "aggregate_id")?;
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = self.replay_trust_projection_unlocked(
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
        committed.push(TrustOperationEnvelope {
            execution_space_id: context.execution_space_id.clone(),
            authenticated_actor_kind: context.authenticated_actor.kind,
            authenticated_actor_id: context.authenticated_actor.id.clone(),
            command_name: context.command_name.clone(),
            operation,
        });
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
