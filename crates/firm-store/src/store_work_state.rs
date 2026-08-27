use super::*;

impl HarnessStore {
    pub(super) fn resolved_work_condition_record(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: &WorkCommandContext,
    ) -> StoreResult<WorkConditionRecord> {
        let active = self
            .work_condition_records()?
            .into_iter()
            .rev()
            .find(|record| {
                record.work_id == work_id
                    && record.condition == WorkCondition::Blocked
                    && record.resolved_at.is_none()
                    && record.work_version <= expected_version
            })
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "ACTIVE_WORK_CONDITION_REQUIRED: Work {work_id} has no unresolved blocker record"
                ))
            })?;
        Ok(WorkConditionRecord {
            id: format!("work-condition-resolution-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: active.condition,
            owner_actor: context.performed_by_actor.clone(),
            impact: active.impact,
            resume_condition: resolution.to_string(),
            next_check_at: None,
            evidence_refs: active.evidence_refs,
            created_at: context.created_at.clone(),
            resolved_at: Some(context.created_at.clone()),
            supersedes_condition_record_id: Some(active.id),
        })
    }

    pub fn release_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, Some(member_run_id), context)
    }

    pub fn release_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, None, context)
    }

    /// Refresh external GitHub/CI evidence without impersonating a Member
    /// Result or touching execution authority. The authenticated Host poll may
    /// update only the Work's evidence snapshot; lifecycle, responsibility,
    /// reports, attention, bindings and deliveries remain independent.
    pub fn update_work_github_links(
        &self,
        work_id: &str,
        expected_version: u64,
        github_links: Vec<GitHubLink>,
        execution_space_id: &str,
        daemon: &NodeDaemonLease,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let latest = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if context.performed_by_actor.kind != firm_core::TeamActorKind::Service {
            return Err(StoreError::Conflict(
                "WORK_GITHUB_EVIDENCE_SERVICE_REQUIRED: authenticated NodeDaemon Service required"
                    .into(),
            ));
        }
        self.require_current_node_daemon_unlocked(
            execution_space_id,
            &daemon.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &firm_core::agentfirm_api::ActorRef {
                kind: firm_core::agentfirm_api::ActorKind::Service,
                id: context.performed_by_actor.id.clone(),
            },
            "work_github_evidence",
            work_id,
        )?;
        let run = self.require_team_run_unlocked(&latest.team_run_id)?;
        if run.execution_node_id != daemon.node_id {
            return Err(StoreError::Conflict(format!(
                "WORK_GITHUB_EVIDENCE_NODE_FENCED: Work {work_id} TeamRun is placed on {}, not {}",
                run.execution_node_id, daemon.node_id
            )));
        }
        let authority = context.authority_actor.as_ref().ok_or_else(|| {
            StoreError::Conflict(
                "WORK_GITHUB_EVIDENCE_HOST_SOURCE_REQUIRED: exact TeamRun Host source required"
                    .into(),
            )
        })?;
        self.require_exact_team_run_host_actor(authority, &latest.team_run_id)?;
        let request_fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "work_id": work_id,
            "expected_version": expected_version,
            "github_links": github_links,
            "execution_space_id": execution_space_id,
            "node_id": daemon.node_id,
            "daemon_id": daemon.daemon_id,
            "daemon_generation": daemon.generation,
        }));
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            if existing
                .event
                .payload
                .get("request_fingerprint")
                .and_then(serde_json::Value::as_str)
                != Some(request_fingerprint.as_str())
            {
                return Err(StoreError::Conflict(format!(
                    "IDEMPOTENCY_CONFLICT: key {} was reused for different GitHub evidence",
                    context.idempotency_key
                )));
            }
            return Ok(existing.work);
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.github_links == github_links {
            return Ok(current);
        }
        let mut next = current.clone();
        next.github_links = github_links;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({
                "reason": "github_evidence_refresh",
                "request_fingerprint": request_fingerprint,
            }),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn accept_work(
        &self,
        _work_id: &str,
        _expected_version: u64,
        _context: WorkCommandContext,
    ) -> StoreResult<Work> {
        Err(StoreError::Conflict(
            "LEGACY_WORK_ACCEPT_RETIRED: use the authenticated team-scoped member-trust Work acceptance command"
                .to_string(),
        ))
    }

    pub fn accept_work_with_summary(
        &self,
        _work_id: &str,
        _expected_version: u64,
        _summary: Option<&str>,
        _context: WorkCommandContext,
    ) -> StoreResult<Work> {
        Err(StoreError::Conflict(
            "LEGACY_WORK_ACCEPT_RETIRED: use the authenticated team-scoped member-trust Work acceptance command"
                .to_string(),
        ))
    }
    pub fn request_work_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "changes-requested reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ChangesRequested,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.phase != WorkPhase::Review || current.condition != WorkCondition::Normal {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must await Host acceptance"
            )));
        }
        let mut next = current.clone();
        // A submitted execution has already settled its exact binding. Host
        // changes therefore return the stable responsibility to the scheduler
        // queue; a new execution admission must Start the next attempt.
        next.phase = WorkPhase::Open;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::ChangesRequested,
            context,
        )
    }

    pub fn cancel_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "cancellation reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_host_actor(&context.performed_by_actor)?;
        let current = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        let request_payload = serde_json::json!({
            "work_id": work_id,
            "expected_version": expected_version,
            "reason": reason,
        });
        let (mutation_context, fingerprint) = self.canonical_work_command_context_unlocked(
            &current,
            expected_version,
            "work.cancel",
            &context,
            &request_payload,
        )?;
        if let Some(replay) =
            self.replay_current_work_mutation_unlocked(&mutation_context, work_id, &fingerprint)?
        {
            return Ok(replay.projection);
        }
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "WORK_VERSION_CONFLICT: Work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already terminal"
            )));
        }
        self.ensure_no_claimed_delivery_unlocked(&current)?;
        let mut next = current.clone();
        next.phase = WorkPhase::Closed;
        next.condition = WorkCondition::Normal;
        next.resolution = Some(WorkResolution::Cancelled);
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        // Preserve the historical WorkEvent read contract as an immutable
        // record inside the one canonical operation. It is not a second Work
        // writer: the resulting Work projection and its successor outbox are
        // committed by commit_current_work_mutation_unlocked.
        let compatibility_event = WorkEvent {
            id: context.event_id.clone(),
            team_run_id: next.team_run_id.clone(),
            work_id: next.id.clone(),
            sequence: next.version,
            kind: WorkEventKind::Cancelled,
            expected_version,
            resulting_version: next.version,
            performed_by_actor: context.performed_by_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: context.causation_ref.clone(),
            idempotency_key: context.idempotency_key.clone(),
            payload: request_payload.clone(),
            created_at: context.created_at.clone(),
        };
        let result = self.commit_current_work_mutation_unlocked(
            &mutation_context,
            "cancelled",
            request_payload,
            &next,
            vec![serde_json::to_value(compatibility_event)?],
            Vec::new(),
        )?;
        Ok(result.projection)
    }

    /// Close a non-terminal Work as failed by an explicit Host decision.
    /// Provider failure never calls this implicitly; it is a responsibility-
    /// plane judgment recorded with the exact Work revision.
    pub fn fail_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        failure_analysis_ref: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() || failure_analysis_ref.trim().is_empty() {
            return Err(StoreError::Conflict(
                "failure reason and FailureAnalysis reference are required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_host_actor(&context.performed_by_actor)?;
        let current = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        let request_payload = serde_json::json!({
            "work_id": work_id,
            "expected_version": expected_version,
            "reason": reason,
            "failure_analysis_ref": failure_analysis_ref,
        });
        let (mutation_context, fingerprint) = self.canonical_work_command_context_unlocked(
            &current,
            expected_version,
            "work.fail",
            &context,
            &request_payload,
        )?;
        if let Some(replay) =
            self.replay_current_work_mutation_unlocked(&mutation_context, work_id, &fingerprint)?
        {
            return Ok(replay.projection);
        }
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "WORK_VERSION_CONFLICT: Work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already terminal"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.phase = WorkPhase::Closed;
        next.condition = WorkCondition::Normal;
        next.resolution = Some(WorkResolution::Failed);
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let decision = WorkOperationalDecision {
            id: format!("work-decision-{}", context.event_id),
            work_id: work_id.to_string(),
            expected_work_version: expected_version,
            kind: firm_core::WorkDecisionKind::Fail,
            decided_by_actor: context.performed_by_actor.clone(),
            rationale: reason.to_string(),
            work_report_id: None,
            gate_requirement_ref: None,
            failure_analysis_ref: Some(failure_analysis_ref.to_string()),
            evidence_refs: Vec::new(),
            created_at: context.created_at.clone(),
        };
        let result = self.commit_current_work_mutation_unlocked(
            &mutation_context,
            "failed",
            request_payload,
            &next,
            vec![serde_json::to_value(decision)?],
            Vec::new(),
        )?;
        Ok(result.projection)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn transition_owned_work_with_payload(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_lifecycle: (WorkPhase, WorkCondition),
        resulting_lifecycle: (WorkPhase, WorkCondition),
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be mutated"
                    .to_string(),
            ));
        }
        // A Closed or Retired ProviderRuntimeProjection no longer mutates its owned Work:
        // unfinished Work moves only via Host reassign/cancel or after an
        // explicit Reopen (docs/product/agent-team-works.md). This aligns
        // member-side transitions with insert/claim/start/receive, which
        // already require active coordination.
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if (current.phase, current.condition) != required_lifecycle
            || !self.member_run_holds_work_responsibility_unlocked(&current, &member)?
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not hold active Work responsibility for {work_id} in required state"
            )));
        }
        if !member.coordination_is_active() {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: ProviderRuntimeProjection {member_run_id} coordination is {:?}; Reopen before mutating owned Work",
                member.coordination_status
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.phase = resulting_lifecycle.0;
        next.condition = resulting_lifecycle.1;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            condition_records,
            reports,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn transition_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_lifecycle: (WorkPhase, WorkCondition),
        resulting_lifecycle: (WorkPhase, WorkCondition),
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if (current.phase, current.condition) != required_lifecycle {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is not in required state"
            )));
        }
        if current.owner_member_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} has no owner to retain"
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.phase = resulting_lifecycle.0;
        next.condition = resulting_lifecycle.1;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            condition_records,
            reports,
            Vec::new(),
        )
    }

    pub(super) fn release_work_with_authority(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Released,
        )? {
            return Ok(existing.work);
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be released"
                    .to_string(),
            ));
        }
        if current.phase != WorkPhase::Open || current.condition != WorkCondition::Normal {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must be open to release"
            )));
        }
        if current.active_member_run_id.is_none()
            && current.owner_member_id.is_none()
            && current.assignee_membership_id.is_none()
        {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already unassigned"
            )));
        }
        match member_run_id {
            Some(member_run_id) => {
                require_member_actor(&context.performed_by_actor, member_run_id)?;
                let member =
                    self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
                if !self.member_run_holds_work_responsibility_unlocked(&current, &member)? {
                    return Err(StoreError::Conflict(format!(
                        "ProviderRuntimeProjection {member_run_id} does not hold responsibility for open Work {work_id}"
                    )));
                }
            }
            None => {
                require_host_actor(&context.performed_by_actor)?;
                self.require_exact_team_run_host_actor(
                    &context.performed_by_actor,
                    &current.team_run_id,
                )?;
            }
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.owner_member_id = None;
        next.active_member_run_id = None;
        next.assignee_membership_id = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Released, context)
    }

    pub(super) fn append_work_transition_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            kind,
            context,
            serde_json::Value::Null,
        )
    }

    pub(super) fn append_work_transition_with_payload_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        payload: serde_json::Value,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_work_transition_with_records_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        decisions: Vec<WorkOperationalDecision>,
    ) -> StoreResult<Work> {
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let sequence = self
            .work_operations_unlocked()?
            .iter()
            .filter(|operation| operation.work.id == current.id)
            .count() as u64
            + 1;
        let payload = self.work_graph_outbox_payload_unlocked(&next, kind, payload)?;
        let evidence_records = reports
            .iter()
            .map(|report| {
                let evidence_id = report.evidence_refs.first().cloned().ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_REPORT_EVIDENCE_REQUIRED: report {} has no candidate evidence",
                        report.id
                    ))
                })?;
                Ok(WorkEvidence {
                    id: evidence_id,
                    work_id: report.work_id.clone(),
                    work_report_id: report.id.clone(),
                    work_version: report.work_version,
                    candidate_revision: report.candidate_revision.clone(),
                    source_type: "work_candidate_revision".to_string(),
                    source_ref: report.candidate_revision.clone(),
                    summary: format!(
                        "Exact candidate evidence for immutable WorkReport {}",
                        report.id
                    ),
                    created_at: report.created_at.clone(),
                })
            })
            .collect::<StoreResult<Vec<_>>>()?;
        let delegation_revisions =
            self.work_delegation_rollup_revisions_unlocked(&next, &context)?;
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: next.team_run_id.clone(),
                work_id: next.id.clone(),
                sequence,
                kind,
                expected_version: current.version,
                resulting_version: next.version,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload,
                created_at: context.created_at,
            },
            work: next.clone(),
            condition_records,
            reports,
            evidence_records,
            decisions,
            delegation_revisions,
        };
        self.append_work_operation_unlocked(&operation)?;
        // The outbox itself is in the crash-atomic WorkOperation. Materialized
        // HostAttention rows are deterministic and replay-repairable.
        self.ensure_downstream_host_attentions_for_work_operation_unlocked(&operation)?;
        self.ensure_host_attention_for_work_operation_unlocked(&operation)?;
        Ok(next)
    }
}
