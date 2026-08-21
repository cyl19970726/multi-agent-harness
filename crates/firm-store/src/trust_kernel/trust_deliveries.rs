use super::*;

impl HarnessStore {
    #[cfg(any())]
    pub fn trust_message_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<MessageDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    #[cfg(any())]
    pub fn trust_team_messages(&self, execution_space_id: &str) -> StoreResult<Vec<TeamMessage>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_message")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_gate_waivers(&self, execution_space_id: &str) -> StoreResult<Vec<GateWaiver>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "gate_waiver")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<WorkDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    #[cfg(any())]
    pub fn create_trust_team_message_with_deliveries(
        &self,
        context: &MutationContext,
        message: TeamMessage,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMessage>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(&message.id, "TeamMessage.id")?;
        required(&message.team_run_id, "TeamMessage.team_run_id")?;
        required(&message.body, "TeamMessage.body")?;
        required(&message.correlation_id, "TeamMessage.correlation_id")?;
        if message.sender != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "message sender must equal authenticated actor",
                "team_message",
                &message.id,
                None,
            ));
        }
        if message.recipients.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "message requires at least one recipient",
                "team_message",
                &message.id,
                None,
            ));
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.id == message.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message references a missing TeamRun",
                    "team_message",
                    &message.id,
                    None,
                )
            })?;
        let team = self
            .latest_teams()?
            .remove(&team_run.agent_team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message TeamRun references a missing AgentTeam",
                    "team_message",
                    &message.id,
                    None,
                )
            })?;
        let host_agent_member_id = self
            .team_host_membership(&context.execution_space_id, &team.id, true)?
            .agent_member_id;
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        if message.sender.kind == ActorKind::AgentMember
            && message.sender.id != host_agent_member_id
            && !runs.iter().any(|run| {
                run.team_run_id == message.team_run_id
                    && run.agent_member_id == message.sender.id
                    && run.coordination_status == MemberCoordinationStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentMember sender has no active MemberRun in the TeamRun",
                "team_message",
                &message.id,
                None,
            ));
        }
        if let Some(work_id) = message.work_id.as_deref() {
            let work = self
                .latest_works_unlocked()?
                .remove(work_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "linked TeamMessage references a missing Work",
                        "work",
                        work_id,
                        None,
                    )
                })?;
            if work.team_run_id != message.team_run_id
                || work.accountable_team_id.as_deref() != Some(team.id.as_str())
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "linked TeamMessage Work must belong to the exact Team and TeamRun",
                    "work",
                    work_id,
                    Some(work.version),
                ));
            }
            let actor_is_host = context.authenticated_actor.kind == ActorKind::AgentMember
                && (context.authenticated_actor.id == host_agent_member_id
                    || context.authority_actor.as_ref().is_some_and(|authority| {
                        authority.kind == ActorKind::AgentMember
                            && authority.id == host_agent_member_id
                    }));
            if !actor_is_host {
                self.require_exact_work_member_unlocked(
                    &context.execution_space_id,
                    &work,
                    &context.authenticated_actor,
                )?;
            }
        }
        let mut seen = BTreeSet::new();
        let mut deliveries = Vec::new();
        for recipient in &message.recipients {
            if recipient.kind != ActorKind::AgentMember || !seen.insert(recipient.id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message recipients must be unique AgentMember references",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let matching = runs
                .iter()
                .filter(|run| {
                    run.team_run_id == message.team_run_id
                        && run.agent_member_id == recipient.id
                        && run.coordination_status != MemberCoordinationStatus::Retired
                })
                .collect::<Vec<_>>();
            if recipient.id == host_agent_member_id && matching.is_empty() {
                continue;
            }
            if matching.len() != 1 {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "recipient must resolve to exactly one non-retired MemberRun in the TeamRun",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let run = matching[0];
            deliveries.push(MessageDelivery {
                id: format!("{}:{}", message.id, run.id),
                message_id: message.id.clone(),
                recipient_member_run_id: run.id.clone(),
                status: MessageDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection_unlocked(
            context,
            "team_message",
            &message.id,
            "created",
            serde_json::to_value(&message)?,
            &message,
            Vec::new(),
            deliveries
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    pub fn create_trust_work_deliveries(
        &self,
        context: &MutationContext,
        work_event_id: &str,
        work_id: &str,
        work_revision: u64,
        recipient_member_run_ids: &[String],
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Vec<WorkDelivery>>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(work_event_id, "work_event_id")?;
        required(work_id, "work_id")?;
        if recipient_member_run_ids.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkEvent requires at least one delivery recipient",
                "work_event",
                work_event_id,
                None,
            ));
        }
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        let mut unique = BTreeSet::new();
        let mut deliveries = Vec::new();
        for run_id in recipient_member_run_ids {
            if !unique.insert(run_id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipients must be unique",
                    "work_event",
                    work_event_id,
                    None,
                ));
            }
            let run = runs.iter().find(|run| run.id == *run_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipient MemberRun does not exist",
                    "work_event",
                    work_event_id,
                    None,
                )
            })?;
            match run.coordination_status {
                MemberCoordinationStatus::Active | MemberCoordinationStatus::Closed => {}
                MemberCoordinationStatus::Retired => {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunRetired,
                        "retired MemberRun rejects new WorkDelivery",
                        "member_run",
                        run_id,
                        Some(run.version),
                    ))
                }
            }
            deliveries.push(WorkDelivery {
                id: format!("{work_event_id}:{run_id}"),
                work_event_id: work_event_id.to_string(),
                work_id: work_id.to_string(),
                work_revision,
                recipient_member_run_id: run_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_event_delivery_batch",
            work_event_id,
            "deliveries_created",
            serde_json::json!({
                "work_event_id": work_event_id,
                "work_id": work_id,
                "work_revision": work_revision,
                "recipients": recipient_member_run_ids,
            }),
            &deliveries,
            Vec::new(),
            deliveries
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    pub(super) fn claimable_member_run(
        &self,
        execution_space_id: &str,
        member_run_id: &str,
        member_generation: u64,
    ) -> StoreResult<MemberRun> {
        let run = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .find(|run| run.id == member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "delivery references a missing MemberRun",
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
        match run.coordination_status {
            MemberCoordinationStatus::Closed => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunClosed,
                    "closed MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunRetired,
                    "retired MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Active => {}
        }
        if run.runtime_generation != member_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery claim used a stale MemberRun generation",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        let member = self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun AgentMember is missing",
                    "agent_member",
                    &run.agent_member_id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => Ok(run),
            AgentMemberOrganizationStatus::Paused => Err(trust_error(
                TrustErrorCode::AgentMemberPaused,
                "paused AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
            AgentMemberOrganizationStatus::Retired => Err(trust_error(
                TrustErrorCode::AgentMemberRetired,
                "retired AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
        }
    }

    #[cfg(any())]
    pub fn claim_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued MessageDelivery may be claimed",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claim.supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery remains frozen for the closed generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[cfg(any())]
    pub fn receive_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            receipt.supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale supervisor or member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[cfg(any())]
    pub fn acknowledge_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim_id: &str,
        member_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::ProviderReceived
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.provider_receipt_id.is_none()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryReceiptMissing,
                "acknowledgement requires the exact claim and provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let claimed_supervisor_generation =
            delivery.claimed_supervisor_generation.ok_or_else(|| {
                trust_error(
                    TrustErrorCode::DeliveryClaimConflict,
                    "acknowledgement requires a claimed Supervisor generation",
                    "message_delivery",
                    delivery_id,
                    Some(delivery.version),
                )
            })?;
        let team_run_id = self
            .trust_message_team_run_unlocked(&context.execution_space_id, &delivery.message_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claimed_supervisor_generation,
            "message_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            member_generation,
        )?;
        if delivery.claimed_member_generation != Some(member_generation) {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "ack used a stale member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "acknowledged",
            serde_json::json!({"claim_id": claim_id, "member_generation": member_generation}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[cfg(any())]
    pub fn reconcile_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed delivery without receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let transition = match outcome {
            DeliveryReconcileOutcome::Acknowledged => {
                delivery.status = MessageDeliveryStatus::Acknowledged;
                "reconciled_acknowledged"
            }
            DeliveryReconcileOutcome::RetrySafeFailure => {
                delivery.status = MessageDeliveryStatus::Failed;
                delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
                delivery.failure_detail = Some(evidence_ref.to_string());
                "reconciled_retry_safe_failure"
            }
        };
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            transition,
            serde_json::json!({"outcome": outcome, "evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    #[cfg(any())]
    pub fn retry_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Failed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only failed MessageDelivery can be retried",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "retried",
            serde_json::json!({"attempt": delivery.attempt}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn claim_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        // Authority must be established before the stale-revision branch below:
        // invalidation is an intentional durable mutation, not a rejection
        // side effect available to an old or caller-invented Supervisor.
        let team_run_id = self.trust_work_team_run_unlocked(&delivery.work_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            claim.supervisor_generation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        if delivery.work_revision != current_work_revision {
            delivery.status = WorkDeliveryStatus::Invalidated;
            delivery.failure_code = Some("WORK_REVISION_STALE".into());
            delivery.version += 1;
            delivery.updated_at = updated_at.to_string();
            let _ = self.commit_trust_projection_unlocked(
                context,
                "work_delivery",
                delivery_id,
                "invalidated_stale_revision",
                serde_json::json!({"current_work_revision": current_work_revision}),
                &delivery,
                vec![serde_json::to_value(&delivery)?],
                Vec::new(),
            )?;
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery revision is stale and was invalidated",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        if delivery.status != WorkDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued WorkDelivery may be claimed",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery remains frozen for the closed generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn receive_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let team_run_id = self.trust_work_team_run_unlocked(&delivery.work_id)?;
        self.require_current_trust_supervisor_unlocked(
            context,
            &team_run_id,
            receipt.supervisor_generation,
            "work_delivery",
            delivery_id,
            Some(delivery.version),
        )?;
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn reconcile_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed WorkDelivery",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
        delivery.failure_detail = Some(evidence_ref.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "reconciled_retry_safe_failure",
            serde_json::json!({"evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn retry_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Failed
            || delivery.work_revision != current_work_revision
        {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery retry requires failed status and exact current Work revision",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(context, "work_delivery", delivery_id, "retried", serde_json::json!({"attempt": delivery.attempt, "work_revision": current_work_revision}), &delivery, vec![serde_json::to_value(&delivery)?], Vec::new())
    }
}
