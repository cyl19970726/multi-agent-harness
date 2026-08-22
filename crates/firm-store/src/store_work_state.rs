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

    #[allow(clippy::too_many_arguments)]
    pub fn submit_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.submit_work_with_links(
            work_id,
            expected_version,
            member_run_id,
            result_summary,
            artifact_refs,
            check_refs,
            Vec::new(),
            context,
        )
    }

    /// [`submit_work`] plus an explicit GitHub issue/PR linkage snapshot
    /// (issue #369). The base method keeps its historical signature; links are
    /// merged into any links already attached at create time.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_work_with_links(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.submit_work_with_revision_and_links(
            work_id,
            expected_version,
            member_run_id,
            result_summary,
            artifact_refs,
            check_refs,
            github_links,
            None,
            None,
            context,
        )
    }

    /// Submit one immutable candidate. `candidate_revision` is the preferred
    /// source revision for code delivery; when omitted the Store derives a
    /// deterministic digest from the complete submitted payload.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_work_with_revision_and_links(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        github_links: Vec<GitHubLink>,
        base_revision: Option<String>,
        candidate_revision: Option<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        let candidate_revision = candidate_revision
            .filter(|revision| !revision.trim().is_empty())
            .unwrap_or_else(|| {
                canonical_work_candidate_revision(
                    result_summary,
                    &artifact_refs,
                    &check_refs,
                    &github_links,
                )
            });
        if base_revision
            .as_deref()
            .is_some_and(|revision| revision.trim().is_empty())
        {
            return Err(StoreError::Conflict(
                "base revision must not be empty".to_string(),
            ));
        }
        let report_id = format!("work-report-{}", context.event_id);
        let evidence_id = format!("work-evidence-{}", context.event_id);
        let report = WorkReport {
            id: report_id,
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            report_revision: 1,
            submitted_by_actor: context.performed_by_actor.clone(),
            base_revision,
            candidate_revision,
            result_summary: result_summary.to_string(),
            artifact_refs: artifact_refs.clone(),
            check_refs: check_refs.clone(),
            evidence_refs: vec![evidence_id],
            known_risks: Vec::new(),
            created_at: context.created_at.clone(),
        };
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Submitted,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Review, WorkCondition::Normal),
            serde_json::Value::Null,
            Vec::new(),
            vec![report],
            |work| {
                work.result_summary = Some(result_summary.to_string());
                work.artifact_refs = artifact_refs;
                work.check_refs = check_refs;
                // Issue links describe durable provenance. Pull-request links
                // describe this submission candidate and are replaced, so a
                // prior merged PR cannot satisfy a resubmitted candidate.
                let mut candidate_links = work
                    .github_links
                    .iter()
                    .filter(|link| link.kind == firm_core::GitHubLinkKind::Issue)
                    .cloned()
                    .collect::<Vec<_>>();
                for link in github_links {
                    if !candidate_links.contains(&link) {
                        candidate_links.push(link);
                    }
                }
                work.github_links = candidate_links;
                work.blocker_reason = None;
            },
        )
    }

    /// Refresh the GitHub linkage snapshot on a Work without touching its
    /// lifecycle (issue #369 Phase 2, daemon CI poll). Host/Service actor
    /// only. When the links are unchanged the current Work is returned without
    /// appending a `Updated` operation, so a steady-state poll never churns
    /// versions.
    pub fn update_work_github_links(
        &self,
        work_id: &str,
        expected_version: u64,
        github_links: Vec<GitHubLink>,
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
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.github_links == github_links {
            return Ok(current);
        }
        let mut next = current.clone();
        next.github_links = github_links;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let reports = if current.phase == WorkPhase::Review {
            let previous = self
                .work_operations_unlocked()?
                .into_iter()
                .flat_map(|operation| operation.reports)
                .filter(|report| {
                    report.work_id == current.id && report.work_version == current.version
                })
                .max_by_key(|report| report.report_revision)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "CURRENT_WORK_REPORT_REQUIRED: Work {work_id} version {} cannot refresh review evidence",
                        current.version
                    ))
                })?;
            vec![WorkReport {
                id: format!("work-report-{}", context.event_id),
                work_id: previous.work_id,
                work_version: next.version,
                report_revision: previous.report_revision.saturating_add(1),
                submitted_by_actor: previous.submitted_by_actor,
                base_revision: previous.base_revision,
                candidate_revision: previous.candidate_revision,
                result_summary: previous.result_summary,
                artifact_refs: previous.artifact_refs,
                check_refs: previous.check_refs,
                evidence_refs: vec![format!("work-evidence-{}", context.event_id)],
                known_risks: previous.known_risks,
                created_at: context.created_at.clone(),
            }]
        } else {
            Vec::new()
        };
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({ "reason": "github_ci_poll" }),
            Vec::new(),
            reports,
            Vec::new(),
        )
    }

    /// Host-side auto-submit when the daemon observes a linked pull request
    /// reach `MERGED` (issue #369 Phase 2). The Work must be `in_progress` and
    /// carry a `pull_request` link with `status == "MERGED"`; the fresh link
    /// snapshot is stored with the transition. Host acceptance still moves the
    /// Work from `review` to `done`; this only automates the submission step.
    pub fn submit_work_on_pr_merge(
        &self,
        work_id: &str,
        expected_version: u64,
        result_summary: &str,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        if !github_links.iter().any(|link| {
            link.kind == firm_core::GitHubLinkKind::PullRequest
                && link.status.as_deref() == Some("MERGED")
        }) {
            return Err(StoreError::Conflict(
                "PR_MERGE_REQUIRED: auto-submit requires a pull_request link with status MERGED"
                    .to_string(),
            ));
        }
        let report_id = format!("work-report-{}", context.event_id);
        let evidence_id = format!("work-evidence-{}", context.event_id);
        let candidate_revision =
            canonical_work_candidate_revision(result_summary, &[], &[], &github_links);
        let report = WorkReport {
            id: report_id,
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            report_revision: 1,
            submitted_by_actor: context.performed_by_actor.clone(),
            base_revision: None,
            candidate_revision,
            result_summary: result_summary.to_string(),
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            evidence_refs: vec![evidence_id],
            known_risks: Vec::new(),
            created_at: context.created_at.clone(),
        };
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Submitted,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Review, WorkCondition::Normal),
            serde_json::json!({ "reason": "github_pr_merge_observed" }),
            Vec::new(),
            vec![report],
            |work| {
                work.result_summary = Some(result_summary.to_string());
                // The fresh observed PR snapshot replaces the prior candidate;
                // durable issue provenance is carried forward.
                let mut merged = work
                    .github_links
                    .iter()
                    .filter(|link| link.kind == firm_core::GitHubLinkKind::Issue)
                    .cloned()
                    .collect::<Vec<_>>();
                for link in github_links {
                    if !merged.contains(&link) {
                        merged.push(link);
                    }
                }
                work.github_links = merged;
                work.blocker_reason = None;
            },
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
        next.phase = WorkPhase::Active;
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
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Cancelled,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already terminal"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.phase = WorkPhase::Closed;
        next.condition = WorkCondition::Normal;
        next.resolution = Some(WorkResolution::Cancelled);
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Cancelled, context)
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
        if (current.phase, current.condition) != required_lifecycle
            || current.active_member_run_id.as_deref() != Some(member_run_id)
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not own active work {work_id} in required state"
            )));
        }
        // A Closed or Retired ProviderRuntimeProjection no longer mutates its owned Work:
        // unfinished Work moves only via Host reassign/cancel or after an
        // explicit Reopen (docs/product/agent-team-works.md). This aligns
        // member-side transitions with insert/claim/start/receive, which
        // already require active coordination.
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
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
        if current.active_member_run_id.is_none() || current.owner_member_id.is_none() {
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
                if current.active_member_run_id.as_deref() != Some(member_run_id) {
                    return Err(StoreError::Conflict(format!(
                        "ProviderRuntimeProjection {member_run_id} does not own open work {work_id}"
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
        let prereq_event_id = context.event_id.clone();
        let prereq_created_at = context.created_at.clone();
        let deliveries = if matches!(
            kind,
            WorkEventKind::Assigned
                | WorkEventKind::ChangesRequested
                | WorkEventKind::Resumed
                | WorkEventKind::Rebound
                | WorkEventKind::ExecutionRetargeted
                | WorkEventKind::Accepted
                | WorkEventKind::Cancelled
        ) {
            self.initial_work_deliveries_unlocked(&next, &context.event_id, &context.created_at)?
        } else {
            Vec::new()
        };
        let mut next_delivery_update_sequence =
            self.next_work_delivery_update_sequence_unlocked()?;
        let delivery_updates = self
            .latest_work_deliveries_unlocked()?
            .into_values()
            .filter(|delivery| {
                delivery.work_id == current.id
                    && delivery.status == ProviderWorkDispatchStatus::Queued
                    && delivery.work_version < next.version
            })
            .map(|delivery| {
                let update_sequence = next_delivery_update_sequence;
                next_delivery_update_sequence = next_delivery_update_sequence.saturating_add(1);
                ProviderWorkDispatchUpdate {
                    delivery_id: delivery.id,
                    update_sequence,
                    status: ProviderWorkDispatchStatus::Invalidated,
                    attempt: delivery.attempt,
                    claim_id: delivery.claim_id,
                    claimed_by_supervisor_id: delivery.claimed_by_supervisor_id,
                    claimed_generation: delivery.claimed_generation,
                    provider_receipt_id: delivery.provider_receipt_id,
                    failure_reason: delivery.failure_reason,
                    updated_at: context.created_at.clone(),
                }
            })
            .collect();
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
            deliveries,
            delivery_updates,
            delegation_revisions,
        };
        self.append_work_operation_unlocked(&operation)?;
        // When a work is accepted (Done), notify works that depend on it
        // as a prerequisite: create deliveries for their owner members.
        if kind == WorkEventKind::Accepted {
            let team_run_id = &next.team_run_id;
            let prerequisite_id = &next.id;
            let all_works = self.latest_works_unlocked()?;
            for dependent_work in all_works.values() {
                if dependent_work.team_run_id == *team_run_id
                    && dependent_work
                        .prerequisite_work_ids
                        .iter()
                        .any(|pid| pid == prerequisite_id)
                    && !dependent_work.is_terminal()
                {
                    if let Some(owner_member_id) = dependent_work.active_member_run_id.as_deref() {
                        if let Ok(member) =
                            self.require_member_run_unlocked(owner_member_id, team_run_id)
                        {
                            if self
                                .ensure_member_can_receive_work_unlocked(&member)
                                .is_ok()
                            {
                                let dep_delivery = ProviderWorkDispatch {
                                    id: format!(
                                        "work-delivery-prereq-{}-{}",
                                        prereq_event_id, dependent_work.id
                                    ),
                                    work_event_id: prereq_event_id.clone(),
                                    team_run_id: team_run_id.clone(),
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    recipient_member_run_id: owner_member_id.to_string(),
                                    status: ProviderWorkDispatchStatus::Queued,
                                    attempt: 0,
                                    claim_id: None,
                                    claimed_by_supervisor_id: None,
                                    claimed_generation: None,
                                    provider_receipt_id: None,
                                    failure_reason: None,
                                    updated_at: prereq_created_at.clone(),
                                };
                                self.append_jsonl_unlocked("work_deliveries.jsonl", &dep_delivery)?;
                                // Also ensure HostAttention for prerequisite completion
                                let prereq_attention = HostAttention {
                                    id: format!("host-attention-prereq-{}", dep_delivery.id),
                                    team_run_id: team_run_id.clone(),
                                    kind: HostAttentionKind::WorkPrerequisiteCompleted,
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    source_event_ref: prereq_event_id.clone(),
                                    member_run_id: Some(owner_member_id.to_string()),
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
                                    created_at: prereq_created_at.clone(),
                                    updated_at: prereq_created_at.clone(),
                                };
                                prereq_attention
                                    .validate()
                                    .map_err(|error| StoreError::Conflict(error.to_string()))?;
                                self.append_jsonl_unlocked(
                                    "host_attentions.jsonl",
                                    &prereq_attention,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        self.ensure_host_attention_for_work_operation_unlocked(&operation)?;
        Ok(next)
    }
}
