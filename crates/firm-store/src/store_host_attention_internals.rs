use super::*;
use firm_application::HostRuntimeBinding;

impl HarnessStore {
    pub(super) fn require_managed_host_attention_fence_unlocked(
        &self,
        attention: &HostAttention,
        observed_at: &str,
    ) -> StoreResult<()> {
        let observed_unix_ms = parse_iso8601_to_unix_ms(observed_at).ok_or_else(|| {
            StoreError::Conflict("managed Host completion requires unix-ms updated_at".to_string())
        })?;
        let HostRuntimeBinding::Managed(binding) =
            self.host_runtime_binding_unlocked(&attention.team_run_id, observed_unix_ms)?
        else {
            return Err(StoreError::Conflict(
                "MANAGED_HOST_ATTENTION_MODE_FENCED".to_string(),
            ));
        };
        let member_run_id = attention
            .claimed_recipient_member_run_id
            .as_deref()
            .ok_or_else(|| StoreError::Conflict("managed claim has no MemberRun".to_string()))?;
        let session_id = attention
            .claimed_recipient_session_id
            .as_deref()
            .ok_or_else(|| StoreError::Conflict("managed claim has no AgentSession".to_string()))?;
        let session_generation =
            attention
                .claimed_recipient_session_generation
                .ok_or_else(|| {
                    StoreError::Conflict("managed claim has no session generation".to_string())
                })?;
        let daemon_id = attention
            .claimed_node_daemon_id
            .as_deref()
            .ok_or_else(|| StoreError::Conflict("managed claim has no NodeDaemon".to_string()))?;
        let daemon_generation = attention.claimed_node_daemon_generation.ok_or_else(|| {
            StoreError::Conflict("managed claim has no daemon generation".to_string())
        })?;
        if binding.member_run.id != member_run_id
            || binding.agent_session.id != session_id
            || binding.agent_session.runtime_generation != session_generation
            || binding.node_daemon.daemon_id != daemon_id
            || binding.node_daemon.generation != daemon_generation
        {
            return Err(StoreError::Conflict(
                "MANAGED_HOST_ATTENTION_BINDING_FENCED".to_string(),
            ));
        }
        Ok(())
    }

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
                claimed_recipient_member_run_id: None,
                claimed_recipient_session_id: None,
                claimed_recipient_session_generation: None,
                claimed_node_daemon_id: None,
                claimed_node_daemon_generation: None,
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
            let graph_reconciliation = matches!(
                attention.kind,
                HostAttentionKind::WorkPrerequisiteCompleted
                    | HostAttentionKind::WorkPrerequisiteNeedsReconciliation
            );
            let source_matches = if graph_reconciliation {
                operation
                    .event
                    .payload
                    .get("work_graph_outbox")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|records| {
                        records.iter().any(|record| {
                            record
                                .get("dependent_work_id")
                                .and_then(serde_json::Value::as_str)
                                == Some(attention.work_id.as_str())
                                && record
                                    .get("dependent_work_version")
                                    .and_then(serde_json::Value::as_u64)
                                    == Some(attention.work_version)
                                && record
                                    .get("dependent_team_run_id")
                                    .and_then(serde_json::Value::as_str)
                                    == Some(attention.team_run_id.as_str())
                        })
                    })
            } else {
                operation.event.team_run_id == attention.team_run_id
                    && operation.event.work_id == attention.work_id
                    && operation.event.resulting_version == attention.work_version
            };
            if !source_matches {
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
        firm_application::same_host_attention_identity(left, right)
    }

    pub(super) fn host_attention_for_work_operation(
        operation: &WorkOperation,
    ) -> Option<HostAttention> {
        if operation.event.performed_by_actor.kind == TeamActorKind::Host {
            return None;
        }
        let kind = match operation.event.kind {
            WorkEventKind::Submitted => HostAttentionKind::WorkReviewRequested,
            WorkEventKind::Blocked => HostAttentionKind::WorkBlocked,
            WorkEventKind::Accepted => HostAttentionKind::WorkAccepted,
            WorkEventKind::ChangesRequested => HostAttentionKind::WorkChangesRequested,
            WorkEventKind::Cancelled => HostAttentionKind::WorkCancelled,
            WorkEventKind::Created
            | WorkEventKind::Assigned
            | WorkEventKind::Claimed
            | WorkEventKind::Started
            | WorkEventKind::Released
            | WorkEventKind::Resumed
            | WorkEventKind::Updated
            | WorkEventKind::Rebound
            | WorkEventKind::Failed
            | WorkEventKind::DependenciesChanged
            | WorkEventKind::ExecutionRetargeted => HostAttentionKind::WorkChanged,
        };
        Some(HostAttention {
            id: format!("host-attention-{}", operation.event.id),
            team_run_id: operation.event.team_run_id.clone(),
            kind,
            work_id: operation.event.work_id.clone(),
            work_version: operation.event.resulting_version,
            source_event_ref: operation.event.id.clone(),
            // The WorkEvent actor is immutable execution evidence written
            // after the exact binding fence succeeds. Attention keeps that
            // submitting runtime for evidence only; it never authorizes Work.
            member_run_id: (operation.event.performed_by_actor.kind
                == TeamActorKind::ProviderRuntimeProjection)
                .then(|| operation.event.performed_by_actor.id.clone()),
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
            for attention in Self::downstream_host_attentions_for_work_operation(operation)? {
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
                self.ensure_host_attention_unlocked(&attention)?;
                projected.insert(attention.id.clone(), attention.clone());
                reconciled.push(attention);
            }
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
        for attention in self.canonical_host_attention_outbox_unlocked()? {
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
            self.ensure_host_attention_unlocked(&attention)?;
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
        let host_binding = self.host_member_binding(team_run_id)?;
        let warning = (host_binding.mode == firm_core::HostControlMode::ExternalInteractive
            && !attentions.is_empty())
        .then(|| {
            format!(
                "EXTERNAL_HOST_PULL_ONLY: Host MemberRun {} must explicitly read this inbox; no provider receipt or timely wake is available",
                host_binding.member_run.id
            )
        });
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
        // Canonical operations own the immutable source fact, while
        // host_attentions.jsonl owns the later delivery lifecycle projection.
        // Fold source records first so Claimed/Delivered/Acknowledged rows are
        // not reset to their initial Actionable state on every read.
        let mut sources = std::collections::BTreeMap::new();
        for execution_space_id in self.canonical_execution_space_ids()? {
            for attention in self.trust_side_records::<HostAttention>(&execution_space_id)? {
                let decision = firm_application::fold_host_attention_source(
                    sources.get(&attention.id),
                    &attention,
                )
                .map_err(|error| {
                    StoreError::Conflict(format!(
                        "HOST_ATTENTION_SOURCE_FACT_CONFLICT: canonical source {}: {error}",
                        attention.id
                    ))
                })?;
                if decision != firm_application::ProjectionFoldDecision::Replay {
                    sources.insert(attention.id.clone(), attention);
                }
            }
        }
        let mut latest = sources;
        for attention in self.read_jsonl::<HostAttention>("host_attentions.jsonl")? {
            let decision = firm_application::fold_host_attention_lifecycle(
                latest.get(&attention.id),
                &attention,
            )
            .map_err(|error| {
                let code = if error
                    == firm_application::ProjectionFoldViolation::ImmutableIdentityConflict
                {
                    "HOST_ATTENTION_SOURCE_FACT_CONFLICT"
                } else {
                    "HOST_ATTENTION_LIFECYCLE_FOLD_CONFLICT"
                };
                StoreError::Conflict(format!(
                    "{code}: lifecycle projection {}: {error}",
                    attention.id
                ))
            })?;
            if decision != firm_application::ProjectionFoldDecision::Replay {
                latest.insert(attention.id.clone(), attention);
            }
        }
        Ok(latest)
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
        let mut works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|candidate| candidate.id != work.id)
            .collect::<Vec<_>>();
        works.push(work.clone());
        // All current writers use the same pure Kernel DAG validator. This
        // also closes cycles at creation/import boundaries instead of relying
        // only on the dedicated dependency replacement command.
        firm_core::prepare_dependency_change(work, work.prerequisite_work_ids.clone(), &works)
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if work.active_member_run_id.is_some() {
            return Err(StoreError::Conflict(
                "LEGACY_RUNTIME_WORK_AUTHORITY_RETIRED: active_member_run_id is historical read/export evidence and cannot enter a current Work mutation"
                    .to_string(),
            ));
        }
        // DOC-106: responsibility never depends on an active MemberRun or
        // runtime. `assignee_membership_id` (mirrored by `owner_member_id`) is
        // durable and survives Close/Reopen; only the transient execution
        // binding above is runtime-fenced.
        Ok(())
    }

    /// Authorize one current MemberRun against stable Work responsibility and
    /// the exact active execution binding. Historical runtime-owned Work is
    /// read/export evidence only and never grants current mutation authority.
    pub(super) fn member_run_holds_work_responsibility_unlocked(
        &self,
        work: &Work,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<bool> {
        if work.owner_member_id.as_deref() != Some(member.agent_member_id.as_str()) {
            return Ok(false);
        }
        let Some(membership_id) = work.assignee_membership_id.as_deref() else {
            return Ok(false);
        };
        let Some(team_id) = work.accountable_team_id.as_deref() else {
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
        let active_bindings = self
            .fabric_work_execution_bindings(space_id)?
            .into_iter()
            .filter(|binding| {
                binding.work_id == work.id
                    && binding.status
                        == firm_core::agentfirm_api::WorkExecutionBindingStatus::Active
            })
            .collect::<Vec<_>>();
        let [binding] = active_bindings.as_slice() else {
            return Ok(false);
        };
        if binding.work_revision > work.version
            || self.work_responsibility_changed_after_revision_unlocked(
                &work.id,
                binding.work_revision,
            )?
            || binding.team_id != team_id
            || binding.team_membership_id != membership.id
            || binding.agent_member_id != member.agent_member_id
        {
            return Ok(false);
        }
        let sessions = self
            .fabric_agent_sessions(space_id)?
            .into_iter()
            .filter(|session| session.id == binding.agent_session_id)
            .collect::<Vec<_>>();
        let [session] = sessions.as_slice() else {
            return Ok(false);
        };
        let admission = self.work_execution_runtime_binding(space_id, &binding.id)?;
        Ok(session.agent_member_id == member.agent_member_id
            && session.runtime_generation == binding.agent_session_generation
            && session.lifecycle != firm_core::agentfirm_api::AgentSessionStatus::Closed
            && admission.target_member_run_id.as_deref() == Some(member.id.as_str())
            && admission.target_member_run_generation == Some(member.runtime_generation)
            && admission.target_session_id.as_deref() == Some(session.id.as_str())
            && admission.target_runtime_generation == Some(session.runtime_generation)
            && member.coordination_is_active()
            && !matches!(
                member.status,
                firm_core::MemberRunStatus::Completed
                    | firm_core::MemberRunStatus::Failed
                    | firm_core::MemberRunStatus::Stopped
            ))
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
