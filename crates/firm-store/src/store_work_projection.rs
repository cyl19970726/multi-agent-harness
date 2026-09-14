use super::*;

impl HarnessStore {
    pub fn work_responsibility_changed_after_revision(
        &self,
        work_id: &str,
        revision: u64,
    ) -> StoreResult<bool> {
        self.work_responsibility_changed_after_revision_unlocked(work_id, revision)
    }

    /// Did this Work's responsibility move after `revision`?
    ///
    /// Reads the one Work journal: an execution binding is fenced by a
    /// responsibility change, and every responsibility transition has lived in
    /// the trust journal since the W4 writer cutover. A ledger-only answer
    /// would report "unchanged" for every current reassignment and let a stale
    /// binding keep executing.
    pub(super) fn work_responsibility_changed_after_revision_unlocked(
        &self,
        work_id: &str,
        revision: u64,
    ) -> StoreResult<bool> {
        Ok(self.work_journal_unlocked()?.records.iter().any(|record| {
            record.work.id == work_id
                && record.event.resulting_version > revision
                && matches!(
                    record.event.kind,
                    WorkEventKind::Assigned
                        | WorkEventKind::Claimed
                        | WorkEventKind::Released
                        | WorkEventKind::Rebound
                        | WorkEventKind::ExecutionRetargeted
                        | WorkEventKind::ExecutionRecovered
                )
        }))
    }

    /// Versioned, append-only responsibility migration (DOC-106). Each legacy
    /// Work gains its durable `accountable_team_id` (resolved through its
    /// TeamRun) and, where one exact TeamMembership exists, its
    /// `assignee_membership_id`. Every resolution is reported; ambiguous or
    /// missing targets fail closed per field and are never guessed. Work IDs,
    /// versions, Operation/Event history, provenance, reports, evidence, gates
    /// and decisions are preserved: the only writes are new `Updated`
    /// WorkOperations appended to the same `work_operations.jsonl` authority.
    /// `team_run_scope` restricts the sweep to one TeamRun. Every Work the
    /// migration writes is gated on the exact Host of that Work's own TeamRun,
    /// so a store whose Work spans several Teams migrates one Host at a time
    /// instead of letting any Host-shaped actor rewrite all of it.
    pub fn migrate_work_responsibility(
        &self,
        execution_space_id: &str,
        team_run_scope: Option<&str>,
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
        // Plan every write first. Authority and mutability are checked per
        // Work, so a refusal on a later Work must not leave earlier ones
        // already appended: nothing is written until the whole sweep is
        // proven legal.
        let mut planned = Vec::new();
        for work in works.values() {
            if team_run_scope.is_some_and(|scope| work.team_run_id != scope) {
                continue;
            }
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
                // The same authority rule every other Host Work verb uses: the
                // exact Host actor stored on this Work's own TeamRun, proven
                // against the AgentTeam Host and its active Host membership.
                self.require_exact_team_run_host_actor(
                    &context.performed_by_actor,
                    &work.team_run_id,
                )?;
                require_mutable_work(
                    work,
                    "a closed Work keeps the responsibility it settled with",
                )?;
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
                require_valid_work_transition(work, &next, WorkEventKind::Updated)?;
                let payload = serde_json::json!({
                    "responsibility_migration": true,
                    "accountable_team": accountable_team,
                    "assignee": assignee,
                });
                let operation = WorkOperation {
                    event: WorkEvent {
                        id: format!("{}:{}", context.event_id, work.id),
                        team_run_id: work.team_run_id.clone(),
                        work_id: work.id.clone(),
                        sequence: self
                            .work_journal_unlocked()?
                            .records
                            .iter()
                            .filter(|record| record.work.id == work.id)
                            .count() as u64
                            + 1,
                        kind: WorkEventKind::Updated,
                        expected_version: work.version,
                        resulting_version: next.version,
                        performed_by_actor: context.performed_by_actor.clone(),
                        authority_actor: context.authority_actor.clone(),
                        causation_ref: context.causation_ref.clone(),
                        idempotency_key: format!("{}:{}", context.idempotency_key, work.id),
                        payload: payload.clone(),
                        created_at: context.created_at.clone(),
                        executed_by_member_run_id: None,
                    },
                    work: next.clone(),
                    condition_records: Vec::new(),
                    reports: Vec::new(),
                    evidence_records: Vec::new(),
                };
                self.validate_work_operation_records_unlocked(&operation)?;
                let run = self.require_team_run_unlocked(&work.team_run_id)?;
                let mutation_context = firm_core::agentfirm_api::MutationContext {
                    execution_space_id: self.current_team_run_execution_space_unlocked(&run)?,
                    authenticated_actor: self.canonical_work_actor_unlocked(
                        &context.performed_by_actor,
                        &work.team_run_id,
                    ),
                    authority_actor: context
                        .authority_actor
                        .as_ref()
                        .map(|actor| self.canonical_work_actor_unlocked(actor, &work.team_run_id)),
                    command_name: "work.responsibility.migrate".to_string(),
                    idempotency_key: operation.event.idempotency_key.clone(),
                    expected_version: work.version,
                    request_fingerprint: None,
                };
                planned.push(crate::trust_kernel::CurrentWorkMutation {
                    context: mutation_context,
                    transition: WorkEventKind::Updated.canonical_transition().to_string(),
                    request_payload: payload,
                    work: next,
                    immutable_side_records: vec![serde_json::to_value(&operation)?],
                    initial_outbox_records: Vec::new(),
                });
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
        // One atomic write for the whole sweep: a partially applied migration
        // has no single revision to reconcile from.
        self.commit_current_work_mutations_atomic_unlocked(&planned)?;
        Ok(WorkResponsibilityMigrationReport {
            execution_space_id: execution_space_id.to_string(),
            migrated_work_ids,
            entries,
            created_at: context.created_at,
        })
    }

    /// Read the Work a command is fencing against, in the command's own
    /// Execution Space.
    ///
    /// The writer must resolve the same revision its caller read. Every scoped
    /// reader narrows to one space, so a writer that resolved through the
    /// unscoped fold could see a revision written in another space — during a
    /// recovery or import a physical store may hold more than one — and refuse
    /// a Host's write with a VERSION_CONFLICT against a revision that Host can
    /// never read. Reader and writer therefore use the same scope.
    pub(super) fn current_work_unlocked(
        &self,
        execution_space_id: &ExecutionSpaceId,
        work_id: &str,
        expected_version: u64,
    ) -> StoreResult<Work> {
        let current = self
            .latest_works_in_space_unlocked(execution_space_id)?
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

    pub(super) fn canonical_work_deliveries_for_work_unlocked(
        &self,
        work: &Work,
    ) -> StoreResult<Vec<CanonicalWorkDelivery>> {
        let run = self.require_team_run_unlocked(&work.team_run_id)?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        Ok(self
            .fabric_work_deliveries(&execution_space_id)?
            .into_iter()
            .filter(|delivery| delivery.work_id == work.id)
            .collect())
    }

    pub(super) fn ensure_deliveries_reassignable_unlocked(&self, work: &Work) -> StoreResult<()> {
        if self
            .canonical_work_deliveries_for_work_unlocked(work)?
            .iter()
            .any(|delivery| {
                delivery.work_revision == work.version
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
                    )
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work delivery was already accepted".to_string(),
            ));
        }
        Ok(())
    }

