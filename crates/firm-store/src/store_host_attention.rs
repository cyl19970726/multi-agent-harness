use super::*;

impl HarnessStore {
    /// Compare-and-append one TeamRun revision.
    ///
    /// Host binding is mutable coordination metadata, but changing it must not
    /// silently overwrite a concurrent lifecycle/member update. Keep the
    /// identity, execution scope, and creation time stable while allowing the
    /// caller to revise addressability fields and `updated_at`.
    pub fn compare_and_append_team_run(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        self.current_team_run_execution_space_unlocked(&current)?;
        if next.member_run_ids != current.member_run_ids {
            return Err(StoreError::Conflict(
                "TEAM_MEMBERSHIP_REQUIRES_ADMISSION: Host binding revision cannot change member_run_ids"
                    .to_string(),
            ));
        }
        if next.id != current.id
            || next.created_at != current.created_at
            || next.agent_team_id != current.agent_team_id
            || next.execution_node_id != current.execution_node_id
            || next.project_binding_id != current.project_binding_id
            || next.previous_run_id != current.previous_run_id
            || next.execution_root != current.execution_root
            || next.member_run_ids != current.member_run_ids
            || next.status != current.status
            || next.objective != current.objective
            || next.budget_limit_usd != current.budget_limit_usd
            || next.completed_at != current.completed_at
        {
            return Err(StoreError::Conflict(
                "Host binding revision must preserve TeamRun identity, scope, members, lifecycle, and objective"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("team_runs.jsonl", next)
    }

    /// Acquire exclusive ownership of a TeamRun's current exact Host binding.
    /// A live owner is never preempted. Expiry, release, or a TeamRun rebind
    /// permits takeover and advances the durable generation.
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_host_binding_lease(
        &self,
        team_run_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        owner_kind: HostBindingLeaseOwnerKind,
        owner_id: &str,
        lease_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        require_non_empty_store(owner_id, "Host binding lease owner id")?;
        require_non_empty_store(lease_id, "Host binding lease id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run =
            self.require_exact_host_binding_unlocked(team_run_id, host_surface, host_thread_id)?;
        let current = self.latest_host_binding_lease_unlocked(team_run_id)?;
        if let Some(current) = current.as_ref() {
            let current_matches_binding = canonical_surface(&current.host_surface)
                == canonical_surface(&run.host_surface)
                && current.host_thread_id == host_thread_id;
            if current.is_effective_at(now_unix_ms) && current_matches_binding {
                if current.owner_kind == owner_kind
                    && current.owner_id == owner_id
                    && current.lease_id == lease_id
                {
                    return Ok(current.clone());
                }
                return Err(StoreError::Conflict(format!(
                    "HOST_BINDING_LEASE_HELD: TeamRun {team_run_id} binding is owned by {:?} {} generation {} until unix-ms:{}",
                    current.owner_kind, current.owner_id, current.generation, current.expires_unix_ms
                )));
            }
        }
        let generation = match current.as_ref() {
            Some(current) => current.generation.checked_add(1).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_BINDING_LEASE_GENERATION_EXHAUSTED: TeamRun {team_run_id}"
                ))
            })?,
            None => 1,
        };
        let lease = HostBindingLease {
            team_run_id: team_run_id.to_string(),
            host_surface: run.host_surface,
            host_thread_id: host_thread_id.to_string(),
            owner_kind,
            owner_id: owner_id.to_string(),
            generation,
            lease_id: lease_id.to_string(),
            acquired_unix_ms: now_unix_ms,
            heartbeat_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            status: HostBindingLeaseStatus::Active,
            released_unix_ms: None,
        };
        lease
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Renew an exact current lease. Every identity component is a CAS fence.
    pub fn renew_host_binding_lease(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_exact_host_binding_unlocked(
            &expected.team_run_id,
            &expected.host_surface,
            &expected.host_thread_id,
        )?;
        let mut current =
            self.require_current_host_binding_lease_owner_unlocked(expected, now_unix_ms)?;
        current.heartbeat_unix_ms = now_unix_ms;
        current.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &current)?;
        Ok(current)
    }

