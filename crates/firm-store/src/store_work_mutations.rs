use super::*;

impl HarnessStore {
    fn require_unassigned_work_creation(work: &Work) -> StoreResult<()> {
        if work.active_member_run_id.is_some() {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: Work creation cannot carry active_member_run_id; assign one canonical TeamMembership, then admit execution through WorkExecutionBinding"
                    .to_string(),
            ));
        }
        if work.owner_member_id.is_some() || work.assignee_membership_id.is_some() {
            return Err(StoreError::Conflict(
                "WORK_CREATE_UNASSIGNED_REQUIRED: Work creation cannot carry responsibility; create unassigned, then assign one canonical TeamMembership"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Insert a Work and its authoritative creation event/outbox as one
    /// crash-atomic JSONL row. Work commands intentionally refuse a legacy
    /// Assignment-message store so one Execution Space never has two ownership
    /// authorities.
    pub fn insert_work(&self, mut work: Work, context: WorkCommandContext) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        Self::require_unassigned_work_creation(&work)?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            &work.id,
            WorkEventKind::Created,
        )? {
            return Ok(existing.work);
        }
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let team_run = self.require_team_run_unlocked(&work.team_run_id)?;
        if matches!(
            team_run.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "team run {} is {:?} and cannot accept new Work",
                team_run.id, team_run.status
            )));
        }
        let run_team_id = durable_team_id(&team_run);
        match (work.accountable_team_id.as_deref(), run_team_id) {
            (Some(work_team_id), Some(run_team_id)) if work_team_id != run_team_id => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_MISMATCH: Work names accountable AgentTeam {work_team_id}, but TeamRun {} belongs to {run_team_id}",
                    team_run.id
                )));
            }
            (Some(_), Some(_)) => {}
            (None, Some(run_team_id)) => work.accountable_team_id = Some(run_team_id.to_string()),
            (Some(_), None) => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_UNAVAILABLE: TeamRun {} has no durable AgentTeam identity",
                    team_run.id
                )));
            }
            _ => {}
        }
        if self.latest_works_unlocked()?.contains_key(work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                work.id
            )));
        }
        if !context.duplicate_ok {
            let normalized = normalize_work_title(&work.title);
            for existing in self.latest_works_unlocked()?.values() {
                if existing.team_run_id == work.team_run_id
                    && !existing.is_terminal()
                    && normalize_work_title(&existing.title) == normalized
                {
                    return Err(StoreError::Conflict(format!(
                        "DUPLICATE_TITLE: a non-terminal Work ({}) with title \"{}\" already exists in team run {}; pass --duplicate-ok to skip this guard",
                        existing.id, existing.title, work.team_run_id
                    )));
                }
            }
        }
        if work.title.trim().is_empty() || work.completion_criteria_markdown.trim().is_empty() {
            return Err(StoreError::Conflict(
                "work title and completion criteria are required".to_string(),
            ));
        }
        work.version = 1;
        work.phase = WorkPhase::Open;
        work.condition = WorkCondition::Normal;
        work.resolution = None;
        work.created_at = context.created_at.clone();
        work.updated_at = context.created_at.clone();
        work.created_by_actor = context.performed_by_actor.clone();
        match context.performed_by_actor.kind {
            firm_core::TeamActorKind::ProviderRuntimeProjection => {
                let member = self.require_member_run_unlocked(
                    &context.performed_by_actor.id,
                    &work.team_run_id,
                )?;
                if !member.has_live_runtime_authority() {
                    return Err(StoreError::Conflict(
                        "only a live ProviderRuntimeProjection may create Work".to_string(),
                    ));
                }
                let own_identity = member_identity(&member);
                if work
                    .created_by_member_id
                    .as_deref()
                    .is_some_and(|creator| creator != own_identity)
                {
                    return Err(StoreError::Conflict(
                        "created_by_member_id does not match creator ProviderRuntimeProjection stable identity"
                            .to_string(),
                    ));
                }
                work.created_by_member_id = Some(own_identity.clone());
            }
            _ => {
                self.require_exact_team_run_host_actor(
                    &context.performed_by_actor,
                    &work.team_run_id,
                )?;
                if work.created_by_member_id.is_some() {
                    return Err(StoreError::Conflict(
                        "only a ProviderRuntimeProjection actor may set created_by_member_id"
                            .to_string(),
                    ));
                }
            }
        }
        self.validate_work_relations_unlocked(&work)?;
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: work.team_run_id.clone(),
                work_id: work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload: serde_json::Value::Null,
                created_at: context.created_at,
            },
            work: work.clone(),
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
            decisions: Vec::new(),
            delegation_revisions: Vec::new(),
        };
        self.append_work_operation_unlocked(&operation)?;
        Ok(work)
    }

    /// Create a target Team's root Work and the cross-Team Delegation in one
    /// crash-atomic ledger row. No target Work becomes visible without the
    /// corresponding Delegation event and projection.
    pub fn create_work_delegation_with_target_work(
        &self,
        mut delegation: WorkDelegation,
        mut target_work: Work,
        context: WorkCommandContext,
    ) -> StoreResult<(WorkDelegation, Work)> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        Self::require_unassigned_work_creation(&target_work)?;

        let request_fingerprint =
            work_delegation_request_fingerprint(&delegation, &target_work, &context);

        if let Some(existing) = self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .find(|revision| revision.event.idempotency_key == context.idempotency_key)
        {
            self.require_work_delegation_actor_unlocked(
                &context.performed_by_actor,
                &existing.delegation.source_work_ref.team_run_id,
                &existing.delegation.source_owner_member_id,
                "delegate",
            )?;
            if existing.event.payload.get("request_fingerprint") == Some(&request_fingerprint) {
                let target = self
                    .latest_works_unlocked()?
                    .remove(&existing.delegation.target_work_ref.work_id)
                    .ok_or_else(|| {
                        StoreError::Conflict(
                            "DELEGATION_CORRUPT: idempotent target Work is missing".to_string(),
                        )
                    })?;
                return Ok((existing.delegation, target));
            }
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {} already belongs to Delegation {}",
                context.idempotency_key, existing.delegation.id
            )));
        }

        let source = self.current_work_unlocked(
            &delegation.source_work_ref.work_id,
            delegation.source_work_version,
        )?;
        if source.team_run_id != delegation.source_work_ref.team_run_id {
            return Err(StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source WorkRef does not match the authoritative Work"
                    .to_string(),
            ));
        }
        let source_owner = source.owner_member_id.clone().ok_or_else(|| {
            StoreError::Conflict(
                "DELEGATION_NOT_AUTHORIZED: source Work has no durable owner".to_string(),
            )
        })?;
        if delegation.source_owner_member_id != source_owner {
            return Err(StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source owner changed".to_string(),
            ));
        }
        if let Some(member_run_id) = self.require_work_delegation_actor_unlocked(
            &context.performed_by_actor,
            &source.team_run_id,
            &source_owner,
            "delegate",
        )? {
            delegation.created_by_member_run_id = Some(member_run_id);
        }

        let target_team = latest_by_id(self.all_agent_teams()?, |team| team.id.clone())
            .remove(&delegation.target_agent_team_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "DELEGATION_TARGET_INVALID: AgentTeam {} not found",
                    delegation.target_agent_team_id
                ))
            })?;
        if target_team.status != firm_core::AgentTeamStatus::Active {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_TARGET_INVALID: AgentTeam {} is {:?}",
                target_team.id, target_team.status
            )));
        }
        let target_run = self.require_team_run_unlocked(&target_work.team_run_id)?;
        if target_run.agent_team_id != target_team.id
            || target_run.execution_node_id != target_team.node_id
            || matches!(
                target_run.status,
                TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
            )
        {
            return Err(StoreError::Conflict(
                "DELEGATION_TARGET_INVALID: target TeamRun is not an active run of target Team"
                    .to_string(),
            ));
        }
        let source_team_id = source.accountable_team_id.clone().ok_or_else(|| {
            StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source Work has no AgentTeam provenance".to_string(),
            )
        })?;
        if source_team_id == target_team.id {
            return Err(StoreError::Conflict(
                "DELEGATION_TARGET_INVALID: cross-Team Delegation requires a different target Team"
                    .to_string(),
            ));
        }
        let latest_works = self.latest_works_unlocked()?;
        if latest_works.contains_key(target_work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                target_work.id
            )));
        }
        let target_ref = WorkRef {
            team_run_id: target_work.team_run_id.clone(),
            work_id: target_work.id.clone(),
        };
        // Delegation always creates a fresh target Work, so WorkRef-level cycle
        // detection would be vacuous. The meaningful graph is Team -> Team:
        // reject A -> B when a non-cancelled B -> ... -> A path already exists.
        let delegations = self.latest_work_delegations_unlocked()?;
        let mut outgoing = std::collections::BTreeMap::<String, Vec<String>>::new();
        for existing in delegations
            .values()
            .filter(|candidate| candidate.state != WorkDelegationState::Cancelled)
        {
            let existing_source = latest_works
                .get(&existing.source_work_ref.work_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "DELEGATION_CORRUPT: source Work {} is missing",
                        existing.source_work_ref.work_id
                    ))
                })?;
            let existing_source_team =
                existing_source
                    .accountable_team_id
                    .as_ref()
                    .ok_or_else(|| {
                        StoreError::Conflict(format!(
                            "DELEGATION_CORRUPT: source Work {} has no AgentTeam provenance",
                            existing_source.id
                        ))
                    })?;
            outgoing
                .entry(existing_source_team.clone())
                .or_default()
                .push(existing.target_agent_team_id.clone());
        }
        let mut pending = vec![target_team.id.clone()];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(cursor) = pending.pop() {
            if !visited.insert(cursor.clone()) {
                continue;
            }
            if cursor == source_team_id {
                return Err(StoreError::Conflict(
                    "DELEGATION_CYCLE: cross-Team delegation graph must be acyclic".to_string(),
                ));
            }
            if let Some(next) = outgoing.get(&cursor) {
                pending.extend(next.iter().cloned());
            }
        }

        target_work.accountable_team_id = Some(target_team.id.clone());
        target_work.legacy_containment_ref = None;
        target_work.phase = WorkPhase::Open;
        target_work.condition = WorkCondition::Normal;
        target_work.resolution = None;
        target_work.version = 1;
        target_work.created_at = context.created_at.clone();
        target_work.updated_at = context.created_at.clone();
        target_work.created_by_actor = context.performed_by_actor.clone();
        target_work
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        self.validate_work_relations_unlocked(&target_work)?;

        delegation.target_work_ref = target_ref;
        delegation.delegated_by_actor = context.performed_by_actor.clone();
        delegation.state = WorkDelegationState::Active;
        delegation.resolution_summary = None;
        delegation.blocker_reason = None;
        delegation.version = 1;
        delegation.created_at = context.created_at.clone();
        delegation.updated_at = context.created_at.clone();
        delegation
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
        if self
            .latest_work_delegations_unlocked()?
            .contains_key(&delegation.id)
        {
            return Err(StoreError::Conflict(format!(
                "work delegation already exists: {}",
                delegation.id
            )));
        }

        let target_event_id = format!("{}:target-work", context.event_id);
        self.ensure_work_event_id_available_unlocked(&target_event_id)?;
        let target_work_operation = WorkOperation {
            event: WorkEvent {
                id: target_event_id,
                team_run_id: target_work.team_run_id.clone(),
                work_id: target_work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.performed_by_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: Some(firm_core::WorkCausationRef {
                    kind: "work_delegation".to_string(),
                    id: delegation.id.clone(),
                }),
                idempotency_key: format!("{}:target-work", context.idempotency_key),
                payload: serde_json::json!({
                    "delegation_id": delegation.id,
                    "source_work_ref": delegation.source_work_ref,
                }),
                created_at: context.created_at.clone(),
            },
            work: target_work.clone(),
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
            decisions: Vec::new(),
            delegation_revisions: Vec::new(),
        };
        let event = WorkDelegationEvent {
            id: context.event_id,
            delegation_id: delegation.id.clone(),
            sequence: 1,
            transition: WorkDelegationTransition::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: context.performed_by_actor,
            causation_ref: context.causation_ref,
            idempotency_key: context.idempotency_key,
            payload: serde_json::json!({"request_fingerprint": request_fingerprint}),
            created_at: context.created_at,
        };
        event
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}")))?;
        self.append_jsonl_unlocked(
            "work_delegation_operations.jsonl",
            &WorkDelegationOperation {
                delegation: delegation.clone(),
                event,
                target_work_operation,
            },
        )?;
        Ok((delegation, target_work))
    }

    /// Assign or reassign Work responsibility to exactly one TeamMembership of
    /// the Work's accountable Team (DOC-106). This is the canonical
    /// responsibility mutation: it fences on the expected Work version, never
    /// requires a running provider process, and never creates execution
    /// authority. A Paused AgentMember or Inactive TeamMembership may hold
    /// responsibility; automatic execution authority begins only with a later
    /// exact WorkExecutionBinding against the new revision.
    pub fn assign_work_to_membership(
        &self,
        work_id: &str,
        expected_version: u64,
        membership_id: &str,
        execution_space_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Assigned,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work cannot be assigned; export or verify it without creating current authority"
                    .to_string(),
            ));
        }
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be reassigned"
            )));
        }
        let team_id = current.accountable_team_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_NOT_TEAM_SCOPED: run responsibility migration for Work {work_id} before membership assignment"
            ))
        })?;
        if current.assignee_membership_id.as_deref() == Some(membership_id) {
            return Err(StoreError::Conflict(format!(
                "WORK_ALREADY_ASSIGNED: Work {work_id} is already assigned to TeamMembership {membership_id}"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let membership = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .find(|membership| membership.id == membership_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_MEMBERSHIP_NOT_FOUND: TeamMembership {membership_id} does not exist"
                ))
            })?;
        if membership.team_id != team_id {
            return Err(StoreError::Conflict(format!(
                "TEAM_SCOPE_MISMATCH: TeamMembership {membership_id} belongs to Team {}, not the Work's accountable Team {team_id}",
                membership.team_id
            )));
        }
        if membership.role == firm_core::agentfirm_api::TeamMembershipRole::Observer {
            return Err(StoreError::Conflict(format!(
                "ASSIGNEE_ROLE_INVALID: Observer TeamMembership {membership_id} cannot hold Work responsibility"
            )));
        }
        // Automatic execution authority requires both an Active membership and
        // an Active AgentMember; everything else holds responsibility dormant.
        let agent_member_active = self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .find(|member| member.id == membership.agent_member_id)
            .is_some_and(|member| {
                member.organization_status
                    == firm_core::agentfirm_api::AgentMemberOrganizationStatus::Active
            });
        let automatic_execution_authority = membership.state
            == firm_core::agentfirm_api::TeamMembershipStatus::Active
            && agent_member_active;
        let mut next = current.clone();
        next.assignee_membership_id = Some(membership.id.clone());
        next.owner_member_id = Some(membership.agent_member_id.clone());
        // Responsibility moves; any legacy runtime binding of the previous
        // assignee is fenced off. The transition appends no delivery because
        // the new projection carries no runtime binding.
        let cleared_member_run_id = next.active_member_run_id.take();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Assigned,
            context,
            serde_json::json!({
                "assignee_membership_id": membership.id,
                "assignee_agent_member_id": membership.agent_member_id,
                "assignee_membership_state": membership.state,
                "automatic_execution_authority": automatic_execution_authority,
                "cleared_active_member_run_id": cleared_member_run_id,
            }),
        )
    }

    /// Append an explicit full-projection repair after a stale mixed-version
    /// writer omitted immutable additive provenance. Raw sparse operations
    /// remain untouched; the recovered reducer state becomes a new `Updated`
    /// WorkOperation at the next version without changing lifecycle, owner, or
    /// runtime binding.
    pub fn reconcile_work_projection_provenance(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let raw_current = latest_by_id(self.work_operations_unlocked()?, |operation| {
            operation.work.id.clone()
        })
        .remove(work_id)
        .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        self.require_exact_team_run_host_actor(
            &context.performed_by_actor,
            &raw_current.work.team_run_id,
        )?;
        if raw_current.work.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is at version {}, expected {expected_version}",
                raw_current.work.version
            )));
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        let mut recovered_fields = Vec::new();
        if raw_current.work.accountable_team_id.is_none() && current.accountable_team_id.is_some() {
            recovered_fields.push("accountable_team_id");
        }
        if raw_current.work.created_by_member_id.is_none() && current.created_by_member_id.is_some()
        {
            recovered_fields.push("created_by_member_id");
        }
        if recovered_fields.is_empty() {
            return Err(StoreError::Conflict(format!(
                "WORK_PROJECTION_PROVENANCE_CURRENT: Work {work_id} has no recoverable sparse provenance"
            )));
        }

        let mut next = current.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({
                "reason": "mixed_version_projection_recovery",
                "recovered_fields": recovered_fields,
                "source_event_id": raw_current.event.id,
            }),
        )
    }

    /// Move a persistent Work onto a successor execution attempt of the same
    /// AgentTeam. Stable ownership, creator provenance, and
    /// Work identity remain unchanged; only the execution binding moves.
    pub fn retarget_work_execution(
        &self,
        work_id: &str,
        expected_version: u64,
        successor_team_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ExecutionRetargeted,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work cannot be retargeted; export or verify it without creating current authority"
                    .to_string(),
            ));
        }
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be retargeted"
            )));
        }
        self.reconcile_work_host_attentions_unlocked()?;
        if self
            .latest_host_attentions_unlocked()?
            .values()
            .any(|attention| {
                attention.work_id == current.id
                    && attention.team_run_id == current.team_run_id
                    && attention.needs_host_action()
            })
        {
            return Err(StoreError::Conflict(format!(
                "HOST_ATTENTION_PENDING: Work {work_id} has unresolved attention owned by TeamRun {}; the exact Host must ACK intake before execution retarget",
                current.team_run_id
            )));
        }
        let team_id = current.accountable_team_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_NOT_TEAM_SCOPED: run responsibility migration for Work {work_id} before retargeting execution"
            ))
        })?;
        if current.team_run_id == successor_team_run_id {
            return Err(StoreError::Conflict(format!(
                "Work {work_id} already targets TeamRun {successor_team_run_id}"
            )));
        }
        let successor = self.require_team_run_unlocked(successor_team_run_id)?;
        if matches!(
            successor.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "successor TeamRun {} is {:?} and cannot execute Work",
                successor.id, successor.status
            )));
        }
        if durable_team_id(&successor) != Some(team_id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "TEAM_SCOPE_MISMATCH: successor TeamRun {} does not belong to AgentTeam {team_id}",
                successor.id
            )));
        }
        if self
            .canonical_work_deliveries_for_work_unlocked(&current)?
            .iter()
            .any(|delivery| delivery.status == WorkDeliveryStatus::Claimed)
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work has a claimed delivery".to_string(),
            ));
        }

        let previous_team_run_id = current.team_run_id.clone();
        let mut next = current.clone();
        next.team_run_id = successor_team_run_id.to_string();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let responsibility_membership_id = next.assignee_membership_id.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::ExecutionRetargeted,
            context,
            serde_json::json!({
                "team_id": team_id,
                "previous_team_run_id": previous_team_run_id,
                "successor_team_run_id": successor_team_run_id,
                "responsibility_membership_id": responsibility_membership_id,
            }),
        )
    }

    pub fn claim_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Claimed,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.phase != WorkPhase::Open
            || current.condition != WorkCondition::Normal
            || current.owner_member_id.is_some()
            || current.claim_mode != WorkClaimMode::TeamClaim
        {
            return Err(StoreError::Conflict(format!(
                "CLAIM_LOST: work {work_id} is not an unowned team-claim Work"
            )));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !matches!(
            member.status,
            firm_core::MemberRunStatus::Idle | firm_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} is not available and active"
            )));
        }
        let owner_id = member_identity(&member);
        if !current.eligible_member_ids.is_empty()
            && !current.eligible_member_ids.iter().any(|id| id == &owner_id)
        {
            return Err(StoreError::Conflict(format!(
                "member {owner_id} is not eligible to claim work {work_id}"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        let mut next = current.clone();
        next.owner_member_id = Some(owner_id);
        next.assignee_membership_id = self.resolve_assignee_membership_id_unlocked(
            next.accountable_team_id.as_deref(),
            next.owner_member_id.as_deref().unwrap_or_default(),
        )?;
        if next.assignee_membership_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "WORK_RESPONSIBILITY_UNRESOLVED: member {member_run_id} has no exact active TeamMembership for Work {work_id}"
            )));
        }
        // Claim freezes only stable TeamMembership/AgentMember responsibility.
        // Runtime ownership is resolved later by the canonical scheduler into
        // one exact WorkExecutionBinding before Start; it is never copied into
        // the Work projection.
        next.active_member_run_id = None;
        next.phase = WorkPhase::Open;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Claimed, context)
    }

    pub fn start_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Started,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be started"
                    .to_string(),
            ));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if current.phase != WorkPhase::Open
            || current.condition != WorkCondition::Normal
            || !self.member_run_holds_work_responsibility_unlocked(&current, &member)?
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not hold responsibility for open Work {work_id}"
            )));
        }
        if !matches!(
            member.status,
            firm_core::MemberRunStatus::Idle | firm_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} is not available and active"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        if works.iter().any(|work| {
            work.team_run_id == current.team_run_id
                && work.phase == WorkPhase::Active
                && work.condition == WorkCondition::Normal
                && work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
        }) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} already has active Work"
            )));
        }
        let mut next = current.clone();
        next.phase = WorkPhase::Active;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Started, context)
    }

    pub fn block_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        let condition_record = WorkConditionRecord {
            id: format!("work-condition-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: WorkCondition::Blocked,
            owner_actor: context.performed_by_actor.clone(),
            impact: reason.to_string(),
            resume_condition: "blocker is resolved and evidence is recorded".to_string(),
            next_check_at: None,
            evidence_refs: Vec::new(),
            created_at: context.created_at.clone(),
            resolved_at: None,
            supersedes_condition_record_id: None,
        };
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Blocked,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Active, WorkCondition::Blocked),
            serde_json::Value::Null,
            vec![condition_record],
            Vec::new(),
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn block_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        let condition_record = WorkConditionRecord {
            id: format!("work-condition-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: WorkCondition::Blocked,
            owner_actor: context.performed_by_actor.clone(),
            impact: reason.to_string(),
            resume_condition: "blocker is resolved and evidence is recorded".to_string(),
            next_check_at: None,
            evidence_refs: Vec::new(),
            created_at: context.created_at.clone(),
            resolved_at: None,
            supersedes_condition_record_id: None,
        };
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Blocked,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Active, WorkCondition::Blocked),
            serde_json::Value::Null,
            vec![condition_record],
            Vec::new(),
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn resume_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        let resolved_record =
            self.resolved_work_condition_record(work_id, expected_version, resolution, &context)?;
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Resumed,
            (WorkPhase::Active, WorkCondition::Blocked),
            (WorkPhase::Active, WorkCondition::Normal),
            serde_json::json!({ "resolution": resolution }),
            vec![resolved_record],
            Vec::new(),
            |work| work.blocker_reason = None,
        )
    }

    pub fn resume_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        let resolved_record =
            self.resolved_work_condition_record(work_id, expected_version, resolution, &context)?;
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Resumed,
            (WorkPhase::Active, WorkCondition::Blocked),
            (WorkPhase::Active, WorkCondition::Normal),
            serde_json::json!({ "resolution": resolution }),
            vec![resolved_record],
            Vec::new(),
            |work| work.blocker_reason = None,
        )
    }
}
