use super::*;

impl HarnessStore {
    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn claim_work_delivery(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDeliveryClaimResult> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use WorkExecutionBinding and identity-first WorkDelivery"
                .into(),
        ));
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut deliveries = self.latest_work_deliveries_unlocked()?;
        let Some(mut delivery) = deliveries.remove(delivery_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        if delivery.team_run_id != team_run_id
            || delivery.recipient_member_run_id != member_run_id
            || !matches!(
                delivery.status,
                ProviderWorkDispatchStatus::Queued | ProviderWorkDispatchStatus::Failed
            )
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let works = self.latest_works_unlocked()?;
        let Some(work) = works.get(&delivery.work_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        // A queued row is only actionable for the newest Work revision and
        // current runtime binding. `Open` is deliberately not required:
        // revisions created by resume/change-request/rebind can be delivered
        // while the Work is in progress, blocked, or under review.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || work.is_terminal()
            || !work.prerequisites_satisfied(works.values())
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // A provider receipt is published as soon as the native runtime
        // accepts a Work prompt. The member may not have executed `work start`
        // yet, so the Work can still be `open` during this hand-off window.
        // Treat that receipted (or still-claimed) Work as occupying the single
        // member execution slot, in addition to explicitly active lifecycle
        // states. A later revision of the *same* Work remains deliverable for
        // resume/change-request; only a different Work is fenced.
        if works.values().any(|other| {
            other.id != work.id
                && other.team_run_id == team_run_id
                && other.active_member_run_id.as_deref() == Some(member_run_id)
                && ((other.phase == WorkPhase::Active)
                    || (other.phase == WorkPhase::Open
                        && other.condition == WorkCondition::Normal
                        && deliveries.values().any(|existing| {
                            existing.work_id == other.id
                                && existing.recipient_member_run_id == member_run_id
                                && matches!(
                                    existing.status,
                                    ProviderWorkDispatchStatus::Claimed
                                        | ProviderWorkDispatchStatus::ProviderReceived
                                )
                        })))
        }) {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let member = self.require_member_run_unlocked(member_run_id, team_run_id)?;
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        delivery.status = ProviderWorkDispatchStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(WorkDeliveryClaimResult::Claimed(Box::new(delivery)))
    }

    /// Claim a queued ProviderWorkDispatch for a terminal work notification.
    ///
    /// Like [`claim_work_delivery`] but permits terminal (Accepted /
    /// Cancelled) works, skips the prerequisite-satisfied check, and does not
    /// fence on another active work occupying the member slot. A terminal-work
    /// notification is informational (the supervisor turns it into a
    /// TeamMessageProjection), not an execution assignment.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_notification(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut deliveries = self.latest_work_deliveries_unlocked()?;
        let Some(mut delivery) = deliveries.remove(delivery_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        if delivery.team_run_id != team_run_id
            || delivery.recipient_member_run_id != member_run_id
            || !matches!(
                delivery.status,
                ProviderWorkDispatchStatus::Queued | ProviderWorkDispatchStatus::Failed
            )
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let works = self.latest_works_unlocked()?;
        let Some(work) = works.get(&delivery.work_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        // Terminal works are allowed; the supervisor will turn this delivery
        // into a TeamMessageProjection, not a work-assignment prompt.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || !work.is_terminal()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // No slot-occupancy fence: a terminal-work notification never blocks
        // an active execution assignment.
        delivery.status = ProviderWorkDispatchStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(WorkDeliveryClaimResult::Claimed(Box::new(delivery)))
    }

    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn complete_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use NodeDaemon provider receipt on canonical WorkDelivery"
                .into(),
        ));
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }
        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == ProviderWorkDispatchStatus::ProviderReceived && owns_claim {
            if delivery.provider_receipt_id.as_deref() != Some(provider_receipt_id) {
                return Err(StoreError::Conflict(format!(
                    "ProviderWorkDispatch claim {claim_id} was already completed with a different provider receipt"
                )));
            }
            return Ok(delivery);
        }
        if !owns_claim
            || delivery.recipient_member_run_id != member_run_id
            || delivery.status != ProviderWorkDispatchStatus::Claimed
        {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch claim {claim_id} no longer owns {delivery_id}"
            )));
        }
        delivery.status = ProviderWorkDispatchStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: delivery.provider_receipt_id.clone(),
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    /// Fail the currently-owned ProviderWorkDispatch claim. Only the Supervisor that
    /// owns the current, unexpired TeamRun lease and the exact durable claim
    /// may write this terminal delivery outcome. The failure reason is control
    /// evidence, not a copy of provider output.
    #[allow(clippy::too_many_arguments, unreachable_code, unused_variables)]
    pub fn fail_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        return Err(StoreError::Conflict(
            "RETIRED_RUNTIME_WRITER: use canonical WorkDelivery recovery".into(),
        ));
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "ProviderWorkDispatch failure reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == ProviderWorkDispatchStatus::Failed && owns_claim {
            if delivery.failure_reason.as_deref() != Some(reason) {
                return Err(StoreError::Conflict(format!(
                    "ProviderWorkDispatch claim {claim_id} was already failed with a different reason"
                )));
            }
            return Ok(delivery);
        }
        if delivery.status != ProviderWorkDispatchStatus::Claimed || !owns_claim {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch claim {claim_id} no longer owns {delivery_id}"
            )));
        }

        delivery.status = ProviderWorkDispatchStatus::Failed;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: delivery.failure_reason.clone(),
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        self.ensure_host_attention_unlocked(&HostAttention {
            id: format!("host-attention-wd-{}-failed", delivery.id),
            team_run_id: delivery.team_run_id.clone(),
            kind: HostAttentionKind::WorkDeliveryFailed,
            work_id: delivery.work_id.clone(),
            work_version: delivery.work_version,
            source_event_ref: format!("wd-update:{}", update_sequence),
            member_run_id: Some(delivery.recipient_member_run_id.clone()),
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
            created_at: delivery.updated_at.clone(),
            updated_at: delivery.updated_at.clone(),
        })?;
        Ok(delivery)
    }

    /// Requeue a ProviderWorkDispatch claim abandoned by an older Supervisor
    /// generation. This is intentionally explicit: an expired lease alone is
    /// not proof that the provider did not receive the Work.
    ///
    /// Only the current, unexpired successor lease may reconcile. A claim with
    /// a provider receipt, or a delivery already marked provider-received or
    /// acknowledged, is never rolled back.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_stale_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        if delivery.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch {delivery_id} belongs to {}, not {team_run_id}",
                delivery.team_run_id
            )));
        }
        if delivery.status == ProviderWorkDispatchStatus::Queued
            && delivery.claim_id.is_none()
            && delivery.claimed_by_supervisor_id.is_none()
            && delivery.claimed_generation.is_none()
            && delivery.provider_receipt_id.is_none()
        {
            return Ok(delivery);
        }
        if delivery.status != ProviderWorkDispatchStatus::Claimed {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} is {:?} and cannot be requeued",
                delivery.status
            )));
        }
        if delivery.provider_receipt_id.is_some() {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} has a provider receipt"
            )));
        }
        let claimed_generation = delivery.claimed_generation.ok_or_else(|| {
            StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} has no claimed generation"
            ))
        })?;
        if claimed_generation >= supervisor_generation {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch {delivery_id} is not a stale claim from a predecessor Supervisor generation"
            )));
        }

        delivery.status = ProviderWorkDispatchStatus::Queued;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    pub fn team_supervisor_leases(&self) -> StoreResult<Vec<TeamSupervisorLease>> {
        self.read_jsonl("team_supervisor_leases.jsonl")
    }

    pub fn latest_team_supervisor_lease(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        Ok(latest_by_id(self.team_supervisor_leases()?, |lease| {
            lease.team_run_id.clone()
        })
        .remove(team_run_id))
    }

    pub fn team_member_close_requests(&self) -> StoreResult<Vec<TeamMemberCloseRequest>> {
        self.read_jsonl("team_member_close_requests.jsonl")
    }

    pub fn latest_team_member_close_request(
        &self,
        member_run_id: &str,
    ) -> StoreResult<Option<TeamMemberCloseRequest>> {
        Ok(latest_by_id(self.team_member_close_requests()?, |request| {
            request.member_run_id.clone()
        })
        .remove(member_run_id))
    }

    pub fn member_actions(&self) -> StoreResult<Vec<MemberAction>> {
        self.read_jsonl("member_actions.jsonl")
    }

    pub fn delegation_runs(&self) -> StoreResult<Vec<DelegationRun>> {
        self.read_jsonl("delegation_runs.jsonl")
    }

    /// Raw historical event rows for explicit Legacy diagnostics/export.
    /// Current product projections must use `current_team_run_events`.
    pub fn legacy_team_run_events(&self) -> StoreResult<Vec<TeamRunEvent>> {
        self.read_jsonl("team_run_events.jsonl")
    }

    /// Read the current event projection only after the whole TeamRun has one
    /// coherent canonical Execution Space.
    pub fn current_team_run_events(&self, team_run_id: &str) -> StoreResult<Vec<TeamRunEvent>> {
        self.init()?;
        let run = self.require_team_run_unlocked(team_run_id)?;
        self.current_team_run_execution_space(&run)?;
        Ok(self
            .read_jsonl("team_run_events.jsonl")?
            .into_iter()
            .filter(|event: &TeamRunEvent| event.team_run_id == team_run_id)
            .collect())
    }
}
