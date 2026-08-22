use super::*;

impl HarnessStore {
    /// Versioned, append-only responsibility migration (DOC-106). Each legacy
    /// Work gains its durable `accountable_team_id` (resolved through its
    /// TeamRun) and, where one exact TeamMembership exists, its
    /// `assignee_membership_id`. Every resolution is reported; ambiguous or
    /// missing targets fail closed per field and are never guessed. Work IDs,
    /// versions, Operation/Event history, provenance, reports, evidence, gates
    /// and decisions are preserved: the only writes are new `Updated`
    /// WorkOperations appended to the same `work_operations.jsonl` authority.
    pub fn migrate_work_responsibility(
        &self,
        execution_space_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<firm_core::WorkResponsibilityMigrationReport> {
        use firm_core::{
            WorkResponsibilityMigrationEntry, WorkResponsibilityMigrationReport,
            WorkResponsibilityResolution,
        };

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        require_host_actor(&context.performed_by_actor)?;
        // The recovered fold fails closed on provenance conflicts, so the
        // migration never builds on an ambiguous projection.
        let works = self.latest_works_unlocked()?;
        let runs = self.team_runs()?;
        let teams = self.latest_teams()?;
        let memberships = self.fabric_team_memberships(execution_space_id)?;
        let mut entries = Vec::new();
        let mut migrated_work_ids = Vec::new();
        for work in works.values() {
            let accountable_team = match work.accountable_team_id.as_deref() {
                Some(team_id) if teams.contains_key(team_id) => {
                    WorkResponsibilityResolution::AlreadyCanonical
                }
                Some(team_id) => WorkResponsibilityResolution::Unresolved {
                    reason: format!("accountable AgentTeam {team_id} not found in this store"),
                },
                None => match runs.iter().find(|run| run.id == work.team_run_id) {
                    Some(run) if !run.agent_team_id.is_empty() => {
                        if teams.contains_key(&run.agent_team_id) {
                            WorkResponsibilityResolution::Resolved {
                                value: run.agent_team_id.clone(),
                            }
                        } else {
                            WorkResponsibilityResolution::Unresolved {
                                    reason: format!(
                                        "TeamRun {} resolves to AgentTeam {} which is not present in this store",
                                        run.id, run.agent_team_id
                                    ),
                                }
                        }
                    }
                    Some(run) => WorkResponsibilityResolution::Unresolved {
                        reason: format!("TeamRun {} has no durable AgentTeam identity", run.id),
                    },
                    None => WorkResponsibilityResolution::Unresolved {
                        reason: format!(
                            "TeamRun {} not found; cannot resolve the accountable Team",
                            work.team_run_id
                        ),
                    },
                },
            };
            let resolved_team_id = match &accountable_team {
                WorkResponsibilityResolution::AlreadyCanonical => work.accountable_team_id.clone(),
                WorkResponsibilityResolution::Resolved { value } => Some(value.clone()),
                _ => None,
            };
            let assignee = match (
                work.assignee_membership_id.as_deref(),
                work.owner_member_id.as_deref(),
            ) {
                (Some(membership_id), _) => {
                    match memberships
                        .iter()
                        .find(|membership| membership.id == membership_id)
                    {
                        Some(membership)
                            if Some(membership.team_id.as_str())
                                == resolved_team_id.as_deref() =>
                        {
                            WorkResponsibilityResolution::AlreadyCanonical
                        }
                        Some(membership) => WorkResponsibilityResolution::Unresolved {
                            reason: format!(
                                "assignee TeamMembership {membership_id} belongs to Team {}, not the accountable Team {}",
                                membership.team_id,
                                resolved_team_id.as_deref().unwrap_or("<unresolved>")
                            ),
                        },
                        None => WorkResponsibilityResolution::Unresolved {
                            reason: format!(
                                "assignee TeamMembership {membership_id} not found in Execution Space {execution_space_id}"
                            ),
                        },
                    }
                }
                (None, None) => WorkResponsibilityResolution::Unassigned,
                (None, Some(owner)) => match resolved_team_id.as_deref() {
                    None => WorkResponsibilityResolution::Unresolved {
                        reason: "accountable Team is unresolved; assignee cannot be derived safely"
                            .to_string(),
                    },
                    Some(team_id) => {
                        let matching = memberships
                            .iter()
                            .filter(|membership| {
                                membership.team_id == team_id && membership.agent_member_id == owner
                            })
                            .collect::<Vec<_>>();
                        let active = matching
                            .iter()
                            .filter(|membership| {
                                membership.state
                                    == firm_core::agentfirm_api::TeamMembershipStatus::Active
                            })
                            .collect::<Vec<_>>();
                        if active.len() == 1 {
                            WorkResponsibilityResolution::Resolved {
                                value: active[0].id.clone(),
                            }
                        } else if active.is_empty() && matching.len() == 1 {
                            WorkResponsibilityResolution::Resolved {
                                value: matching[0].id.clone(),
                            }
                        } else if matching.is_empty() {
                            WorkResponsibilityResolution::Unresolved {
                                reason: format!(
                                    "no TeamMembership binds AgentMember {owner} in Team {team_id}"
                                ),
                            }
                        } else {
                            WorkResponsibilityResolution::Unresolved {
                                reason: format!(
                                    "ambiguous: {} TeamMemberships ({} Active) bind AgentMember {owner} in Team {team_id}",
                                    matching.len(),
                                    active.len()
                                ),
                            }
                        }
                    }
                },
            };
            let needs_team_write = matches!(
                accountable_team,
                WorkResponsibilityResolution::Resolved { .. }
            );
            let needs_assignee_write =
                matches!(assignee, WorkResponsibilityResolution::Resolved { .. });
            let mut to_version = None;
            if needs_team_write || needs_assignee_write {
                self.ensure_work_event_id_available_unlocked(&format!(
                    "{}:{}",
                    context.event_id, work.id
                ))?;
                let mut next = work.clone();
                if let WorkResponsibilityResolution::Resolved { value } = &accountable_team {
                    next.accountable_team_id = Some(value.clone());
                }
                if let WorkResponsibilityResolution::Resolved { value } = &assignee {
                    next.assignee_membership_id = Some(value.clone());
                }
                next.version += 1;
                next.updated_at = context.created_at.clone();
                let operation = WorkOperation {
                    event: WorkEvent {
                        id: format!("{}:{}", context.event_id, work.id),
                        team_run_id: work.team_run_id.clone(),
                        work_id: work.id.clone(),
                        sequence: self
                            .work_operations_unlocked()?
                            .iter()
                            .filter(|operation| operation.work.id == work.id)
                            .count() as u64
                            + 1,
                        kind: WorkEventKind::Updated,
                        expected_version: work.version,
                        resulting_version: next.version,
                        performed_by_actor: context.performed_by_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: context.causation_ref.clone(),
                        idempotency_key: format!("{}:{}", context.idempotency_key, work.id),
                        payload: serde_json::json!({
                            "responsibility_migration": true,
                            "accountable_team": accountable_team,
                            "assignee": assignee,
                        }),
                        created_at: context.created_at.clone(),
                    },
                    work: next,
                    condition_records: Vec::new(),
                    reports: Vec::new(),
                    evidence_records: Vec::new(),
                    decisions: Vec::new(),
                    deliveries: Vec::new(),
                    delivery_updates: Vec::new(),
                    delegation_revisions: Vec::new(),
                };
                self.append_work_operation_unlocked(&operation)?;
                to_version = Some(work.version + 1);
                migrated_work_ids.push(work.id.clone());
            }
            entries.push(WorkResponsibilityMigrationEntry {
                work_id: work.id.clone(),
                from_version: work.version,
                to_version,
                accountable_team,
                assignee,
            });
        }
        Ok(WorkResponsibilityMigrationReport {
            execution_space_id: execution_space_id.to_string(),
            migrated_work_ids,
            entries,
            created_at: context.created_at,
        })
    }

    pub(super) fn initial_work_deliveries_unlocked(
        &self,
        work: &Work,
        event_id: &str,
        updated_at: &str,
    ) -> StoreResult<Vec<ProviderWorkDispatch>> {
        let Some(member_run_id) = work.active_member_run_id.as_deref() else {
            return Ok(Vec::new());
        };
        let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(Vec::new());
        }
        // Skip loopback deliveries for terminal work: the owning member
        // already knows their work is Done/Cancelled — self-notification is
        // redundant. Non-terminal events (Created, Assigned, ChangesRequested,
        // Resumed, Rebound) genuinely need delivery even to the owner.
        if work.is_terminal() {
            if let Some(ref owner_id) = work.owner_member_id {
                if owner_id == &member_identity(&member) {
                    return Ok(Vec::new());
                }
            }
        }
        Ok(vec![ProviderWorkDispatch {
            id: format!("work-delivery-{event_id}-{member_run_id}"),
            work_event_id: event_id.to_string(),
            team_run_id: work.team_run_id.clone(),
            work_id: work.id.clone(),
            work_version: work.version,
            recipient_member_run_id: member_run_id.to_string(),
            status: ProviderWorkDispatchStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: updated_at.to_string(),
        }])
    }

