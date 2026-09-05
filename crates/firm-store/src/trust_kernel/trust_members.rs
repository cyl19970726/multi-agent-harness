use super::*;

impl HarnessStore {
    pub fn trust_agent_members(&self, execution_space_id: &str) -> StoreResult<Vec<AgentMember>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_member")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_agent_members_for_ids(
        &self,
        execution_space_id: &str,
        member_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<AgentMember>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = &envelope.operation.event;
            if envelope.execution_space_id == execution_space_id
                && event.aggregate_kind == "agent_member"
                && member_ids.contains(&event.aggregate_id)
            {
                latest.insert(event.aggregate_id.clone(), envelope);
            }
        }
        latest.values().map(event_projection).collect()
    }

    /// Company/read-model projection only. One HarnessStore is one Execution
    /// Space in normal operation; this fold exists for callers that were given
    /// only the physical store and must not resurrect a second identity ledger.
    pub fn all_trust_agent_members(&self) -> StoreResult<Vec<AgentMember>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "agent_member" {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        envelope.operation.event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest.values().map(event_projection).collect()
    }

    pub fn create_trust_agent_member(
        &self,
        context: &MutationContext,
        mut member: AgentMember,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(&member.id, "AgentMember.id")?;
        required(&member.name, "AgentMember.name")?;
        required(&member.role, "AgentMember.role")?;
        required(&member.workspace_policy, "AgentMember.workspace_policy")?;
        if member.version != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentMember create requires absent CAS and version 1",
                "agent_member",
                &member.id,
                Some(0),
            ));
        }
        if member.created_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "created_by must equal the authenticated actor",
                "agent_member",
                &member.id,
                None,
            ));
        }
        member.updated_at = member.created_at.clone();
        let payload = serde_json::to_value(&member)?;
        self.commit_trust_projection_unlocked(
            context,
            "agent_member",
            &member.id,
            "created",
            payload,
            &member,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_trust_agent_member(
        &self,
        context: &MutationContext,
        member_id: &str,
        next_status: AgentMemberOrganizationStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut current = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_member")?
            .remove(member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentMember not found",
                    "agent_member",
                    member_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentMember>(&envelope))?;
        let allowed = matches!(
            (current.organization_status, next_status),
            (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Paused
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Active
            ) | (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Retired
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Retired
            )
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentMember transition is not allowed",
                "agent_member",
                member_id,
                Some(current.version),
            ));
        }
        current.organization_status = next_status;
        current.version += 1;
        current.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "agent_member",
            member_id,
            match next_status {
                AgentMemberOrganizationStatus::Active => "resumed",
                AgentMemberOrganizationStatus::Paused => "paused",
                AgentMemberOrganizationStatus::Retired => "retired",
            },
            serde_json::json!({"status": next_status, "updated_at": updated_at}),
            &current,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_member_runs(&self, execution_space_id: &str) -> StoreResult<Vec<MemberRun>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_member_runs_for_team_run(
        &self,
        execution_space_id: &str,
        team_run_id: &str,
    ) -> StoreResult<Vec<MemberRun>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = &envelope.operation.event;
            if envelope.execution_space_id == execution_space_id
                && event.aggregate_kind == "member_run"
                && envelope.operation.resulting_projection["team_run_id"].as_str()
                    == Some(team_run_id)
            {
                latest.insert(event.aggregate_id.clone(), envelope);
            }
        }
        latest.values().map(event_projection).collect()
    }

    pub(crate) fn trust_member_runs_for_ids_all_scopes(
        &self,
        member_run_ids: &std::collections::BTreeSet<String>,
    ) -> StoreResult<Vec<(String, MemberRun)>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = &envelope.operation.event;
            if event.aggregate_kind == "member_run" && member_run_ids.contains(&event.aggregate_id)
            {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest
            .into_iter()
            .map(|((scope, _), envelope)| event_projection(&envelope).map(|run| (scope, run)))
            .collect()
    }

    pub fn trust_member_run_scope(&self, member_run_id: &str) -> StoreResult<Option<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .rev()
            .find(|envelope| {
                envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == member_run_id
            })
            .map(|envelope| envelope.execution_space_id))
    }

    pub(super) fn validate_trust_member_run_authority_unlocked(
        &self,
        context: &MutationContext,
        run: &MemberRun,
        team_run: &firm_core::AgentTeamRun,
    ) -> StoreResult<()> {
        required(&run.id, "MemberRun.id")?;
        required(&run.agent_member_id, "MemberRun.agent_member_id")?;
        required(&run.team_run_id, "MemberRun.team_run_id")?;
        if run.version != 1 || run.runtime_generation != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "MemberRun create requires absent CAS, version 1 and generation 1",
                "member_run",
                &run.id,
                Some(0),
            ));
        }
        if run.team_run_id != team_run.id {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun does not belong to the admitted TeamRun",
                "member_run",
                &run.id,
                None,
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing AgentMember in the selected Execution Space",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => {}
            AgentMemberOrganizationStatus::Paused => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberPaused,
                    "paused AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ));
            }
            AgentMemberOrganizationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberRetired,
                    "retired AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ));
            }
        }
        let team = self
            .latest_teams()?
            .remove(&team_run.agent_team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamRun references a missing AgentTeam",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        let exact_membership = self
            .fabric_team_memberships(&context.execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.agent_member_id == run.agent_member_id
                    && membership.state == TeamMembershipStatus::Active
            })
            .count();
        if team.status != AgentTeamStatus::Active || exact_membership != 1 {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "MemberRun requires one exact active durable TeamMembership on an Active Team",
                "member_run",
                &run.id,
                None,
            ));
        }
        Ok(())
    }

    /// Validate every proposed canonical MemberRun against one frozen TeamRun
    /// and exact Execution Space while the caller holds the Store write lock.
    /// This is deliberately stricter than idempotent standalone create: current
    /// admission must materialize new, absent canonical rows.
    pub(crate) fn validate_new_trust_member_runs_unlocked(
        &self,
        execution_space_id: &str,
        team_run: &firm_core::AgentTeamRun,
        admissions: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<()> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        let mut proposed_ids = BTreeSet::new();
        let mut proposed_idempotency = BTreeSet::new();
        for admission in admissions {
            let context = &admission.context;
            let run = &admission.run;
            if context.execution_space_id != execution_space_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun admission changed Execution Space",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            self.validate_trust_member_run_authority_unlocked(context, run, team_run)?;
            if !proposed_ids.insert(run.id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::VersionConflict,
                    "MemberRun admission contains a duplicate id",
                    "member_run",
                    &run.id,
                    Some(0),
                ));
            }
            let idempotency_identity = (
                context.execution_space_id.clone(),
                context.authenticated_actor.kind,
                context.authenticated_actor.id.clone(),
                context.command_name.clone(),
                context.idempotency_key.clone(),
            );
            if !proposed_idempotency.insert(idempotency_identity.clone()) {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MemberRun admission contains a duplicate idempotency key",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            if existing.iter().any(|envelope| {
                envelope.execution_space_id == execution_space_id
                    && envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == run.id
            }) {
                return Err(trust_error(
                    TrustErrorCode::VersionConflict,
                    "MemberRun already exists in the selected Execution Space",
                    "member_run",
                    &run.id,
                    Some(1),
                ));
            }
            if existing.iter().any(|envelope| {
                envelope.execution_space_id == idempotency_identity.0
                    && envelope.authenticated_actor_kind == idempotency_identity.1
                    && envelope.authenticated_actor_id == idempotency_identity.2
                    && envelope.command_name == idempotency_identity.3
                    && envelope.operation.event.idempotency_key == idempotency_identity.4
            }) {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "MemberRun admission idempotency key already exists",
                    "member_run",
                    &run.id,
                    None,
                ));
            }
            // Prove the complete canonical payload is serializable before the
            // caller performs the first legacy-ledger append.
            serde_json::to_value(run)?;
        }
        Ok(())
    }

    /// Commit a previously validated set of new MemberRuns in one atomic
    /// replacement of the canonical trust ledger. The caller must retain the
    /// Store write lock from validation through this call.
    pub(crate) fn commit_new_trust_member_runs_unlocked(
        &self,
        admissions: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<Vec<CanonicalMutationResult<MemberRun>>> {
        let mut committed = self.trust_operation_envelopes_unlocked()?;
        let first_store_sequence = committed
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let mut results = Vec::with_capacity(admissions.len());
        for (next_store_sequence, admission) in (first_store_sequence..).zip(admissions) {
            let context = &admission.context;
            let run = &admission.run;
            let payload = serde_json::to_value(run)?;
            let fingerprint = context
                .request_fingerprint
                .clone()
                .unwrap_or_else(|| canonical_json_fingerprint(&payload));
            let event = CanonicalMutationEvent {
                id: format!("trust-event-{next_store_sequence}"),
                aggregate_kind: "member_run".to_string(),
                aggregate_id: run.id.clone(),
                sequence: 1,
                store_sequence: next_store_sequence,
                transition: "created".to_string(),
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.authenticated_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: None,
                idempotency_key: context.idempotency_key.clone(),
                canonical_request_fingerprint: fingerprint,
                payload,
                created_at: now_string(),
            };
            let operation = CanonicalOperation {
                event: event.clone(),
                resulting_projection: serde_json::to_value(run)?,
                immutable_side_records: Vec::new(),
                initial_outbox_records: Vec::new(),
            };
            committed.push(TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: context.command_name.clone(),
                operation,
            });
            results.push(CanonicalMutationResult {
                projection: run.clone(),
                event,
                replayed: false,
            });
        }
        self.write_trust_operation_envelopes_atomic_unlocked(&committed)?;
        Ok(results)
    }

    pub(crate) fn prepare_current_member_runtime_sync_unlocked(
        &self,
        execution_space_id: &str,
        runtime: &ProviderRuntimeProjection,
    ) -> StoreResult<Option<PreparedCurrentMemberSync>> {
        self.prepare_current_member_runtime_sync_with_generation_unlocked(
            execution_space_id,
            runtime,
            false,
        )
    }

    pub(super) fn prepare_current_member_runtime_sync_with_generation_unlocked(
        &self,
        execution_space_id: &str,
        runtime: &ProviderRuntimeProjection,
        allow_reopen_generation_advance: bool,
    ) -> StoreResult<Option<PreparedCurrentMemberSync>> {
        let envelope = self
            .latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .remove(&runtime.id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but no canonical MemberRun exists",
                    runtime.team_run_id, runtime.id
                ))
            })?;
        let current = event_projection::<MemberRun>(&envelope)?;
        let generation_matches = current.runtime_generation == runtime.runtime_generation
            || (allow_reopen_generation_advance
                && current.coordination_status == MemberCoordinationStatus::Closed
                && runtime.coordination_status == LegacyMemberCoordinationStatus::Active
                && runtime.runtime_generation == current.runtime_generation.saturating_add(1));
        if current.team_run_id != runtime.team_run_id
            || current.agent_member_id != runtime.agent_member_id
            || current.role_snapshot != runtime.role
            || !generation_matches
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} cannot synchronize a mismatched canonical projection in Execution Space {}",
                runtime.team_run_id, runtime.id, execution_space_id
            )));
        }
        if current_member_lifecycle_matches(&current, runtime)? {
            return Ok(None);
        }
        let mut next = current.clone();
        next.coordination_status = canonical_coordination_status(runtime.coordination_status);
        next.runtime_status = canonical_runtime_status(runtime.status);
        next.runtime_generation = runtime.runtime_generation;
        next.native_session = runtime
            .native_session
            .as_ref()
            .map(canonical_native_session)
            .transpose()?;
        next.started_at = runtime.started_at.clone();
        next.last_event_at = runtime.last_event_at.clone();
        next.finished_at = runtime.finished_at.clone();
        next.version = current.version.saturating_add(1);
        serde_json::to_value(&next)?;
        Ok(Some(PreparedCurrentMemberSync {
            context: MutationContext {
                execution_space_id: execution_space_id.to_string(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: "node-daemon:member-projection-sync".to_string(),
                },
                authority_actor: Some(ActorRef {
                    kind: ActorKind::AgentMember,
                    id: runtime.agent_member_id.clone(),
                }),
                command_name: "team_run.member_projection.sync".to_string(),
                idempotency_key: format!("team-run-member-sync:{}:{}", runtime.id, next.version),
                expected_version: current.version,
                request_fingerprint: None,
            },
            projection: next,
            transition: "runtime_projection_synchronized",
            side_records: Vec::new(),
        }))
    }

    pub(crate) fn commit_prepared_current_member_sync_unlocked(
        &self,
        prepared: PreparedCurrentMemberSync,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.commit_trust_projection_unlocked(
            &prepared.context,
            "member_run",
            &prepared.projection.id,
            prepared.transition,
            current_member_sync_payload(&prepared.projection),
            &prepared.projection,
            prepared.side_records,
            Vec::new(),
        )
    }

    /// Explicit reconstruction seam for Legacy/import tests. Current Team
    /// Member admission must use the combined TeamRun admission APIs.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn legacy_import_create_trust_member_run_projection(
        &self,
        context: &MutationContext,
        run: MemberRun,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == run.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing TeamRun",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        self.validate_trust_member_run_authority_unlocked(context, &run, &team_run)?;
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            &run.id,
            "created",
            serde_json::to_value(&run)?,
            &run,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_current_team_member_lifecycle(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        transition: CurrentTeamMemberLifecycleTransition,
        updated_at: &str,
    ) -> StoreResult<CurrentTeamMemberLifecycleResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun not found",
                "member_run",
                member_run_id,
                None,
            )
        })?;
        let team_run = latest_by_id(
            self.read_jsonl::<firm_core::AgentTeamRun>("team_runs.jsonl")?,
            |run| run.id.clone(),
        )
        .remove(&current.team_run_id)
        .ok_or_else(|| {
            trust_error(
                TrustErrorCode::InvalidStateTransition,
                "MemberRun references a missing TeamRun",
                "member_run",
                member_run_id,
                None,
            )
        })?;
        let execution_space_id = self.current_team_run_execution_space_unlocked(&team_run)?;
        if execution_space_id != context.execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {} belongs to Execution Space {}, not {}",
                team_run.id, execution_space_id, context.execution_space_id
            )));
        }
        let canonical_current = self
            .latest_trust_envelopes_unlocked(&execution_space_id, "member_run")?
            .remove(member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun not found",
                    "member_run",
                    member_run_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        if !current_member_lifecycle_validation_mismatch_fields(&canonical_current, &current)?
            .is_empty()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} lifecycle projections diverge",
                team_run.id, member_run_id
            )));
        }

        let already_at_requested_result = match transition {
            CurrentTeamMemberLifecycleTransition::Close => {
                current.coordination_status == LegacyMemberCoordinationStatus::Closed
                    && current.status == MemberRunStatus::Stopped
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.as_deref() == Some(updated_at)
            }
            CurrentTeamMemberLifecycleTransition::Retire => {
                current.coordination_status == LegacyMemberCoordinationStatus::Retired
                    && current.status == MemberRunStatus::Stopped
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.as_deref() == Some(updated_at)
            }
            CurrentTeamMemberLifecycleTransition::Reopen => {
                current.coordination_status == LegacyMemberCoordinationStatus::Active
                    && current.status == MemberRunStatus::Queued
                    && current.started_at == updated_at
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.is_none()
            }
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession => {
                current.coordination_status == LegacyMemberCoordinationStatus::Active
                    && current.status == MemberRunStatus::Starting
                    && current.last_event_at.as_deref() == Some(updated_at)
                    && current.finished_at.is_none()
            }
        };
        if already_at_requested_result {
            let payload = current_member_sync_payload(&canonical_current);
            let fingerprint = context
                .request_fingerprint
                .clone()
                .unwrap_or_else(|| canonical_json_fingerprint(&payload));
            if let Some(replay) = self.replay_trust_projection_unlocked(
                context,
                "member_run",
                member_run_id,
                &fingerprint,
            )? {
                return Ok(CurrentTeamMemberLifecycleResult {
                    runtime_projection: current,
                    canonical: replay,
                });
            }
        }

        let mut next = current.clone();
        let transition_name = match transition {
            CurrentTeamMemberLifecycleTransition::Close => {
                if !current.coordination_is_active() {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Close requires an active MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.coordination_status = LegacyMemberCoordinationStatus::Closed;
                next.status = MemberRunStatus::Stopped;
                next.finished_at = Some(updated_at.to_string());
                "closed"
            }
            CurrentTeamMemberLifecycleTransition::Retire => {
                if current.coordination_is_retired() {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "MemberRun is already retired",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.coordination_status = LegacyMemberCoordinationStatus::Retired;
                next.status = MemberRunStatus::Stopped;
                next.finished_at = Some(updated_at.to_string());
                "retired"
            }
            CurrentTeamMemberLifecycleTransition::Reopen => {
                if current.coordination_status != LegacyMemberCoordinationStatus::Closed {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Reopen requires a closed MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                let session = current.native_session.as_ref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::NativeSessionMissing,
                        "reopen requires a resumable NativeSessionRef",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    )
                })?;
                if !session.supports_resume
                    || matches!(
                        session.availability,
                        firm_core::NativeSessionAvailability::Missing
                            | firm_core::NativeSessionAvailability::Incompatible
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::NativeSessionIncompatible,
                        "NativeSessionRef is not safely resumable",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                next.runtime_generation = current.runtime_generation.saturating_add(1);
                next.coordination_status = LegacyMemberCoordinationStatus::Active;
                next.status = MemberRunStatus::Queued;
                next.started_at = updated_at.to_string();
                next.finished_at = None;
                "reopened"
            }
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession => {
                if current.coordination_status != LegacyMemberCoordinationStatus::Active
                    || !matches!(
                        current.status,
                        MemberRunStatus::Disconnected
                            | MemberRunStatus::Failed
                            | MemberRunStatus::Stopped
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "Resume native session requires an active, disconnected, failed, or stopped MemberRun",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                let session = current.native_session.as_ref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::NativeSessionMissing,
                        "resume native session requires a resumable NativeSessionRef",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    )
                })?;
                if !session.supports_resume
                    || matches!(
                        session.availability,
                        firm_core::NativeSessionAvailability::Missing
                            | firm_core::NativeSessionAvailability::Incompatible
                    )
                {
                    return Err(trust_error(
                        TrustErrorCode::NativeSessionIncompatible,
                        "NativeSessionRef is not safely resumable",
                        "member_run",
                        member_run_id,
                        Some(canonical_current.version),
                    ));
                }
                // Resuming a still-active MemberRun reattaches its exact
                // frozen provider-native session. It is not a coordination
                // reopen and therefore does not mint a new runtime
                // generation.
                next.status = MemberRunStatus::Starting;
                next.finished_at = None;
                "native_session_resume_requested"
            }
        };
        next.last_event_at = Some(updated_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let allow_reopen_generation_advance =
            transition == CurrentTeamMemberLifecycleTransition::Reopen;
        let mut prepared = self
            .prepare_current_member_runtime_sync_with_generation_unlocked(
                &execution_space_id,
                &next,
                allow_reopen_generation_advance,
            )?
            .ok_or_else(|| {
                StoreError::Conflict("MemberRun lifecycle transition made no change".to_string())
            })?;
        prepared.context = context.clone();
        prepared.transition = transition_name;
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        let payload = current_member_sync_payload(&prepared.projection);
        let fingerprint = context
            .request_fingerprint
            .clone()
            .unwrap_or_else(|| canonical_json_fingerprint(&payload));
        if self
            .replay_trust_projection_unlocked::<MemberRun>(
                context,
                "member_run",
                member_run_id,
                &fingerprint,
            )?
            .is_some()
        {
            return Err(StoreError::Conflict(
                "MEMBER_RUN_IDEMPOTENT_REPLAY_STATE_MISMATCH: prior lifecycle operation exists but current runtime projection does not match its result"
                    .to_string(),
            ));
        }
        if context.expected_version != canonical_current.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                format!(
                    "expected version {}, current version is {}",
                    context.expected_version, canonical_current.version
                ),
                "member_run",
                member_run_id,
                Some(canonical_current.version),
            ));
        }
        serde_json::to_value(&next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        let canonical = self.commit_prepared_current_member_sync_unlocked(prepared)?;
        Ok(CurrentTeamMemberLifecycleResult {
            runtime_projection: next,
            canonical,
        })
    }

    /// Advance one current provider runtime generation while keeping the
    /// canonical MemberRun and its runtime projection coherent under the same
    /// Store write lock. Generic projection CAS deliberately cannot change a
    /// generation: reopen/recovery must use this combined authority boundary.
    ///
    /// The two physical ledgers are validated and serialized before the first
    /// write. They are not backed by a cross-file crash journal; a storage
    /// failure between the Legacy JSONL append and canonical atomic replace is
    /// therefore detected as an incomplete current TeamRun on restart and
    /// fails closed rather than being silently repaired.
    pub fn compare_and_advance_member_run_generation(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.compare_and_advance_member_run_generation_with_host_mode(expected, next, None, None)
    }

    /// Advance one exact MemberRun generation as an explicit coordination
    /// Reopen. Generic Supervisor recovery must use
    /// `compare_and_advance_member_run_generation` and therefore cannot emit
    /// the canonical `reopened` evidence consumed by Result settlement.
    pub fn compare_and_reopen_member_run_generation(
        &self,
        reopened_by: &firm_core::TeamActorRef,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.compare_and_advance_member_run_generation_with_host_mode(
            expected,
            next,
            None,
            Some(reopened_by),
        )
    }

    /// Reopen the exact Host MemberRun into a different control mode while
    /// atomically advancing its runtime generation and the TeamRun mode. This
    /// is the sole write boundary for managed ↔ external_interactive changes;
    /// ordinary MemberRuns and live Host generations cannot use it.
    pub fn compare_and_transition_host_mode(
        &self,
        reopened_by: &firm_core::TeamActorRef,
        expected_run: &firm_core::AgentTeamRun,
        next_run: &firm_core::AgentTeamRun,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.compare_and_advance_member_run_generation_with_host_mode(
            expected,
            next,
            Some((expected_run, next_run)),
            Some(reopened_by),
        )
    }

    fn compare_and_advance_member_run_generation_with_host_mode(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
        host_mode_transition: Option<(&firm_core::AgentTeamRun, &firm_core::AgentTeamRun)>,
        formal_reopen_actor: Option<&firm_core::TeamActorRef>,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the generation transition",
                expected.id
            )));
        }
        if let Some(reopened_by) = formal_reopen_actor {
            self.require_exact_team_run_host_actor(reopened_by, &current.team_run_id)?;
            let expected_closed =
                current.coordination_is_closed() && current.status == MemberRunStatus::Stopped;
            let next_reopened = next.coordination_is_active()
                && if next.is_external_interactive() {
                    next.status == MemberRunStatus::Idle
                } else {
                    next.status == MemberRunStatus::Queued
                };
            if !expected_closed || !next_reopened {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "formal Reopen requires exact Closed+Stopped predecessor and Active+Queued/Idle successor",
                    "member_run",
                    &current.id,
                    None,
                ));
            }
            if host_mode_transition.is_none() && current.native_session != next.native_session {
                return Err(trust_error(
                    TrustErrorCode::NativeSessionIncompatible,
                    "formal Reopen must preserve the exact verified native Session",
                    "member_run",
                    &current.id,
                    None,
                ));
            }
        }
        let next_team_run = if let Some((expected_run, next_run)) = host_mode_transition {
            let current_run = self.require_team_run_unlocked(&current.team_run_id)?;
            if current_run != *expected_run {
                return Err(StoreError::Conflict(format!(
                    "TEAM_RUN_CHANGED: TeamRun {} changed concurrently; retry Host mode transition",
                    current.team_run_id
                )));
            }
            if expected_run.host_control_mode == next_run.host_control_mode {
                return Err(StoreError::Conflict(
                    "HOST_MODE_TRANSITION_REQUIRED: target Host mode must differ".into(),
                ));
            }
            if expected_run.host_actor.as_ref().is_none_or(|actor| {
                actor.kind != TeamActorKind::Host || actor.id != current.agent_member_id
            }) || next_run.host_actor != expected_run.host_actor
            {
                return Err(StoreError::Conflict(
                    "HOST_AUTHORITY_FENCED: mode transition requires the exact durable Host AgentMember"
                        .into(),
                ));
            }
            if current.coordination_is_active()
                || !matches!(
                    current.status,
                    MemberRunStatus::Stopped | MemberRunStatus::Completed | MemberRunStatus::Failed
                )
            {
                return Err(StoreError::Conflict(
                    "HOST_MODE_TRANSITION_REQUIRES_CLOSED_RUNTIME: Close and settle the Host runtime before changing mode"
                        .into(),
                ));
            }
            let mut allowed = expected_run.clone();
            allowed.host_control_mode = next_run.host_control_mode;
            allowed.host_thread_id = next_run.host_thread_id.clone();
            allowed.updated_at = next_run.updated_at.clone();
            if allowed != *next_run {
                return Err(StoreError::Conflict(
                    "HOST_MODE_TRANSITION_SCOPE: transition may change only Host mode, external thread reference, and timestamp"
                        .into(),
                ));
            }
            match next_run.host_control_mode {
                firm_core::HostControlMode::Managed => {
                    if next.is_external_interactive() || next_run.host_thread_id.is_some() {
                        return Err(StoreError::Conflict(
                            "MANAGED_HOST_REQUIRES_TEAM_RUNTIME: managed Host cannot retain external_interactive profile or external thread"
                                .into(),
                        ));
                    }
                }
                firm_core::HostControlMode::ExternalInteractive => {
                    if !next.is_external_interactive() || next.native_session.is_some() {
                        return Err(StoreError::Conflict(
                            "EXTERNAL_HOST_REQUIRES_USER_DRIVEN_MEMBER_RUN: external Host must use external_interactive without a native-session binding"
                                .into(),
                        ));
                    }
                }
            }
            next_run
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            Some(next_run)
        } else {
            None
        };
        let execution_space_id = self.require_current_member_mutation_scope_unlocked(&current)?;
        ensure_member_provenance_unchanged(&current, next)?;
        ensure_member_lifecycle_revision(&current, next)?;
        ensure_provider_compatibility_cause_unchanged(&current, next)?;
        if next.runtime_generation != current.runtime_generation.saturating_add(1) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_GENERATION_TRANSITION_REQUIRED: ProviderRuntimeProjection {} must advance runtime_generation exactly once through combined Store authority",
                current.id
            )));
        }
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        let canonical_envelope = self
            .latest_trust_envelopes_unlocked(&execution_space_id, "member_run")?
            .remove(&current.id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but no canonical MemberRun exists",
                    current.team_run_id, current.id
                ))
            })?;
        let mut canonical = event_projection::<MemberRun>(&canonical_envelope)?;
        if canonical.team_run_id != current.team_run_id
            || canonical.agent_member_id != current.agent_member_id
            || canonical.role_snapshot != current.role
            || canonical.runtime_generation != current.runtime_generation
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} cannot advance a mismatched canonical generation in Execution Space {}",
                current.team_run_id, current.id, execution_space_id
            )));
        }

        canonical.coordination_status = match next.coordination_status {
            LegacyMemberCoordinationStatus::Active => MemberCoordinationStatus::Active,
            LegacyMemberCoordinationStatus::Closed => MemberCoordinationStatus::Closed,
            LegacyMemberCoordinationStatus::Retired => MemberCoordinationStatus::Retired,
        };
        canonical.runtime_status = match next.status {
            MemberRunStatus::Starting => MemberRuntimeStatus::Starting,
            MemberRunStatus::Idle => MemberRuntimeStatus::Idle,
            MemberRunStatus::Queued => MemberRuntimeStatus::Queued,
            MemberRunStatus::Running => MemberRuntimeStatus::Running,
            MemberRunStatus::Waiting => MemberRuntimeStatus::Waiting,
            MemberRunStatus::Disconnected => MemberRuntimeStatus::Disconnected,
            MemberRunStatus::Reviewing => MemberRuntimeStatus::Reviewing,
            MemberRunStatus::Blocked => MemberRuntimeStatus::Blocked,
            MemberRunStatus::Completed => MemberRuntimeStatus::Completed,
            MemberRunStatus::Failed => MemberRuntimeStatus::Failed,
            MemberRunStatus::Stopped => MemberRuntimeStatus::Stopped,
        };
        canonical.runtime_generation = next.runtime_generation;
        canonical.native_session = next
            .native_session
            .as_ref()
            .map(canonical_native_session)
            .transpose()?;
        canonical.version = canonical.version.saturating_add(1);
        canonical.started_at = next.started_at.clone();
        canonical.last_event_at = next.last_event_at.clone();
        canonical.finished_at = next.finished_at.clone();

        let context = MutationContext {
            execution_space_id,
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: "node-daemon:member-generation".to_string(),
            },
            authority_actor: Some(ActorRef {
                kind: ActorKind::AgentMember,
                id: current.agent_member_id.clone(),
            }),
            command_name: "team_run.advance_member_generation".to_string(),
            idempotency_key: format!(
                "team-run-member-generation:{}:{}",
                current.id, next.runtime_generation
            ),
            expected_version: canonical.version.saturating_sub(1),
            request_fingerprint: None,
        };
        let payload = serde_json::json!({
            "member_run_id": current.id,
            "team_run_id": current.team_run_id,
            "runtime_generation": next.runtime_generation,
            "coordination_status": canonical.coordination_status,
            "runtime_status": canonical.runtime_status,
        });
        // Prove both rows serialize before the first durable mutation.
        serde_json::to_value(next)?;
        serde_json::to_value(&canonical)?;
        if let Some(next_team_run) = next_team_run {
            serde_json::to_value(next_team_run)?;
            self.append_jsonl_unlocked("team_runs.jsonl", next_team_run)?;
        }
        self.append_jsonl_unlocked("member_runs.jsonl", next)?;
        self.commit_trust_projection_unlocked(
            &context,
            "member_run",
            &canonical.id,
            if formal_reopen_actor.is_some() {
                "reopened"
            } else {
                "generation_advanced"
            },
            payload,
            &canonical,
            Vec::new(),
            Vec::new(),
        )?;
        Ok(())
    }

    /// Write the settled provider-native Session binding onto a trust MemberRun.
    /// Fresh starts cannot know the provider thread id at MemberRun creation, so
    /// the binding lands later as its own CAS + generation-fenced mutation.
    /// Coordination status, runtime status, and runtime generation are untouched.
    /// The write is idempotent for the same native id (an identical rebind
    /// carries the same value) and rejects a conflicting rebind to another id.
    pub fn bind_member_run_native_session(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        expected_generation: u64,
        native_session: NativeSessionRef,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(
            &native_session.native_session_id,
            "NativeSessionRef.native_session_id",
        )?;
        required(&native_session.provider, "NativeSessionRef.provider")?;
        let mut run = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "member_run")?
            .remove(member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun not found",
                    "member_run",
                    member_run_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        if run.coordination_status != MemberCoordinationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an active MemberRun can bind a provider-native Session",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        if run.runtime_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "MemberRun runtime generation is {}, the settled binding observed {expected_generation}",
                    run.runtime_generation
                ),
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        if let Some(current) = run.native_session.as_ref() {
            if current.native_session_id != native_session.native_session_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun already binds another provider-native Session",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
        }
        run.native_session = Some(native_session.clone());
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            member_run_id,
            "native_session_bound",
            serde_json::json!({
                "member_run_id": member_run_id,
                "runtime_generation": expected_generation,
                "native_session": native_session,
                "updated_at": updated_at,
            }),
            &run,
            Vec::new(),
            Vec::new(),
        )
    }

    pub(crate) fn trust_side_records<T: for<'de> Deserialize<'de>>(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<T>> {
        let mut rows = Vec::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in envelope
                .operation
                .initial_outbox_records
                .into_iter()
                .chain(envelope.operation.immutable_side_records)
            {
                if let Ok(row) = serde_json::from_value::<T>(value) {
                    rows.push(row);
                }
            }
        }
        Ok(rows)
    }

    pub(super) fn trust_gate_requirements_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, GateRequirement>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in &envelope.operation.immutable_side_records {
                if let Ok(requirement) = serde_json::from_value::<GateRequirement>(value.clone()) {
                    latest.insert(requirement.id.clone(), requirement);
                }
            }
            if envelope.operation.event.aggregate_kind == "gate_requirement" {
                let requirement = event_projection::<GateRequirement>(&envelope)?;
                latest.insert(requirement.id.clone(), requirement);
            }
        }
        Ok(latest)
    }
}
