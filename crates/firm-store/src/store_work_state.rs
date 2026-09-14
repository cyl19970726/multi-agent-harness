use super::*;
use crate::store_work_journal_writer::{command_space, WorkCommandAuthority, WorkCommandEntrance};

/// Who is returning a submitted Work for changes. The Store keeps the two
/// authorities separate so the Host gate is never relaxed to admit a peer:
/// each arm proves its own exact identity before the shared transition runs.
enum WorkChangesReviewer {
    Host,
    ExactTeamPeer { member_run_id: String },
}

impl HarnessStore {
    /// The exact active non-owner Team peer that may review one Host-owned
    /// Work. Mirrors the acceptance rule in `work_review_authorized`: the Work
    /// must be owned by this Team's Host, the reviewer must hold exactly one
    /// Active TeamMembership of that Team, and exactly one active MemberRun in
    /// the Work's TeamRun. Any other shape fails closed.
    fn require_exact_host_owned_work_peer_reviewer_unlocked(
        &self,
        work: &Work,
        member_run_id: &str,
    ) -> StoreResult<()> {
        let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
        if !member_is_active_reviewer_runtime(&member) {
            return Err(StoreError::Conflict(format!(
                "WORK_REVIEW_NOT_AUTHORIZED: MemberRun {member_run_id} is not an active reviewer runtime"
            )));
        }
        let team_id = work.accountable_team_id.as_deref().ok_or_else(|| {
            StoreError::Conflict(
                "WORK_REVIEW_NOT_AUTHORIZED: Work has no accountable AgentTeam".to_string(),
            )
        })?;
        let team = self.latest_teams()?.remove(team_id).ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_REVIEW_NOT_AUTHORIZED: accountable AgentTeam {team_id} not found"
            ))
        })?;
        if work.owner_member_id.as_deref() != Some(team.host_agent_id.as_str()) {
            return Err(StoreError::Conflict(
                "WORK_REVIEW_NOT_AUTHORIZED: only Host-owned Work admits a Team peer reviewer; Member Work stays Host-reviewed"
                    .to_string(),
            ));
        }
        if work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str()) {
            return Err(StoreError::Conflict(
                "WORK_REVIEW_NOT_AUTHORIZED: the accountable Work owner cannot review its own candidate"
                    .to_string(),
            ));
        }
        // Count memberships only inside the Work's own TeamRun Execution
        // Space. A physical Store may temporarily hold more than one space
        // during recovery/import; folding them together would let another
        // scope's row grant review authority here, or let a duplicate row over
        // there withdraw it.
        let run = self.require_team_run_unlocked(&work.team_run_id)?;
        let execution_space_id = self.require_team_run_execution_space_unlocked(&run)?;
        let memberships = self
            .fabric_team_memberships(&execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.node_id == team.node_id
                    && membership.agent_member_id == member.agent_member_id
                    && membership.state == firm_core::agentfirm_api::TeamMembershipStatus::Active
            })
            .collect::<Vec<_>>();
        if memberships.len() != 1 {
            return Err(StoreError::Conflict(format!(
                "WORK_REVIEW_NOT_AUTHORIZED: expected exactly one Active TeamMembership for the reviewer, found {}",
                memberships.len()
            )));
        }
        Ok(())
    }

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
    /// Result or touching execution authority. The authenticated NodeDaemon,
    /// with the exact TeamRun Host as its authority source, may update only the
    /// Work's external evidence snapshot and append its `Updated` revision;
    /// lifecycle, responsibility, reports, attention, bindings and deliveries
    /// remain independent.
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
        let request = serde_json::json!({
            "work_id": work_id,
            "expected_version": expected_version,
            "github_links": github_links,
            "execution_space_id": execution_space_id,
            "node_id": daemon.node_id,
            "daemon_id": daemon.daemon_id,
            "daemon_generation": daemon.generation,
        });
        let request_fingerprint = canonical_json_fingerprint(&request);
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Updated,
            WorkCommandAuthority::CheckedByCommand,
            &context,
            &request,
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
        // External CI evidence never reopens a settled responsibility. The
        // daemon poll skips terminal Work, so reaching here is a caller bug.
        require_mutable_work(
            &current,
            "external GitHub evidence cannot advance a closed Work revision",
        )?;
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
            &mutation_context,
            serde_json::json!({
                "reason": "github_evidence_refresh",
                "request_fingerprint": request_fingerprint,
            }),
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
            "LEGACY_WORK_ACCEPT_RETIRED: this local writer is closed; accept through `firm member work accept` (Supervisor-bound), `firm team-run work accept` (local Host), or `firm member-trust mutate --json '{\"AcceptWork\":...}'`"
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
            "LEGACY_WORK_ACCEPT_RETIRED: this local writer is closed; accept through `firm member work accept` (Supervisor-bound), `firm team-run work accept` (local Host), or `firm member-trust mutate --json '{\"AcceptWork\":...}'`"
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
        self.request_work_changes_by_reviewer(
            work_id,
            expected_version,
            reason,
            WorkChangesReviewer::Host,
            context,
        )
    }

    /// Peer review of Host-owned Work. Ordinary Member Work stays Host-reviewed;
    /// Host-owned Work has no Host reviewer, so exactly one active non-owner
    /// Team peer may return it for changes — the same authority that already
    /// accepts Host-owned Work (`work_review_authorized`). This never widens
    /// beyond Host-owned Work and never lets the owner review its own
    /// candidate.
    pub fn request_work_changes_as_peer_reviewer(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.request_work_changes_by_reviewer(
            work_id,
            expected_version,
            reason,
            WorkChangesReviewer::ExactTeamPeer {
                member_run_id: member_run_id.to_string(),
            },
            context,
        )
    }

    fn request_work_changes_by_reviewer(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        reviewer: WorkChangesReviewer,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "changes-requested reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::ChangesRequested,
            match &reviewer {
                WorkChangesReviewer::Host => WorkCommandAuthority::Host,
                WorkChangesReviewer::ExactTeamPeer { member_run_id } => {
                    WorkCommandAuthority::MemberRun(member_run_id)
                }
            },
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "reason": reason,
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        let current = match &reviewer {
            WorkChangesReviewer::Host => {
                require_host_actor(&context.performed_by_actor)?;
                let current = self.current_work_unlocked(
                    &command_space(&mutation_context),
                    work_id,
                    expected_version,
                )?;
                self.require_exact_team_run_host_actor(
                    &context.performed_by_actor,
                    &current.team_run_id,
                )?;
                current
            }
            WorkChangesReviewer::ExactTeamPeer { member_run_id } => {
                require_member_actor(&context.performed_by_actor, member_run_id)?;
                let current = self.current_work_unlocked(
                    &command_space(&mutation_context),
                    work_id,
                    expected_version,
                )?;
                self.require_exact_host_owned_work_peer_reviewer_unlocked(&current, member_run_id)?;
                current
            }
        };
        if current.phase != WorkPhase::Review || current.condition != WorkCondition::Normal {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must await Host acceptance"
            )));
        }
        let mut next = current.clone();
        // A submitted execution has already settled its exact binding. A
        // changes-requested review therefore returns the stable responsibility
        // to the scheduler queue; a new execution admission must Start the
        // next attempt. The transition is identical for either reviewer.
        next.phase = WorkPhase::Open;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        // Only the peer arm marks its payload. Re-authorization of the next
        // execution admission reads this marker, never "a non-Host member
        // requested changes": the two are the same set only while the runtime
        // generation is the performer, which it no longer is.
        let payload = match &reviewer {
            WorkChangesReviewer::Host => serde_json::Value::Null,
            WorkChangesReviewer::ExactTeamPeer { member_run_id } => serde_json::json!({
                crate::store_work_journal_writer::PEER_REVIEW_MARKER: true,
                "reviewer_member_run_id": member_run_id,
            }),
        };
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::ChangesRequested,
            context,
            &mutation_context,
            payload,
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
        require_mutable_work(&current, "a closed Work cannot be cancelled again")?;
        self.ensure_no_claimed_delivery_unlocked(&current)?;
        let mut next = current.clone();
        next.phase = WorkPhase::Closed;
        next.condition = WorkCondition::Normal;
        next.resolution = Some(WorkResolution::Cancelled);
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        require_valid_work_transition(&current, &next, WorkEventKind::Cancelled)?;
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
            executed_by_member_run_id: None,
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            kind,
            WorkCommandAuthority::MemberRun(member_run_id),
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "member_run_id": member_run_id,
                "payload": payload,
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be mutated"
                    .to_string(),
            ));
        }
        // A Closed or Retired ProviderRuntimeProjection no longer mutates its
        // owned Work: unfinished Work moves only via Host reassign, cancel,
        // redeliver or recover-lost-execution. There is no Reopen verb. This
        // aligns member-side transitions with insert/claim/start/receive,
        // which already require active coordination.
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
            &mutation_context,
            payload,
            condition_records,
            reports,
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            kind,
            WorkCommandAuthority::Host,
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "payload": payload,
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
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
            &mutation_context,
            payload,
            condition_records,
            reports,
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Released,
            match member_run_id {
                Some(member_run_id) => WorkCommandAuthority::MemberRun(member_run_id),
                None => WorkCommandAuthority::Host,
            },
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "member_run_id": member_run_id,
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
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
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::Released,
            context,
            &mutation_context,
        )
    }

    pub(super) fn append_work_transition_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        mutation_context: &firm_core::agentfirm_api::MutationContext,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            kind,
            context,
            mutation_context,
            serde_json::Value::Null,
        )
    }

    pub(super) fn append_work_transition_with_payload_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        mutation_context: &firm_core::agentfirm_api::MutationContext,
        payload: serde_json::Value,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            mutation_context,
            payload,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Commit one Work transition: build its complete [`WorkOperation`] and
    /// write it as the immutable side record of one `work` trust envelope.
    ///
    /// Everything that made the old ledger row crash-atomic is preserved — the
    /// transition guard, the graph outbox payload, the per-row condition
    /// records, reports, evidence and decisions, and the Delegation revisions
    /// this Work state caused — and now lands in a single atomic trust write
    /// instead of a single JSONL append. The derived HostAttention rows stay
    /// outside that write for the same reason they always were: they are
    /// deterministic from the operation and replay-repairable.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_work_transition_with_records_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        mutation_context: &firm_core::agentfirm_api::MutationContext,
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
    ) -> StoreResult<Work> {
        require_valid_work_transition(&current, &next, kind)?;
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let sequence = self
            .work_journal_unlocked()?
            .records
            .iter()
            .filter(|record| record.work.id == current.id)
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
        let (performed_by_actor, executed_by_member_run_id) =
            self.persisted_work_performer_unlocked(&context.performed_by_actor, &next.team_run_id);
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: next.team_run_id.clone(),
                work_id: next.id.clone(),
                sequence,
                kind,
                expected_version: current.version,
                resulting_version: next.version,
                performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload: payload.clone(),
                created_at: context.created_at,
                executed_by_member_run_id,
            },
            work: next.clone(),
            condition_records,
            reports,
            evidence_records,
            delegation_revisions,
        };
        self.validate_work_operation_records_unlocked(&operation)?;
        // The complete WorkOperation is the Work journal row. Its Delegation
        // revisions are ALSO committed as their own side records, in the same
        // atomic write, because the Delegation reader resolves revisions by
        // shape across every trust envelope and must not have to know that one
        // of them is nested inside a Work row.
        let mut side_records = vec![serde_json::to_value(&operation)?];
        for revision in &operation.delegation_revisions {
            side_records.push(serde_json::to_value(revision)?);
        }
        self.commit_current_work_mutation_unlocked(
            mutation_context,
            kind.canonical_transition(),
            payload,
            &next,
            side_records,
            Vec::new(),
        )?;
        // The outbox itself is in the crash-atomic WorkOperation. Materialized
        // HostAttention rows are deterministic and replay-repairable.
        self.ensure_downstream_host_attentions_for_work_operation_unlocked(&operation)?;
        self.ensure_host_attention_for_work_operation_unlocked(&operation)?;
        Ok(next)
    }
}
