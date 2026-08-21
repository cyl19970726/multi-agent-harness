use super::*;

impl HarnessStore {
    pub fn append_member_action(&self, value: &MemberAction) -> StoreResult<()> {
        if value.action_type == "provider_control" {
            return Err(StoreError::Conflict(
                "PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN: use append_member_action_if_member_run_current"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run = self.require_team_run_unlocked(&value.team_run_id)?;
        self.current_team_run_execution_space_unlocked(&run)?;
        self.append_jsonl_unlocked("member_actions.jsonl", value)
    }

    /// Append a provider/control receipt only while the exact ProviderRuntimeProjection
    /// generation and native-session snapshot observed by the caller remains
    /// current. The full-row equality check intentionally binds generation and
    /// session without copying those runtime fields into `MemberAction`.
    ///
    /// Returns true only for the call that appended. Exact action-id retries,
    /// and the bounded provider-control receipt key
    /// `(member_run_id, action_type, title)`, converge to false under the same
    /// global lock. Lifecycle CAS and receipt append therefore cannot cross a
    /// check/append gap.
    pub fn append_member_action_if_member_run_current(
        &self,
        expected_member: &ProviderRuntimeProjection,
        action: &MemberAction,
    ) -> StoreResult<bool> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&expected_member.id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                expected_member.id
            ))
        })?;
        if &current != expected_member {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; provider receipt was not appended",
                expected_member.id
            )));
        }
        if !member_is_active_reviewer_runtime(&current) || current.native_session.is_none() {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} is not active in a native session; provider receipt was not appended",
                current.id
            )));
        }
        let run = self.require_team_run_unlocked(&current.team_run_id)?;
        self.current_team_run_execution_space_unlocked(&run)?;
        if !run
            .member_run_ids
            .iter()
            .any(|member| member == &current.id)
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} is not admitted to TeamRun {}",
                current.id, current.team_run_id
            )));
        }
        if action.team_run_id != current.team_run_id || action.member_run_id != current.id {
            return Err(StoreError::Conflict(format!(
                "MemberAction {} is not bound to ProviderRuntimeProjection {} in TeamRun {}",
                action.id, current.id, current.team_run_id
            )));
        }
        let actions = self.read_jsonl::<MemberAction>("member_actions.jsonl")?;
        if let Some(existing) = actions.iter().find(|existing| existing.id == action.id) {
            if existing == action {
                return Ok(false);
            }
            return Err(StoreError::Conflict(format!(
                "MemberAction id already exists with different semantics: {}",
                action.id
            )));
        }
        if action.action_type == "provider_control"
            && actions.iter().any(|existing| {
                existing.member_run_id == action.member_run_id
                    && existing.action_type == action.action_type
                    && existing.title == action.title
            })
        {
            return Ok(false);
        }
        self.append_jsonl_unlocked("member_actions.jsonl", action)?;
        Ok(true)
    }

    pub fn append_delegation_run(&self, value: &DelegationRun) -> StoreResult<()> {
        self.append_jsonl("delegation_runs.jsonl", value)
    }

    /// Reconstruct a raw historical TeamRun event during explicit Legacy
    /// import. Current event writers must use the guarded next-sequence APIs.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn legacy_import_append_team_run_event(
        &self,
        value: &TeamRunEvent,
    ) -> StoreResult<()> {
        self.append_jsonl("team_run_events.jsonl", value)
    }

    /// Allocate and append the next per-TeamRun event sequence under one store
    /// lock so concurrent HTTP/MCP/provider writers cannot duplicate `seq`.
    pub fn append_team_run_event_next(&self, mut value: TeamRunEvent) -> StoreResult<TeamRunEvent> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run = self.require_team_run_unlocked(&value.team_run_id)?;
        self.current_team_run_execution_space_unlocked(&run)?;
        value.seq = self
            .read_jsonl::<TeamRunEvent>("team_run_events.jsonl")?
            .into_iter()
            .filter(|event| event.team_run_id == value.team_run_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        self.append_jsonl_unlocked("team_run_events.jsonl", &value)?;
        Ok(value)
    }

    /// Idempotently append one semantic TeamRun event under the store lock.
    pub fn ensure_team_run_event_next(
        &self,
        stable_key: &str,
        mut value: TeamRunEvent,
    ) -> StoreResult<TeamRunEvent> {
        require_non_empty_store(stable_key, "TeamRun event stable key")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run = self.require_team_run_unlocked(&value.team_run_id)?;
        self.current_team_run_execution_space_unlocked(&run)?;
        value.id = format!("trev-stable-{}", content_hash_hex16(stable_key));
        let events = self.read_jsonl::<TeamRunEvent>("team_run_events.jsonl")?;
        if let Some(existing) = events.iter().find(|event| event.id == value.id) {
            if same_team_run_event_semantics(existing, &value) {
                return Ok(existing.clone());
            }
            return Err(StoreError::Conflict(format!(
                "TeamRunEvent id {} already names different causal semantics",
                value.id
            )));
        }
        value.seq = events
            .iter()
            .filter(|event| event.team_run_id == value.team_run_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        self.append_jsonl_unlocked("team_run_events.jsonl", &value)?;
        Ok(value)
    }

    /// Compare-and-append a TeamRun lifecycle row. Mission and Node authority
    /// are reached through the immutable AgentTeam relation and are never
    /// copied or updated by a run transition.
    pub fn compare_and_append_team_run_lifecycle(
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
                "team run {} changed concurrently or is no longer startable",
                expected.id
            )));
        }
        self.current_team_run_execution_space_unlocked(&current)?;
        if next.member_run_ids != current.member_run_ids {
            return Err(StoreError::Conflict(
                "TEAM_MEMBERSHIP_REQUIRES_ADMISSION: lifecycle revision cannot change member_run_ids; use admit_member_run"
                    .to_string(),
            ));
        }
        let mut allowed_lifecycle = current.clone();
        allowed_lifecycle.status = next.status;
        allowed_lifecycle.updated_at = next.updated_at.clone();
        allowed_lifecycle.completed_at = next.completed_at.clone();
        if *next != allowed_lifecycle {
            return Err(StoreError::Conflict(
                "TEAM_RUN_LIFECYCLE_SCOPE_IMMUTABLE: lifecycle CAS may only change status, updated_at, and completed_at"
                    .to_string(),
            ));
        }
        if next.status == TeamRunStatus::Completed {
            let unfinished = self
                .latest_works_unlocked()?
                .into_values()
                .filter(|work| work.team_run_id == next.id && !work.is_terminal())
                .collect::<Vec<_>>();
            if !unfinished.is_empty() {
                let detail = unfinished
                    .iter()
                    .map(|work| {
                        let phase = serde_json::to_string(&work.phase)
                            .unwrap_or_else(|_| format!("{:?}", work.phase));
                        let condition = serde_json::to_string(&work.condition)
                            .unwrap_or_else(|_| format!("{:?}", work.condition));
                        format!(
                            "{} ({}/{}, version {})",
                            work.id,
                            phase.trim_matches('"'),
                            condition.trim_matches('"'),
                            work.version
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(StoreError::Conflict(format!(
                    "team run {} cannot complete while Works remain non-terminal: {detail}; accept or cancel every Work first",
                    next.id
                )));
            }
        }

        self.append_jsonl_unlocked("team_runs.jsonl", next)?;
        Ok(())
    }

    pub fn claim_queued_message_delivery(
        &self,
        agent_member_id: &str,
        message_id: &str,
        delivery: RegistryDeliveryAttempt,
    ) -> StoreResult<MessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        let latest_messages = latest_by_id(
            self.read_jsonl::<RegistryMessage>("messages.jsonl")?,
            |message| message.id.clone(),
        );
        if let Some(active) = latest_messages.values().find(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message
                    .delivery
                    .as_ref()
                    .is_some_and(delivery_blocks_another_claim)
        }) {
            let delivery_id = active
                .delivery
                .as_ref()
                .and_then(|delivery| delivery.delivery_id.clone())
                .unwrap_or_else(|| active.id.clone());
            return Ok(MessageDeliveryClaimResult::BlockedByDelivery(delivery_id));
        }
        let Some(mut message) = latest_messages.get(message_id).cloned() else {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        };
        if message.to_agent_id.as_deref() != Some(agent_member_id)
            || message.delivery_status != RegistryDeliveryStatus::Queued
        {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        }

        message.delivery_status = RegistryDeliveryStatus::Acknowledged;
        message.delivery = Some(delivery);
        self.append_jsonl_unlocked("messages.jsonl", &message)?;

        Ok(MessageDeliveryClaimResult::Claimed(Box::new(message)))
    }
}
