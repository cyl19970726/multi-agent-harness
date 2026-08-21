use super::*;

impl HarnessStore {
    pub(super) fn require_team_run_unlocked(&self, team_run_id: &str) -> StoreResult<AgentTeamRun> {
        latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(team_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))
    }

    pub(super) fn latest_host_binding_lease_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<HostBindingLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<HostBindingLease>("host_binding_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id))
    }

    pub(super) fn require_same_host_binding_lease_owner(
        &self,
        current: &HostBindingLease,
        expected: &HostBindingLease,
    ) -> StoreResult<()> {
        if current.team_run_id != expected.team_run_id
            || canonical_surface(&current.host_surface) != canonical_surface(&expected.host_surface)
            || current.host_thread_id != expected.host_thread_id
            || current.owner_kind != expected.owner_kind
            || current.owner_id != expected.owner_id
            || current.generation != expected.generation
            || current.lease_id != expected.lease_id
        {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_LEASE_FENCED: stale lease owner/generation/id for TeamRun {}",
                expected.team_run_id
            )));
        }
        Ok(())
    }

    pub(super) fn require_current_host_binding_lease_owner_unlocked(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        let current = self
            .latest_host_binding_lease_unlocked(&expected.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TeamRun {} has no Host binding lease",
                    expected.team_run_id
                ))
            })?;
        self.require_same_host_binding_lease_owner(&current, expected)?;
        if !current.is_effective_at(now_unix_ms) {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_LEASE_FENCED: lease for TeamRun {} is released or expired",
                expected.team_run_id
            )));
        }
        Ok(current)
    }

    pub(super) fn require_host_attention_lease_fence_unlocked(
        &self,
        attention: &HostAttention,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        let current = self
            .latest_host_binding_lease_unlocked(&attention.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_ATTENTION_LEASE_FENCED: TeamRun {} has no Host binding lease",
                    attention.team_run_id
                ))
            })?;
        let matches = current.owner_kind == HostBindingLeaseOwnerKind::Dispatcher
            && attention.claimed_host_lease_id.as_deref() == Some(current.lease_id.as_str())
            && attention.claimed_host_lease_generation == Some(current.generation)
            && attention.claimed_host_lease_owner_id.as_deref() == Some(current.owner_id.as_str())
            && attention
                .claimed_host_surface
                .as_deref()
                .is_some_and(|surface| {
                    canonical_surface(surface) == canonical_surface(&current.host_surface)
                })
            && attention.claimed_host_thread_id.as_deref() == Some(current.host_thread_id.as_str())
            && current.is_effective_at(now_unix_ms);
        if !matches {
            return Err(StoreError::Conflict(format!(
                "HOST_ATTENTION_LEASE_FENCED: claim {} no longer owns attention {}",
                attention.claim_id.as_deref().unwrap_or("<missing>"),
                attention.id
            )));
        }
        Ok(current)
    }

    pub(super) fn requeue_fenced_host_attention_claims_unlocked(
        &self,
        current: &HostBindingLease,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<()> {
        let run = self.require_team_run_unlocked(&current.team_run_id)?;
        let current_is_effective_exact_dispatcher = current.owner_kind
            == HostBindingLeaseOwnerKind::Dispatcher
            && current.is_effective_at(now_unix_ms)
            && canonical_surface(&current.host_surface) == canonical_surface(&run.host_surface)
            && run.host_thread_id.as_deref() == Some(current.host_thread_id.as_str());
        let attentions = self.latest_host_attentions_unlocked()?;
        for mut attention in attentions.into_values().filter(|attention| {
            attention.team_run_id == current.team_run_id
                && attention.status == HostAttentionStatus::Claimed
                && attention.claimed_host_lease_id.is_some()
                && (!current_is_effective_exact_dispatcher
                    || attention.claimed_host_lease_id.as_deref()
                        != Some(current.lease_id.as_str())
                    || attention.claimed_host_lease_generation != Some(current.generation)
                    || attention.claimed_host_lease_owner_id.as_deref()
                        != Some(current.owner_id.as_str())
                    || attention
                        .claimed_host_surface
                        .as_deref()
                        .map(canonical_surface)
                        != Some(canonical_surface(&current.host_surface))
                    || attention.claimed_host_thread_id.as_deref()
                        != Some(current.host_thread_id.as_str()))
        }) {
            attention.status = HostAttentionStatus::Actionable;
            attention.claim_id = None;
            attention.claimed_host_surface = None;
            attention.claimed_host_thread_id = None;
            attention.claimed_host_lease_id = None;
            attention.claimed_host_lease_generation = None;
            attention.claimed_host_lease_owner_id = None;
            attention.provider_receipt_id = None;
            attention.last_failure_reason =
                Some("previous Host binding lease no longer owns this attention".to_string());
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        }
        Ok(())
    }

    pub(super) fn reconcile_host_binding_stale_attentions_unlocked(
        &self,
        now_unix_ms: u64,
        observed_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        let leases = latest_by_id(
            self.read_jsonl::<HostBindingLease>("host_binding_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        );
        let mut projected = self.latest_host_attentions_unlocked()?;
        let mut stale = Vec::new();
        for run in runs.into_values() {
            if matches!(
                run.status,
                TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
            ) {
                continue;
            }
            let Some(thread_id) = run.host_thread_id.as_deref() else {
                continue;
            };
            let lease = leases.get(&run.id);
            let effective = lease.is_some_and(|lease| {
                lease.is_effective_at(now_unix_ms)
                    && canonical_surface(&lease.host_surface)
                        == canonical_surface(&run.host_surface)
                    && lease.host_thread_id == thread_id
            });
            if effective {
                continue;
            }
            let generation = lease.map(|lease| lease.generation).unwrap_or(0);
            let source_event_ref = format!(
                "host-binding-stale:{}:{}:{}:generation:{}",
                run.id, run.host_surface, thread_id, generation
            );
            let attention = HostAttention {
                id: format!("host-attention-{source_event_ref}"),
                team_run_id: run.id,
                kind: HostAttentionKind::HostBindingStale,
                work_id: String::new(),
                work_version: 0,
                source_event_ref,
                member_run_id: None,
                status: HostAttentionStatus::Actionable,
                attempt: 0,
                claim_id: None,
                claimed_host_surface: None,
                claimed_host_thread_id: None,
                claimed_host_lease_id: None,
                claimed_host_lease_generation: None,
                claimed_host_lease_owner_id: None,
                provider_receipt_id: None,
                last_failure_reason: None,
                created_at: observed_at.to_string(),
                updated_at: observed_at.to_string(),
            };
            if let Some(existing) = projected.get(&attention.id) {
                stale.push(existing.clone());
                continue;
            }
            attention
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            projected.insert(attention.id.clone(), attention.clone());
            stale.push(attention);
        }
        Ok(stale)
    }

    pub(super) fn ensure_host_attention_unlocked(
        &self,
        attention: &HostAttention,
    ) -> StoreResult<HostAttention> {
        if attention.kind == HostAttentionKind::HostBindingStale {
            return Err(StoreError::Conflict(
                "HostBindingStale attention is derived by lease reconciliation".to_string(),
            ));
        }
        attention
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if attention.status != HostAttentionStatus::Actionable
            || attention.attempt != 0
            || attention.claim_id.is_some()
            || attention.claimed_host_surface.is_some()
            || attention.claimed_host_thread_id.is_some()
            || attention.claimed_host_lease_id.is_some()
            || attention.claimed_host_lease_generation.is_some()
            || attention.claimed_host_lease_owner_id.is_some()
            || attention.provider_receipt_id.is_some()
        {
            return Err(StoreError::Conflict(
                "new HostAttention must be actionable and unclaimed".to_string(),
            ));
        }

        let mut attentions = self.latest_host_attentions_unlocked()?;
        if let Some(existing) = attentions.remove(&attention.id) {
            if Self::same_host_attention_fact(&existing, attention) {
                return Ok(existing);
            }
            return Err(StoreError::Conflict(format!(
                "HostAttention id {} already names a different causal fact",
                attention.id
            )));
        }

        self.require_team_run_unlocked(&attention.team_run_id)?;
        let source_operation = self
            .work_operations_unlocked()?
            .into_iter()
            .find(|operation| operation.event.id == attention.source_event_ref);
        if let Some(operation) = source_operation {
            if operation.event.team_run_id != attention.team_run_id
                || operation.event.work_id != attention.work_id
                || operation.event.resulting_version != attention.work_version
            {
                return Err(StoreError::Conflict(format!(
                    "HostAttention {} does not match source WorkEvent {}",
                    attention.id, attention.source_event_ref
                )));
            }
        } else {
            // Member-runtime attention can be caused by a TeamRun/provider
            // event rather than a WorkEvent. Validate that its current Work
            // subject still resolves inside the named TeamRun.
            let work = self
                .latest_works_unlocked()?
                .remove(&attention.work_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("work not found: {}", attention.work_id))
                })?;
            if work.team_run_id != attention.team_run_id {
                return Err(StoreError::Conflict(format!(
                    "Work {} does not belong to TeamRun {}",
                    attention.work_id, attention.team_run_id
                )));
            }
            if work.version < attention.work_version {
                return Err(StoreError::Conflict(format!(
                    "HostAttention references future Work version {} > {}",
                    attention.work_version, work.version
                )));
            }
        }
        if let Some(member_run_id) = attention.member_run_id.as_deref() {
            self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
        }

        self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        Ok(attention.clone())
    }

    pub(super) fn same_host_attention_fact(left: &HostAttention, right: &HostAttention) -> bool {
        left.team_run_id == right.team_run_id
            && left.kind == right.kind
            && left.work_id == right.work_id
            && left.work_version == right.work_version
            && left.source_event_ref == right.source_event_ref
            && left.member_run_id == right.member_run_id
            && left.created_at == right.created_at
    }

    pub(super) fn host_attention_for_work_operation(
        operation: &WorkOperation,
    ) -> Option<HostAttention> {
        let kind = match operation.event.kind {
            WorkEventKind::Submitted => HostAttentionKind::WorkReviewRequested,
            WorkEventKind::Blocked => HostAttentionKind::WorkBlocked,
            WorkEventKind::Accepted => HostAttentionKind::WorkAccepted,
            WorkEventKind::ChangesRequested => HostAttentionKind::WorkChangesRequested,
            WorkEventKind::Cancelled => HostAttentionKind::WorkCancelled,
            _ => return None,
        };
        Some(HostAttention {
            id: format!("host-attention-{}", operation.event.id),
            team_run_id: operation.event.team_run_id.clone(),
            kind,
            work_id: operation.event.work_id.clone(),
            work_version: operation.event.resulting_version,
            source_event_ref: operation.event.id.clone(),
            member_run_id: operation.work.active_member_run_id.clone(),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: operation.event.created_at.clone(),
            updated_at: operation.event.created_at.clone(),
        })
    }

    pub(super) fn ensure_host_attention_for_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<Option<HostAttention>> {
        Self::host_attention_for_work_operation(operation)
            .map(|attention| self.ensure_host_attention_unlocked(&attention))
            .transpose()
    }

    pub(super) fn reconcile_work_host_attentions_unlocked(
        &self,
    ) -> StoreResult<Vec<HostAttention>> {
        let operations = self.work_operations_unlocked()?;
        let mut projected = self.latest_host_attentions_unlocked()?;
        let mut reconciled = Vec::new();
        for operation in &operations {
            let Some(attention) = Self::host_attention_for_work_operation(operation) else {
                continue;
            };
            if let Some(existing) = projected.get(&attention.id) {
                if !Self::same_host_attention_fact(existing, &attention) {
                    return Err(StoreError::Conflict(format!(
                        "HostAttention id {} already names a different causal fact",
                        attention.id
                    )));
                }
                reconciled.push(existing.clone());
                continue;
            }
            attention
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            self.require_team_run_unlocked(&attention.team_run_id)?;
            if let Some(member_run_id) = attention.member_run_id.as_deref() {
                self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
            }
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            projected.insert(attention.id.clone(), attention.clone());
            reconciled.push(attention);
        }
        Ok(reconciled)
    }

    pub(super) fn host_attention_inbox_for_team_run_unreconciled(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        let attentions = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| attention.team_run_id == team_run_id)
            .filter(|attention| include_all || attention.needs_host_action())
            .collect::<Vec<_>>();
        let warning = if run.host_thread_id.is_none() && !attentions.is_empty() {
            Some(format!(
                "UNBOUND_HOST: TeamRun {} has actionable Host attention but no exact native Host task; bind host_surface + host_thread_id before delivery",
                run.id
            ))
        } else {
            None
        };
        Ok(HostAttentionInbox {
            team_run_id: run.id,
            host_surface: run.host_surface,
            host_thread_id: run.host_thread_id,
            warning,
            attentions,
        })
    }

    pub(super) fn latest_host_attentions_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, HostAttention>> {
        Ok(latest_by_id(
            self.read_jsonl::<HostAttention>("host_attentions.jsonl")?,
            |attention| attention.id.clone(),
        ))
    }

    pub(super) fn require_host_attention_unlocked(
        &self,
        attention_id: &str,
    ) -> StoreResult<HostAttention> {
        self.latest_host_attentions_unlocked()?
            .remove(attention_id)
            .ok_or_else(|| StoreError::Conflict(format!("HostAttention not found: {attention_id}")))
    }

    pub(super) fn require_exact_host_binding_unlocked(
        &self,
        team_run_id: &str,
        host_surface: &str,
        host_thread_id: &str,
    ) -> StoreResult<AgentTeamRun> {
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        let run = self.require_team_run_unlocked(team_run_id)?;
        if canonical_surface(&run.host_surface) != canonical_surface(host_surface)
            || run.host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_MISMATCH: TeamRun {team_run_id} is not bound to {host_surface}/{host_thread_id}"
            )));
        }
        Ok(run)
    }

    pub(super) fn require_member_run_unlocked(
        &self,
        member_run_id: &str,
        team_run_id: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {member_run_id}")))?;
        if member.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not belong to TeamRun {team_run_id}"
            )));
        }
        Ok(member)
    }

    /// Resolve a runtime only when the latest TeamRun explicitly names it as a
    /// member. A same-team ProviderRuntimeProjection row is not membership authority: the
    /// append-only ledger can contain stale or forged rows that were never
    /// admitted to the TeamRun.
    #[cfg(test)]
    pub(super) fn ensure_unique_member_identity_unlocked(
        &self,
        team_run: &AgentTeamRun,
        proposed: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        let identity = member_identity(proposed);
        let members = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        );
        if let Some(existing) = team_run
            .member_run_ids
            .iter()
            .filter_map(|id| members.get(id))
            .find(|member| member_identity(member) == identity)
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_IDENTITY_CONFLICT: stable identity {identity} is already admitted as ProviderRuntimeProjection {}",
                existing.id
            )));
        }
        Ok(())
    }

    pub(super) fn ensure_member_admission_identity_unlocked(
        &self,
        team_run: &AgentTeamRun,
        proposed: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        let identity = member_identity(proposed);
        let members = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        );
        let candidates = team_run
            .member_run_ids
            .iter()
            .filter_map(|id| members.get(id))
            .filter(|member| member_identity(member) == identity)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(());
        }
        let max_generation = candidates
            .iter()
            .map(|member| member.runtime_generation)
            .max()
            .unwrap_or(0);
        if candidates
            .iter()
            .any(|member| member_is_active_reviewer_runtime(member))
            || proposed.runtime_generation <= max_generation
            || candidates.iter().any(|member| {
                member.provider != proposed.provider
                    || member.role != proposed.role
                    || member.agent_member_id != proposed.agent_member_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_IDENTITY_CONFLICT: stable identity {identity} is already admitted and is not a closed lower-generation runtime"
            )));
        }
        Ok(())
    }

    /// A stable reviewer identity is trustworthy only when it resolves to one
    /// exact runtime in the latest TeamRun membership. Reject duplicate stable
    /// identities instead of choosing whichever ProviderRuntimeProjection happened to be
    /// loaded first.
    pub(super) fn validate_work_relations_unlocked(&self, work: &Work) -> StoreResult<()> {
        let works = self.latest_works_unlocked()?;
        for prerequisite_id in &work.prerequisite_work_ids {
            let prerequisite = works.get(prerequisite_id).ok_or_else(|| {
                StoreError::Conflict(format!("prerequisite work not found: {prerequisite_id}"))
            })?;
            if !works_share_scope(prerequisite, work) || prerequisite.id == work.id {
                return Err(StoreError::Conflict(
                    "prerequisites must be distinct Works in the same durable Team scope"
                        .to_string(),
                ));
            }
        }
        if let Some(parent_id) = work.parent_work_id.as_deref() {
            let parent = works.get(parent_id).ok_or_else(|| {
                StoreError::Conflict(format!("parent work not found: {parent_id}"))
            })?;
            if !works_share_scope(parent, work) || parent.id == work.id {
                return Err(StoreError::Conflict(
                    "parent_work_id must reference a distinct Work in the same durable Team scope"
                        .to_string(),
                ));
            }
        }
        if let Some(member_run_id) = work.active_member_run_id.as_deref() {
            let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
            self.ensure_member_can_receive_work_unlocked(&member)?;
            if work.owner_member_id.as_deref() != Some(member_identity(&member).as_str()) {
                return Err(StoreError::Conflict(
                    "owner_member_id does not match active ProviderRuntimeProjection stable identity".to_string(),
                ));
            }
        }
        // DOC-106: responsibility never depends on an active MemberRun or
        // runtime. `assignee_membership_id` (mirrored by `owner_member_id`) is
        // durable and survives Close/Reopen; only the transient execution
        // binding above is runtime-fenced.
        Ok(())
    }

    pub(super) fn ensure_member_can_receive_work_unlocked(
        &self,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        if !member.coordination_is_active()
            || matches!(
                member.status,
                firm_core::MemberRunStatus::Stopped | firm_core::MemberRunStatus::Failed
            )
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: ProviderRuntimeProjection {} cannot receive Work while {:?}/{:?}",
                member.id, member.coordination_status, member.status
            )));
        }
        Ok(())
    }

    /// Resolve the assignee TeamMembership for one (accountable Team,
    /// AgentMember) pair without ever guessing. Exactly one Active membership
    /// binds; with no Active row exactly one historical membership binds; zero
    /// rows, multiple Active rows, multiple historical rows, or a multi-space
    /// trust fabric resolve to `None` and stay visible for the responsibility
    /// migration report instead of being silently inferred.
    pub(super) fn resolve_assignee_membership_id_unlocked(
        &self,
        accountable_team_id: Option<&str>,
        agent_member_id: &str,
    ) -> StoreResult<Option<String>> {
        let Some(team_id) = accountable_team_id else {
            return Ok(None);
        };
        if agent_member_id.is_empty() {
            return Ok(None);
        }
        let spaces = self.canonical_execution_space_ids()?;
        let [space_id] = spaces.as_slice() else {
            return Ok(None);
        };
        let matching = self
            .fabric_team_memberships(space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team_id && membership.agent_member_id == agent_member_id
            })
            .collect::<Vec<_>>();
        let active = matching
            .iter()
            .filter(|membership| {
                membership.state == firm_core::agentfirm_api::TeamMembershipStatus::Active
            })
            .collect::<Vec<_>>();
        if active.len() == 1 {
            return Ok(Some(active[0].id.clone()));
        }
        if active.is_empty() && matching.len() == 1 {
            return Ok(Some(matching[0].id.clone()));
        }
        Ok(None)
    }
}