    /// Host cancellation is a responsibility-plane decision. A durable
    /// provider receipt is therefore preserved as evidence instead of
    /// blocking cancellation, while an unsettled claim remains uncertain and
    /// must fail closed.
    pub(super) fn ensure_no_claimed_delivery_unlocked(&self, work: &Work) -> StoreResult<()> {
        if self
            .canonical_work_deliveries_for_work_unlocked(work)?
            .iter()
            .any(|delivery| {
                delivery.work_revision == work.version
                    && delivery.status == WorkDeliveryStatus::Claimed
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work delivery claim is unsettled".to_string(),
            ));
        }
        Ok(())
    }

    /// The legacy `work_operations.jsonl` fold. Nothing writes these rows since
    /// W4; the Work journal reader and `work_record_operations_unlocked` are
    /// what current callers want. The crash-atomic delegation composite this
    /// fold used to merge retired with the Work-ledger delegation stack, and a
    /// store that still holds `work_delegation_operations.jsonl` simply ignores
    /// it.
    pub(super) fn work_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        self.read_jsonl::<WorkOperation>("work_operations.jsonl")
    }

    /// Fold immutable additive provenance through every WorkOperation.
    ///
    /// Mixed-version writers may deserialize a newer complete projection,
    /// discard unknown fields, and append a later row without `team_id` or
    /// `created_by_member_id`. Once either fact has been established, no Work
    /// command is allowed to remove or change it. Reads therefore recover a
    /// missing later value from ordered WorkOperation ledger history, while a
    /// conflicting non-null value remains corruption and is refused.
    pub(super) fn recover_work_operation_provenance(
        &self,
        operations: Vec<WorkOperation>,
    ) -> StoreResult<Vec<WorkOperation>> {
        let mut team_ids = std::collections::BTreeMap::<String, String>::new();
        let mut creator_ids = std::collections::BTreeMap::<String, String>::new();
        let mut recovered = Vec::new();
        for mut operation in operations {
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
    ///
    /// Since W4 this validates the operation a writer is about to commit into
    /// a `work` trust envelope; the ledger file it once appended to is legacy
    /// read-only input. Uniqueness of the per-row record ids is therefore
    /// checked across every journal, not just one file.
    pub(super) fn validate_work_operation_records_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<()> {
        operation
            .work
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        let existing_operations = self.work_record_operations_unlocked()?;
        let existing_record_ids = existing_operations
            .iter()
            .flat_map(|row| {
                row.condition_records
                    .iter()
                    .map(|record| record.id.as_str())
                    .chain(row.reports.iter().map(|record| record.id.as_str()))
                    .chain(row.evidence_records.iter().map(|record| record.id.as_str()))
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
        Ok(())
    }

    /// Append a raw legacy `work_operations.jsonl` row.
    ///
    /// Test-only since W4: no production writer appends to this file any more.
    /// It stays so the reader's legacy half can be exercised with rows that
    /// really are in the shape a pre-cutover binary wrote.
    #[cfg(test)]
    pub(crate) fn append_legacy_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<()> {
        self.validate_work_operation_records_unlocked(operation)?;
        self.append_jsonl_unlocked("work_operations.jsonl", operation)
    }

    /// Event ids are unique across the whole Work journal, not one file: a
    /// HostAttention, a delivery and a Delegation revision all name a Work
    /// revision by its event id, so two journals may not mint the same one.
    pub(super) fn ensure_work_event_id_available_unlocked(
        &self,
        event_id: &str,
    ) -> StoreResult<()> {
        if self.work_journal_event_ids_unlocked()?.contains(event_id) {
            return Err(StoreError::Conflict(format!(
                "WORK_EVENT_ID_CONFLICT: event id {event_id} is already in use"
            )));
        }
        Ok(())
    }
}
