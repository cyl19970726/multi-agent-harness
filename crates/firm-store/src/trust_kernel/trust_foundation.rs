use super::*;

impl HarnessStore {
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

    pub(super) fn trust_work_team_run_unlocked(&self, work_id: &str) -> StoreResult<String> {
        self.latest_works_unlocked()?
            .remove(work_id)
            .map(|work| work.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "WorkDelivery references a missing Work",
                    "work",
                    work_id,
                    None,
                )
            })
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
    ) -> StoreResult<MemberRun> {
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
        let active_member_run_id = work.active_member_run_id.as_deref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::UnauthorizedActor,
                "member-owned Work mutation requires an active WorkExecutionBinding",
                "work",
                &work.id,
                Some(work.version),
            )
        })?;
        let run = self
            .latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .remove(active_member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "WorkExecutionBinding references a missing MemberRun",
                    "work",
                    &work.id,
                    Some(work.version),
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        if run.agent_member_id != actor.id
            || run.team_run_id != work.team_run_id
            || run.coordination_status != MemberCoordinationStatus::Active
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkExecutionBinding is not the authenticated Member's exact active MemberRun",
                "work",
                &work.id,
                Some(work.version),
            ));
        }
        Ok(run)
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

    pub(crate) fn trust_work_delegation_revisions_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let mut revisions = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            for record in envelope.operation.immutable_side_records {
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
    pub(super) fn commit_trust_projection_unlocked<
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
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&request_payload));
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work.id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    &work.id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
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
            transition: "accepted".into(),
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
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(work)?,
            immutable_side_records,
            initial_outbox_records: Vec::new(),
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
}