    pub(super) fn current_work_unlocked(
        &self,
        work_id: &str,
        expected_version: u64,
    ) -> StoreResult<Work> {
        let current = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        Ok(current)
    }

    pub(super) fn ensure_deliveries_reassignable_unlocked(&self, work: &Work) -> StoreResult<()> {
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work.id
                    && delivery.work_version == work.version
                    && work.active_member_run_id.as_deref()
                        == Some(delivery.recipient_member_run_id.as_str())
                    && matches!(
                        delivery.status,
                        ProviderWorkDispatchStatus::Claimed
                            | ProviderWorkDispatchStatus::ProviderReceived
                    )
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work delivery was already accepted".to_string(),
            ));
        }
        Ok(())
    }

    /// Return an exact idempotent retry, while rejecting accidental reuse of
    /// the same key for a different Work or command. A bare key is not enough
    /// to identify an operation safely: without this fingerprint a retry of
    /// `start(work-a)` could silently return the result of `cancel(work-b)`.
    pub(super) fn idempotent_work_operation_unlocked(
        &self,
        idempotency_key: &str,
        work_id: &str,
        kind: WorkEventKind,
    ) -> StoreResult<Option<WorkOperation>> {
        let existing = self
            .work_operations_with_recovered_provenance_unlocked()?
            .into_iter()
            .find(|operation| operation.event.idempotency_key == idempotency_key);
        let Some(existing) = existing else {
            return Ok(None);
        };
        if existing.event.work_id != work_id || existing.event.kind != kind {
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {idempotency_key} already belongs to {:?} on Work {}",
                existing.event.kind, existing.event.work_id
            )));
        }
        // If the original process crashed after fsyncing the WorkOperation but
        // before its derived HostAttention row, the ordinary idempotent retry
        // repairs that gap before returning the already-applied Work result.
        self.ensure_downstream_host_attentions_for_work_operation_unlocked(&existing)?;
        self.ensure_host_attention_for_work_operation_unlocked(&existing)?;
        Ok(Some(existing))
    }

    pub(super) fn work_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        let mut operations: Vec<WorkOperation> = self.read_jsonl("work_operations.jsonl")?;
        let mut delegated = self
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")?
            .into_iter()
            .map(|operation| operation.target_work_operation)
            .collect::<Vec<_>>();
        // WorkDelegation creation is crash-atomic in a separate composite
        // ledger, while later target transitions use the ordinary Work ledger.
        // Concatenating files would place every delegated Work's version 1
        // after its later versions and make the projection regress. Preserve
        // the ordinary ledger's exact append order (the durable `--since`
        // cursor), then insert each composite creation at its temporal slot
        // and always before any later revision of that same Work.
        delegated.sort_by(|left, right| work_event_order(&left.event, &right.event));
        for operation in delegated {
            let same_work = operations
                .iter()
                .position(|existing| existing.work.id == operation.work.id)
                .unwrap_or(operations.len());
            let temporal = operations
                .iter()
                .position(|existing| work_event_order(&operation.event, &existing.event).is_lt())
                .unwrap_or(operations.len());
            operations.insert(same_work.min(temporal), operation);
        }
        Ok(operations)
    }

    pub(super) fn all_work_delegation_revisions_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let mut revisions = self
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")?
            .into_iter()
            .map(|operation| WorkDelegationRevision {
                delegation: operation.delegation,
                event: operation.event,
            })
            .collect::<Vec<_>>();
        revisions
            .extend(self.read_jsonl::<WorkDelegationRevision>("work_delegation_events.jsonl")?);
        revisions.extend(
            self.work_operations_unlocked()?
                .into_iter()
                .flat_map(|operation| operation.delegation_revisions),
        );
        revisions.extend(self.trust_work_delegation_revisions_unlocked()?);
        revisions.sort_by(|left, right| {
            left.delegation
                .id
                .cmp(&right.delegation.id)
                .then(left.event.sequence.cmp(&right.event.sequence))
        });
        Ok(revisions)
    }

    pub(super) fn latest_work_delegations_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, WorkDelegation>> {
        let mut latest = std::collections::BTreeMap::<String, WorkDelegation>::new();
        for revision in self.all_work_delegation_revisions_unlocked()? {
            revision
                .delegation
                .validate()
                .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
            revision.event.validate().map_err(|error| {
                StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}"))
            })?;
            if revision.event.delegation_id != revision.delegation.id
                || revision.event.resulting_version != revision.delegation.version
                || revision.event.expected_version.saturating_add(1)
                    != revision.event.resulting_version
            {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_LEDGER_CORRUPT: event {} does not match projection {} version {}",
                    revision.event.id, revision.delegation.id, revision.delegation.version
                )));
            }
            if let Some(current) = latest.get(&revision.delegation.id) {
                if revision.event.expected_version != current.version {
                    return Err(StoreError::Conflict(format!(
                        "DELEGATION_LEDGER_CORRUPT: Delegation {} expected version {}, current {}",
                        revision.delegation.id, revision.event.expected_version, current.version
                    )));
                }
            } else if revision.event.expected_version != 0 {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_LEDGER_CORRUPT: Delegation {} does not start at version 1",
                    revision.delegation.id
                )));
            }
            latest.insert(revision.delegation.id.clone(), revision.delegation);
        }
        Ok(latest)
    }

    /// Compute every Delegation transition caused by one authoritative target
    /// Work projection. Callers that are already committing a WorkOperation
    /// embed these revisions in that same row; the public reconciler uses the
    /// identical reducer to repair older split-ledger crash gaps.
    pub(super) fn work_delegation_rollup_revisions_unlocked(
        &self,
        target: &Work,
        context: &WorkCommandContext,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let existing_revisions = self.all_work_delegation_revisions_unlocked()?;
        let current = self
            .latest_work_delegations_unlocked()?
            .into_values()
            .filter(|delegation| delegation.target_work_ref.work_id == target.id)
            .collect::<Vec<_>>();
        let mut revisions = Vec::new();
        for delegation in current {
            let desired = if target.phase == WorkPhase::Closed {
                match target.resolution {
                    Some(WorkResolution::Accepted) => Some((
                        WorkDelegationState::Completed,
                        WorkDelegationTransition::Completed,
                        target
                            .result_summary
                            .clone()
                            .unwrap_or_else(|| "target Work accepted".to_string()),
                        None,
                    )),
                    Some(WorkResolution::Failed) => Some((
                        WorkDelegationState::Failed,
                        WorkDelegationTransition::Failed,
                        target
                            .result_summary
                            .clone()
                            .or_else(|| target.blocker_reason.clone())
                            .unwrap_or_else(|| "target Work failed".to_string()),
                        None,
                    )),
                    Some(WorkResolution::Cancelled) => Some((
                        WorkDelegationState::Cancelled,
                        WorkDelegationTransition::Cancelled,
                        target
                            .result_summary
                            .clone()
                            .or_else(|| target.blocker_reason.clone())
                            .unwrap_or_else(|| "target Work cancelled".to_string()),
                        None,
                    )),
                    None => None,
                }
            } else if target.condition == WorkCondition::Blocked {
                Some((
                    WorkDelegationState::Blocked,
                    WorkDelegationTransition::Blocked,
                    String::new(),
                    Some(
                        target
                            .blocker_reason
                            .clone()
                            .unwrap_or_else(|| "target Work blocked".to_string()),
                    ),
                ))
            } else if delegation.state == WorkDelegationState::Blocked {
                Some((
                    WorkDelegationState::Active,
                    WorkDelegationTransition::Resumed,
                    String::new(),
                    None,
                ))
            } else {
                None
            };
            let Some((state, transition, resolution, blocker)) = desired else {
                continue;
            };
            if delegation.state == state
                || matches!(
                    delegation.state,
                    WorkDelegationState::Completed
                        | WorkDelegationState::Failed
                        | WorkDelegationState::Cancelled
                )
            {
                continue;
            }
            let mut next = delegation.clone();
            next.state = state;
            next.version = next.version.saturating_add(1);
            next.updated_at = context.created_at.clone();
            next.blocker_reason = blocker;
            next.resolution_summary = if resolution.is_empty() {
                None
            } else {
                Some(resolution)
            };
            let event = WorkDelegationEvent {
                id: format!("{}:delegation:{}", context.event_id, delegation.id),
                delegation_id: delegation.id.clone(),
                sequence: next.version,
                transition,
                expected_version: delegation.version,
                resulting_version: next.version,
                performed_by_actor: context.performed_by_actor.clone(),
                causation_ref: context.causation_ref.clone(),
                idempotency_key: format!(
                    "{}:delegation:{}",
                    context.idempotency_key, delegation.id
                ),
                payload: serde_json::json!({"target_work_version": target.version}),
                created_at: context.created_at.clone(),
            };
            next.validate()
                .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
            event.validate().map_err(|error| {
                StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}"))
            })?;
            if existing_revisions.iter().any(|revision| {
                revision.event.id == event.id
                    || revision.event.idempotency_key == event.idempotency_key
            }) {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_EVENT_CONFLICT: {}",
                    event.id
                )));
            }
            revisions.push(WorkDelegationRevision {
                delegation: next,
                event,
            });
        }
        Ok(revisions)
    }

    pub(super) fn append_work_delegation_transition_unlocked(
        &self,
        current: &WorkDelegation,
        next: WorkDelegation,
        event: WorkDelegationEvent,
    ) -> StoreResult<WorkDelegation> {
        let latest = self
            .latest_work_delegations_unlocked()?
            .remove(&current.id)
            .ok_or_else(|| StoreError::Conflict(format!("delegation not found: {}", current.id)))?;
        if latest != *current {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_VERSION_CONFLICT: {} changed concurrently",
                current.id
            )));
        }
        if next.id != current.id
            || next.source_work_ref != current.source_work_ref
            || next.source_work_version != current.source_work_version
            || next.source_owner_member_id != current.source_owner_member_id
            || next.created_by_member_run_id != current.created_by_member_run_id
            || next.target_agent_team_id != current.target_agent_team_id
            || next.target_work_ref != current.target_work_ref
            || next.delegated_by_actor != current.delegated_by_actor
            || next.created_at != current.created_at
            || next.version != current.version.saturating_add(1)
            || event.delegation_id != current.id
            || event.expected_version != current.version
            || event.resulting_version != next.version
        {
            return Err(StoreError::Conflict(
                "DELEGATION_TRANSITION_INVALID: immutable identity or CAS fields changed"
                    .to_string(),
            ));
        }
        let legal = matches!(
            (current.state, next.state),
            (WorkDelegationState::Active, WorkDelegationState::Blocked)
                | (WorkDelegationState::Blocked, WorkDelegationState::Active)
                | (WorkDelegationState::Active, WorkDelegationState::Completed)
                | (WorkDelegationState::Blocked, WorkDelegationState::Completed)
                | (WorkDelegationState::Active, WorkDelegationState::Failed)
                | (WorkDelegationState::Blocked, WorkDelegationState::Failed)
                | (WorkDelegationState::Active, WorkDelegationState::Cancelled)
                | (WorkDelegationState::Blocked, WorkDelegationState::Cancelled)
        );
        if !legal {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_TRANSITION_INVALID: {:?}->{:?}",
                current.state, next.state
            )));
        }
        next.validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
        event
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}")))?;
        if self
            .all_work_delegation_revisions_unlocked()?
            .iter()
            .any(|revision| {
                revision.event.id == event.id
                    || revision.event.idempotency_key == event.idempotency_key
            })
        {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_EVENT_CONFLICT: {}",
                event.id
            )));
        }
        self.append_jsonl_unlocked(
            "work_delegation_events.jsonl",
            &WorkDelegationRevision {
                delegation: next.clone(),
                event,
            },
        )?;
        Ok(next)
    }

    /// Fold immutable additive provenance through every WorkOperation.
    ///
    /// Mixed-version writers may deserialize a newer complete projection,
    /// discard unknown fields, and append a later row without `team_id` or
    /// `created_by_member_id`. Once either fact has been established, no Work
    /// command is allowed to remove or change it. Reads therefore recover a
    /// missing later value from ordered WorkOperation ledger history, while a
    /// conflicting non-null value remains corruption and is refused.
    pub(super) fn work_operations_with_recovered_provenance_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkOperation>> {
        let mut team_ids = std::collections::BTreeMap::<String, String>::new();
        let mut creator_ids = std::collections::BTreeMap::<String, String>::new();
        let mut recovered = Vec::new();
        for mut operation in self.work_operations_unlocked()? {
            let work_id = operation.work.id.clone();
            match (
                team_ids.get(&work_id),
                operation.work.accountable_team_id.as_deref(),
            ) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed accountable_team_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => {
                    operation.work.accountable_team_id = Some(expected.clone())
                }
                (None, Some(actual)) => {
                    team_ids.insert(work_id.clone(), actual.to_string());
                }
                _ => {}
            }
            match (
                creator_ids.get(&work_id),
                operation.work.created_by_member_id.as_deref(),
            ) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed created_by_member_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => {
                    operation.work.created_by_member_id = Some(expected.clone())
                }
                (None, Some(actual)) => {
                    creator_ids.insert(work_id, actual.to_string());
                }
                _ => {}
            }
            recovered.push(operation);
        }
        Ok(recovered)
    }

    /// Current-version writers must emit a complete projection. This guard is
    /// the refusal half of mixed-schema compatibility; the recovery fold above
    /// is the lossless-preservation half for sparse rows already appended by a
    /// stale binary.
    pub(super) fn append_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<()> {
        operation
            .work
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        let existing_operations = self.work_operations_unlocked()?;
        let existing_record_ids = existing_operations
            .iter()
            .flat_map(|row| {
                row.condition_records
                    .iter()
                    .map(|record| record.id.as_str())
                    .chain(row.reports.iter().map(|record| record.id.as_str()))
                    .chain(row.evidence_records.iter().map(|record| record.id.as_str()))
                    .chain(row.decisions.iter().map(|record| record.id.as_str()))
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut new_record_ids = std::collections::BTreeSet::new();
        for (id, work_id, validation) in operation
            .condition_records
            .iter()
            .map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            })
            .chain(operation.reports.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
            .chain(operation.evidence_records.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
            .chain(operation.decisions.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
        {
            validation.map_err(|error| {
                StoreError::Conflict(format!("INVALID_WORK_RECORD {id}: {error}"))
            })?;
            if work_id != operation.work.id {
                return Err(StoreError::Conflict(format!(
                    "WORK_RECORD_SCOPE_MISMATCH: record {id} belongs to Work {work_id}, operation belongs to {}",
                    operation.work.id
                )));
            }
            if existing_record_ids.contains(id) || !new_record_ids.insert(id) {
                return Err(StoreError::Conflict(format!(
                    "WORK_RECORD_ID_CONFLICT: record id {id} is already in use"
                )));
            }
        }
        for report in &operation.reports {
            if report.work_version != operation.work.version {
                return Err(StoreError::Conflict(format!(
                    "WORK_REPORT_VERSION_MISMATCH: report {} binds Work version {}, operation produced {}",
                    report.id, report.work_version, operation.work.version
                )));
            }
            let matching_evidence = operation.evidence_records.iter().any(|evidence| {
                evidence.work_report_id == report.id
                    && evidence.work_version == report.work_version
                    && evidence.candidate_revision == report.candidate_revision
                    && report.evidence_refs.contains(&evidence.id)
            });
            if !matching_evidence {
                return Err(StoreError::Conflict(format!(
                    "WORK_REPORT_EVIDENCE_MISMATCH: report {} lacks exact candidate evidence",
                    report.id
                )));
            }
        }
        if let Some(current) = self
            .latest_works_unlocked()?
            .remove(operation.work.id.as_str())
        {
            if current.accountable_team_id.is_some()
                && operation.work.accountable_team_id != current.accountable_team_id
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change accountable_team_id",
                    operation.work.id, operation.event.id
                )));
            }
            if current.created_by_member_id.is_some()
                && operation.work.created_by_member_id != current.created_by_member_id
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change created_by_member_id",
                    operation.work.id, operation.event.id
                )));
            }
        }
        self.append_jsonl_unlocked("work_operations.jsonl", operation)
    }

    pub(super) fn ensure_work_event_id_available_unlocked(
        &self,
        event_id: &str,
    ) -> StoreResult<()> {
        if self
            .work_operations_unlocked()?
            .iter()
            .any(|operation| operation.event.id == event_id)
        {
            return Err(StoreError::Conflict(format!(
                "WORK_EVENT_ID_CONFLICT: event id {event_id} is already in use"
            )));
        }
        Ok(())
    }

    pub(super) fn next_work_delivery_update_sequence_unlocked(&self) -> StoreResult<u64> {
        let embedded_max = self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.delivery_updates)
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        let standalone_max = self
            .read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")?
            .into_iter()
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        Ok(embedded_max.max(standalone_max).saturating_add(1))
    }

    pub(super) fn latest_works_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, Work>> {
        let mut latest = latest_by_id(
            self.work_operations_with_recovered_provenance_unlocked()?,
            |operation| operation.work.id.clone(),
        )
        .into_iter()
        .map(|(id, operation)| (id, operation.work))
        .collect::<std::collections::BTreeMap<_, _>>();
        for work in self.trust_work_projections_unlocked()? {
            match latest.get(&work.id) {
                Some(current) if current.version >= work.version => {}
                _ => {
                    latest.insert(work.id.clone(), work);
                }
            }
        }
        Ok(latest)
    }

    pub(super) fn latest_work_deliveries_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, ProviderWorkDispatch>> {
        let mut deliveries = std::collections::BTreeMap::new();
        let mut legacy_updates = Vec::new();
        let mut sequenced_updates = Vec::new();
        let mut legacy_order = 0_u64;
        for operation in self.work_operations_unlocked()? {
            for delivery in operation.deliveries {
                deliveries.insert(delivery.id.clone(), delivery);
            }
            for update in operation.delivery_updates {
                if update.update_sequence == 0 {
                    legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                    legacy_order = legacy_order.saturating_add(1);
                } else {
                    sequenced_updates.push(update);
                }
            }
        }
        for update in
            self.read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")?
        {
            if update.update_sequence == 0 {
                legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                legacy_order = legacy_order.saturating_add(1);
            } else {
                sequenced_updates.push(update);
            }
        }
        // Rows written before update_sequence existed remain readable. Their
        // best available ordering evidence is timestamp plus stable file-scan
        // order. All new writes are then folded by the Store-assigned sequence,
        // independent of caller clocks or which JSONL file carries the update.
        legacy_updates.sort_by(|left, right| {
            compare_store_timestamps(&left.0, &right.0).then(left.1.cmp(&right.1))
        });
        sequenced_updates.sort_by_key(|update| update.update_sequence);
        for update in legacy_updates
            .into_iter()
            .map(|(_, _, update)| update)
            .chain(sequenced_updates)
        {
            if let Some(delivery) = deliveries.get_mut(&update.delivery_id) {
                apply_work_delivery_update(delivery, update);
            }
        }
        Ok(deliveries)
    }
}
