use super::*;
use crate::store_work_journal_writer::{command_space, WorkCommandAuthority, WorkCommandEntrance};

impl HarnessStore {
    fn work_is_assigned_to_member_without_active_binding_unlocked(
        &self,
        work: &Work,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<bool> {
        if work.owner_member_id.as_deref() != Some(member.agent_member_id.as_str()) {
            return Ok(false);
        }
        let (Some(membership_id), Some(team_id)) = (
            work.assignee_membership_id.as_deref(),
            work.accountable_team_id.as_deref(),
        ) else {
            return Ok(false);
        };
        let mut matches = Vec::new();
        for space_id in self.canonical_execution_space_ids()? {
            matches.extend(
                self.fabric_team_memberships(&space_id)?
                    .into_iter()
                    .filter(|membership| membership.id == membership_id)
                    .map(|membership| (space_id.clone(), membership)),
            );
        }
        let [(space_id, membership)] = matches.as_slice() else {
            return Ok(false);
        };
        if membership.team_id != team_id
            || membership.agent_member_id != member.agent_member_id
            || membership.state != firm_core::agentfirm_api::TeamMembershipStatus::Active
        {
            return Ok(false);
        }
        Ok(!self
            .fabric_work_execution_bindings(space_id)?
            .into_iter()
            .any(|binding| {
                binding.work_id == work.id
                    && binding.team_membership_id == membership.id
                    && binding.agent_member_id == member.agent_member_id
                    && binding.status
                        == firm_core::agentfirm_api::WorkExecutionBindingStatus::Active
            }))
    }

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
        let mutation_context = match self.enter_work_command_for_run_unlocked(
            &work.id,
            &work.team_run_id,
            0,
            WorkEventKind::Created,
            WorkCommandAuthority::HostOrMemberRuntime,
            &context,
            &serde_json::json!({
                "work_id": work.id,
                "team_run_id": work.team_run_id,
                "title": work.title,
                "completion_criteria_markdown": work.completion_criteria_markdown,
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
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
        let (performed_by_actor, executed_by_member_run_id) =
            self.persisted_work_performer_unlocked(&context.performed_by_actor, &work.team_run_id);
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: work.team_run_id.clone(),
                work_id: work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload: serde_json::Value::Null,
                created_at: context.created_at,
                executed_by_member_run_id,
            },
            work: work.clone(),
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
        };
        self.validate_work_operation_records_unlocked(&operation)?;
        self.commit_current_work_mutation_unlocked(
            &mutation_context,
            WorkEventKind::Created.canonical_transition(),
            serde_json::Value::Null,
            &work,
            vec![serde_json::to_value(&operation)?],
            Vec::new(),
        )?;
        Ok(work)
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Assigned,
            WorkCommandAuthority::Host,
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "membership_id": membership_id,
                "execution_space_id": execution_space_id,
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
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work cannot be assigned; export or verify it without creating current authority"
                    .to_string(),
            ));
        }
        require_mutable_work(
            &current,
            "create a new Work instead of reassigning a terminal one",
        )?;
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
            &mutation_context,
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Updated,
            WorkCommandAuthority::Host,
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "reason": "mixed_version_projection_recovery",
            }),
        )? {
            WorkCommandEntrance::Replayed(work) => return Ok(*work),
            WorkCommandEntrance::Admitted(mutation_context) => mutation_context,
        };
        require_host_actor(&context.performed_by_actor)?;
        // Deliberately the legacy ledger fold: this verb repairs a sparse row
        // a stale binary appended to `work_operations.jsonl`, and the sparse
        // row is the thing it is asked about.
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
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
        require_mutable_work(
            &current,
            "a closed Work keeps the provenance it settled with",
        )?;
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
            &mutation_context,
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
        // The replay fence that used to be hand-written here — same Host, same
        // Work version, same successor — is now the entrance's caller-shape
        // gate plus the trust kernel's request fingerprint over exactly those
        // three facts and the authenticated actor the envelope records.
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::ExecutionRetargeted,
            WorkCommandAuthority::Host,
            &context,
            &serde_json::json!({
                "work_id": work_id,
                "expected_version": expected_version,
                "successor_team_run_id": successor_team_run_id,
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
        self.require_exact_team_run_host_actor(&context.performed_by_actor, &current.team_run_id)?;
        if current.active_member_run_id.is_some()
            || (current.owner_member_id.is_some() && current.assignee_membership_id.is_none())
        {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work cannot be retargeted; export or verify it without creating current authority"
                    .to_string(),
            ));
        }
        require_mutable_work(
            &current,
            "create a new Work instead of retargeting a terminal one",
        )?;
        // The versioned Host decision is the intake action. Notification
        // transport/ACK state never gates a Work transition (ADR 0064, S9).
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
            &mutation_context,
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Claimed,
            WorkCommandAuthority::MemberRun(member_run_id),
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
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(
            &command_space(&mutation_context),
            work_id,
            expected_version,
        )?;
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
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::Claimed,
            context,
            &mutation_context,
        )
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
        let mutation_context = match self.enter_work_command_unlocked(
            work_id,
            expected_version,
            WorkEventKind::Started,
            WorkCommandAuthority::MemberRun(member_run_id),
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
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: historical runtime-owned Work is read/export evidence and cannot be started"
                    .to_string(),
            ));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        // One active Work per member is a member-plane fact that needs no
        // delivery evidence, so it is answered before the responsibility and
        // WorkDelivery guards. Otherwise a busy member asking to start a
        // second, still-undispatched Work would be told its delivery evidence
        // is missing instead of that it is already busy.
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
        if current.phase == WorkPhase::Open
            && current.condition == WorkCondition::Normal
            && self.work_is_assigned_to_member_without_active_binding_unlocked(&current, &member)?
        {
            return Err(super::trust_kernel::retryable_trust_error(
                firm_core::agentfirm_api::TrustErrorCode::DeliveryNotDispatched,
                "Work is assigned to you but not yet dispatched by the Supervisor; end this provider turn without sleep or retry loops, then re-read and start after a new Work delivery",
                "work",
                work_id,
                Some(current.version),
            ));
        }
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
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        let mut next = current.clone();
        next.phase = WorkPhase::Active;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::Started,
            context,
            &mutation_context,
        )
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