    /// Release an exact current lease. An exact retry is idempotent; every
    /// stale generation, lease id, or owner is rejected.
    pub fn release_host_binding_lease(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_exact_host_binding_unlocked(
            &expected.team_run_id,
            &expected.host_surface,
            &expected.host_thread_id,
        )?;
        let mut current = self
            .latest_host_binding_lease_unlocked(&expected.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TeamRun {} has no Host binding lease",
                    expected.team_run_id
                ))
            })?;
        self.require_same_host_binding_lease_owner(&current, expected)?;
        if current.status == HostBindingLeaseStatus::Released {
            return Ok(current);
        }
        current.status = HostBindingLeaseStatus::Released;
        current.heartbeat_unix_ms = now_unix_ms;
        current.expires_unix_ms = now_unix_ms;
        current.released_unix_ms = Some(now_unix_ms);
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &current)?;
        Ok(current)
    }

    /// Latest persisted row, including released and expired rows. `None` is
    /// the explicit legacy/unleased state.
    pub fn latest_host_binding_lease(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<HostBindingLease>> {
        self.latest_host_binding_lease_unlocked(team_run_id)
    }

    /// Return the active lease only when it is live and still matches the
    /// TeamRun's current exact Host binding.
    pub fn effective_host_binding_lease_at(
        &self,
        team_run_id: &str,
        now_unix_ms: u64,
    ) -> StoreResult<Option<HostBindingLease>> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        Ok(self
            .latest_host_binding_lease_unlocked(team_run_id)?
            .filter(|lease| {
                lease.is_effective_at(now_unix_ms)
                    && canonical_surface(&lease.host_surface)
                        == canonical_surface(&run.host_surface)
                    && run.host_thread_id.as_deref() == Some(lease.host_thread_id.as_str())
            }))
    }

    /// Materialize one deterministic HostBindingStale attention for every
    /// bound TeamRun whose current binding has no effective lease. Repeated
    /// scans of the same binding/generation are idempotent.
    pub fn reconcile_host_binding_stale_attentions(
        &self,
        now_unix_ms: u64,
        observed_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        require_non_empty_store(observed_at, "Host binding stale observed_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_host_binding_stale_attentions_unlocked(now_unix_ms, observed_at)
    }

    /// Idempotently append one durable Host-attention fact.
    ///
    /// Runtime integration must derive `attention.id` from the causal event
    /// (for example `host-attention-<work-event-id>`). Replaying the same event
    /// returns the latest delivery/intake projection instead of resetting it
    /// to `actionable` or fabricating a TeamMessageProjection.
    pub fn ensure_host_attention(&self, attention: &HostAttention) -> StoreResult<HostAttention> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_host_attention_unlocked(attention)
    }

    /// Repair the only intentional two-ledger crash boundary: a WorkOperation
    /// may be fsynced immediately before its derived HostAttention row. The
    /// deterministic attention id makes this replay safe and lets Host reads or
    /// an explicit startup reconciliation materialize exactly the missing row.
    pub fn reconcile_work_host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()
    }

    /// Latest-wins Host-attention projection across all TeamRuns.
    pub fn host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.reconcile_work_host_attentions()?;
        Ok(self
            .latest_host_attentions_unlocked()?
            .into_values()
            .collect())
    }

    /// Read one TeamRun's Host-attention projection, including an explicit
    /// warning when no exact native Host task is bound.
    pub fn host_attention_inbox_for_team_run(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        self.reconcile_work_host_attentions()?;
        self.host_attention_inbox_for_team_run_unreconciled(team_run_id, include_all)
    }

    /// Aggregate only attentions owned by the exact provider-native Host task.
    /// Unbound TeamRuns and other tasks are excluded by construction.
    pub fn host_attention_inboxes_for_native_thread(
        &self,
        host_surface: &str,
        host_thread_id: &str,
        include_all: bool,
    ) -> StoreResult<Vec<HostAttentionInbox>> {
        if host_surface.trim().is_empty() || host_thread_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "Host surface and native thread id must not be empty".to_string(),
            ));
        }
        self.reconcile_work_host_attentions()?;
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        let mut inboxes = Vec::new();
        for run in runs.into_values().filter(|run| {
            canonical_surface(&run.host_surface) == canonical_surface(host_surface)
                && run.host_thread_id.as_deref() == Some(host_thread_id)
        }) {
            let inbox =
                self.host_attention_inbox_for_team_run_unreconciled(&run.id, include_all)?;
            if include_all || !inbox.attentions.is_empty() {
                inboxes.push(inbox);
            }
        }
        Ok(inboxes)
    }

    /// Fence one delivery attempt to the TeamRun's current exact Host binding.
    /// A claimed or delivered row cannot be claimed again, which prevents a
    /// managed idle wake and a safe-boundary hook from both starting delivery.
    pub fn claim_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        claim_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttentionClaimResult> {
        require_non_empty_store(attention_id, "Host attention id")?;
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        require_non_empty_store(claim_id, "Host attention claim id")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Claimed
            && attention.claim_id.as_deref() == Some(claim_id)
            && attention.claimed_host_surface.as_deref() == Some(host_surface)
            && attention.claimed_host_thread_id.as_deref() == Some(host_thread_id)
        {
            return Ok(HostAttentionClaimResult::Claimed(Box::new(attention)));
        }
        if attention.status != HostAttentionStatus::Actionable {
            return Ok(HostAttentionClaimResult::NotActionable);
        }
        attention.status = HostAttentionStatus::Claimed;
        attention.attempt = attention.attempt.saturating_add(1);
        attention.claim_id = Some(claim_id.to_string());
        attention.claimed_host_surface = Some(host_surface.to_string());
        attention.claimed_host_thread_id = Some(host_thread_id.to_string());
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.claimed_recipient_member_run_id = None;
        attention.claimed_recipient_session_id = None;
        attention.claimed_recipient_session_generation = None;
        attention.claimed_node_daemon_id = None;
        attention.claimed_node_daemon_generation = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = None;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(HostAttentionClaimResult::Claimed(Box::new(attention)))
    }

    /// Claim a bounded HostAttention batch for the managed Host MemberRun.
    /// The claim is fenced to the exact AgentSession and machine NodeDaemon
    /// generations; external Host bindings cannot call this path.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_managed_host_attention_batch(
        &self,
        execution_space_id: &str,
        team_run_id: &str,
        member_run_id: &str,
        session_id: &str,
        session_generation: u64,
        daemon_id: &str,
        daemon_generation: u64,
        claim_id: &str,
        limit: usize,
        include_low_value: bool,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        require_non_empty_store(claim_id, "managed Host attention claim id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let run = self.require_team_run_unlocked(team_run_id)?;
        if run.host_control_mode != firm_core::HostControlMode::Managed {
            return Err(StoreError::Conflict(
                "MANAGED_HOST_CLAIM_REQUIRES_MANAGED_TEAM_RUN".to_string(),
            ));
        }
        let team = latest_by_id(self.all_agent_teams()?, |team| team.id.clone())
            .remove(&run.agent_team_id)
            .ok_or_else(|| StoreError::Conflict("managed Host Team is missing".to_string()))?;
        let member = self.require_member_run_unlocked(member_run_id, team_run_id)?;
        if member.agent_member_id != team.host_agent_id || member.is_external_interactive() {
            return Err(StoreError::Conflict(
                "MANAGED_HOST_MEMBER_RUN_FENCED".to_string(),
            ));
        }
        let session = self
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .find(|session| {
                session.id == session_id
                    && session.agent_member_id == team.host_agent_id
                    && session.runtime_generation == session_generation
                    && session.node_daemon_id == daemon_id
                    && session.node_daemon_generation == daemon_generation
                    && session.lifecycle != firm_core::agentfirm_api::AgentSessionStatus::Closed
            })
            .ok_or_else(|| StoreError::Conflict("AGENT_SESSION_GENERATION_FENCED".to_string()))?;
        let lease = self
            .latest_node_daemon_lease(&session.node_id)?
            .filter(|lease| {
                lease.daemon_id == daemon_id
                    && lease.generation == daemon_generation
                    && lease.status == NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > now_unix_ms
            })
            .ok_or_else(|| StoreError::Conflict("NODE_DAEMON_GENERATION_FENCED".to_string()))?;
        let mut eligible = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                attention.team_run_id == team_run_id
                    && attention.status == HostAttentionStatus::Actionable
                    && (include_low_value || attention.kind != HostAttentionKind::WorkChanged)
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| {
            compare_store_timestamps(&left.created_at, &right.created_at)
                .then(left.id.cmp(&right.id))
        });
        eligible.truncate(limit.max(1));
        for attention in &mut eligible {
            attention.status = HostAttentionStatus::Claimed;
            attention.attempt = attention.attempt.saturating_add(1);
            attention.claim_id = Some(claim_id.to_string());
            attention.claimed_host_surface = Some("managed".to_string());
            attention.claimed_host_thread_id = None;
            attention.claimed_host_lease_id = None;
            attention.claimed_host_lease_generation = None;
            attention.claimed_host_lease_owner_id = None;
            attention.claimed_recipient_member_run_id = Some(member_run_id.to_string());
            attention.claimed_recipient_session_id = Some(session_id.to_string());
            attention.claimed_recipient_session_generation = Some(session_generation);
            attention.claimed_node_daemon_id = Some(lease.daemon_id.clone());
            attention.claimed_node_daemon_generation = Some(lease.generation);
            attention.provider_receipt_id = None;
            attention.last_failure_reason = None;
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        }
        Ok(eligible)
    }

    /// Record provider-native delivery receipt for the currently-owned claim.
    pub fn complete_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(provider_receipt_id, "Host attention provider receipt")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if matches!(
            attention.status,
            HostAttentionStatus::Delivered | HostAttentionStatus::Acknowledged
        ) && attention.claim_id.as_deref() == Some(claim_id)
            && attention.provider_receipt_id.as_deref() == Some(provider_receipt_id)
        {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        let surface = attention.claimed_host_surface.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host surface".to_string())
        })?;
        if surface == "managed" {
            self.require_managed_host_attention_fence_unlocked(&attention, updated_at)?;
            // The shared runtime calls this only from the provider's exact
            // input-acceptance callback. For a managed recipient that is both
            // transport receipt and durable inbox intake/cursor progression;
            // external UI/hook visibility never enters this branch.
            attention.status = HostAttentionStatus::Acknowledged;
            attention.provider_receipt_id = Some(provider_receipt_id.to_string());
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            return Ok(attention);
        }
        let thread_id = attention.claimed_host_thread_id.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host thread id".to_string())
        })?;
        self.require_exact_host_binding_unlocked(&attention.team_run_id, &surface, &thread_id)?;
        if attention.claimed_host_lease_id.is_some() {
            let now_unix_ms = parse_iso8601_to_unix_ms(updated_at).ok_or_else(|| {
                StoreError::Conflict(
                    "leased HostAttention completion requires unix-ms updated_at".to_string(),
                )
            })?;
            self.require_host_attention_lease_fence_unlocked(&attention, now_unix_ms)?;
        }
        attention.status = HostAttentionStatus::Delivered;
        attention.provider_receipt_id = Some(provider_receipt_id.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Return an uncertain/failed claim to the actionable state for retry.
    pub fn fail_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(reason, "Host attention failure reason")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        if attention.claimed_host_lease_id.is_some() {
            let now_unix_ms = parse_iso8601_to_unix_ms(updated_at).ok_or_else(|| {
                StoreError::Conflict(
                    "leased HostAttention failure requires unix-ms updated_at".to_string(),
                )
            })?;
            self.require_host_attention_lease_fence_unlocked(&attention, now_unix_ms)?;
        }
        attention.status = HostAttentionStatus::Actionable;
        attention.claim_id = None;
        attention.claimed_host_surface = None;
        attention.claimed_host_thread_id = None;
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.claimed_recipient_member_run_id = None;
        attention.claimed_recipient_session_id = None;
        attention.claimed_recipient_session_generation = None;
        attention.claimed_node_daemon_id = None;
        attention.claimed_node_daemon_generation = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = Some(reason.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// ACK transport intake from the exact currently-bound Host task. This is
    /// intentionally independent of Work accept/request-changes commands.
    pub fn acknowledge_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Acknowledged {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Delivered
            || attention
                .claimed_host_surface
                .as_deref()
                .map(canonical_surface)
                != Some(canonical_surface(host_surface))
            || attention.claimed_host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention {attention_id} has not been delivered to this exact Host task"
            )));
        }
        attention.status = HostAttentionStatus::Acknowledged;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Mark a Host attention as requiring explicit human escalation. Only valid
    /// from `Actionable` or `Claimed` states. This is a terminal state set by
    /// the headless host dispatcher when an attention needs human decision
    /// (accept/merge/cancel) that the triage-only host cannot make.
    pub fn escalate_host_attention(
        &self,
        attention_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(attention_id, "Host attention id")?;
        require_non_empty_store(reason, "Host attention escalation reason")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status == HostAttentionStatus::EscalationRequired {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Actionable
            && attention.status != HostAttentionStatus::Claimed
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention {attention_id} is not in a state that can be escalated (current: {:?})",
                attention.status
            )));
        }
        // Release any stale claim so the attention is cleanly terminal.
        attention.status = HostAttentionStatus::EscalationRequired;
        attention.claim_id = None;
        attention.claimed_host_surface = None;
        attention.claimed_host_thread_id = None;
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = Some(reason.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Return actionable Host attentions whose `created_at` timestamp is older
    /// than `older_than_unix_ms`. Used by the host dispatcher to find attentions
    /// eligible for headless triage.
    pub fn actionable_attentions_older_than(
        &self,
        older_than_unix_ms: u64,
    ) -> StoreResult<Vec<HostAttention>> {
        self.reconcile_work_host_attentions()?;
        let all = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                if attention.status != HostAttentionStatus::Actionable {
                    return false;
                }
                // Parse the ISO 8601 created_at to a unix ms timestamp.
                // If parsing fails, treat the attention as eligible (fail open
                // so stale-but-malformed rows don't block dispatch forever).
                match crate::parse_iso8601_to_unix_ms(&attention.created_at) {
                    Some(ts) => ts < older_than_unix_ms,
                    None => true,
                }
            })
            .collect();
        Ok(all)
    }

    /// Atomically claim an aged actionable batch under the exact current
    /// Dispatcher lease. A live Interactive lease cannot satisfy this fence,
    /// and the store lock gives concurrent dispatchers one winner.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_dispatcher_host_attention_batch(
        &self,
        expected_lease: &HostBindingLease,
        older_than_unix_ms: u64,
        limit: usize,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        require_non_empty_store(claim_id, "Host attention batch claim id")?;
        require_non_empty_store(updated_at, "Host attention batch updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        self.reconcile_host_binding_stale_attentions_unlocked(now_unix_ms, updated_at)?;
        let current =
            self.require_current_host_binding_lease_owner_unlocked(expected_lease, now_unix_ms)?;
        if current.owner_kind != HostBindingLeaseOwnerKind::Dispatcher {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_INTERACTIVE_SUPPRESSES_DISPATCH: TeamRun {} is owned by Interactive Host {}",
                current.team_run_id, current.owner_id
            )));
        }
        self.require_exact_host_binding_unlocked(
            &current.team_run_id,
            &current.host_surface,
            &current.host_thread_id,
        )?;
        self.requeue_fenced_host_attention_claims_unlocked(&current, now_unix_ms, updated_at)?;

        let mut eligible = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                attention.team_run_id == current.team_run_id
                    && attention.status == HostAttentionStatus::Actionable
                    && parse_iso8601_to_unix_ms(&attention.created_at)
                        .map(|created| created < older_than_unix_ms)
                        .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| {
            compare_store_timestamps(&left.created_at, &right.created_at)
                .then(left.id.cmp(&right.id))
        });
        eligible.truncate(limit);
        for attention in &mut eligible {
            attention.status = HostAttentionStatus::Claimed;
            attention.attempt = attention.attempt.saturating_add(1);
            attention.claim_id = Some(claim_id.to_string());
            attention.claimed_host_surface = Some(current.host_surface.clone());
            attention.claimed_host_thread_id = Some(current.host_thread_id.clone());
            attention.claimed_host_lease_id = Some(current.lease_id.clone());
            attention.claimed_host_lease_generation = Some(current.generation);
            attention.claimed_host_lease_owner_id = Some(current.owner_id.clone());
            attention.provider_receipt_id = None;
            attention.last_failure_reason = None;
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        }
        Ok(eligible)
    }
}
