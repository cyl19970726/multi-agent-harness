use super::*;

impl HarnessStore {
    /// Raw append-only Mission ledger rows, in append order.
    pub fn missions(&self) -> StoreResult<Vec<Mission>> {
        self.read_jsonl("missions.jsonl")
    }

    /// Latest-row-wins Mission projection, ordered by id for deterministic
    /// dashboard/API consumers.
    pub fn latest_missions(&self) -> StoreResult<Vec<Mission>> {
        Ok(latest_by_id(self.missions()?, |mission| mission.id.clone())
            .into_values()
            .collect())
    }

    /// Raw append-only Legacy Wave ledger rows, in append order.
    pub fn legacy_waves(&self) -> StoreResult<Vec<LegacyWave>> {
        self.read_jsonl("waves.jsonl")
    }

    /// Latest-row-wins Legacy Wave projection, ordered by Mission then legacy
    /// index for deterministic historical reads. The id is a final tie-breaker
    /// for corrupt rows.
    pub fn latest_legacy_waves(&self) -> StoreResult<Vec<LegacyWave>> {
        let mut waves = latest_by_id(self.legacy_waves()?, |wave| wave.id.clone())
            .into_values()
            .collect::<Vec<_>>();
        waves.sort_by(|left, right| {
            left.mission_id
                .cmp(&right.mission_id)
                .then(left.index.cmp(&right.index))
                .then(left.id.cmp(&right.id))
        });
        Ok(waves)
    }

    /// Raw append-only Mission Log rows across every Mission, in append
    /// order. Prefer [`Self::mission_log_entries`] when scoping to one
    /// Mission; this is here for parity with `legacy_waves()`/`missions()`.
    pub fn mission_log(&self) -> StoreResult<Vec<MissionLogEntry>> {
        self.read_jsonl("mission_log.jsonl")
    }

    /// Every [`MissionLogEntry`] for one Mission, ordered by `revision`
    /// ascending. There is no latest-wins collapse: unlike Legacy Wave/Mission the
    /// Log has no mutable identity, every row is a permanent entry.
    pub fn mission_log_entries(&self, mission_id: &str) -> StoreResult<Vec<MissionLogEntry>> {
        let mut entries = self
            .mission_log()?
            .into_iter()
            .filter(|entry| entry.mission_id == mission_id)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.revision);
        Ok(entries)
    }

    /// The last `n` [`MissionLogEntry`] rows for one Mission, oldest-first
    /// within the returned slice (Unix `tail` ordering) so a reader sees them
    /// in the order they were written. Returns fewer than `n` rows if the
    /// Mission has fewer entries, and an empty Vec if it has none yet.
    pub fn mission_log_tail(
        &self,
        mission_id: &str,
        n: usize,
    ) -> StoreResult<Vec<MissionLogEntry>> {
        let entries = self.mission_log_entries(mission_id)?;
        let start = entries.len().saturating_sub(n);
        Ok(entries[start..].to_vec())
    }

    pub fn members(&self) -> StoreResult<Vec<ProviderLaunchProfile>> {
        self.read_jsonl("provider_launch_profiles.jsonl")
    }

    pub fn members_for_ids(
        &self,
        member_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<ProviderLaunchProfile>> {
        Ok(self
            .read_jsonl::<ProviderLaunchProfile>("provider_launch_profiles.jsonl")?
            .into_iter()
            .filter(|row| member_ids.contains(&row.id))
            .collect())
    }

    /// Raw append-only compatibility admission rows in causal order.
    /// Invalid JSON or semantically invalid rows fail the entire read closed.
    pub fn provider_compatibility_admissions(
        &self,
    ) -> StoreResult<Vec<ProviderCompatibilityAdmission>> {
        let rows: Vec<ProviderCompatibilityAdmission> =
            self.read_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER)?;
        for row in &rows {
            row.validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
        }
        validate_provider_compatibility_admission_ledger(&rows)?;
        Ok(rows)
    }

    /// Latest-row-wins projection by the exact four-part compatibility key.
    pub fn latest_provider_compatibility_admissions(
        &self,
    ) -> StoreResult<Vec<ProviderCompatibilityAdmission>> {
        let mut latest = std::collections::BTreeMap::new();
        for row in self.provider_compatibility_admissions()? {
            latest.insert(
                (
                    row.project_id.clone(),
                    row.store_id.clone(),
                    row.provider.clone(),
                    row.execution_mode.clone(),
                    row.provider_version.clone(),
                    row.adapter_contract_version.clone(),
                ),
                row,
            );
        }
        Ok(latest.into_values().collect())
    }

    /// Return the active admission for one exact tuple. Terminal latest rows,
    /// other execution modes, and other contract versions never authorize it.
    pub fn effective_provider_compatibility_admission(
        &self,
        provider: &str,
        execution_mode: &str,
        provider_version: &str,
        adapter_contract_version: &str,
    ) -> StoreResult<Option<ProviderCompatibilityAdmission>> {
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        Ok(self
            .provider_compatibility_admissions()?
            .into_iter()
            .rev()
            .find(|row| {
                row.project_id == project_id
                    && row.store_id == store_id
                    && row.exact_key()
                        == (
                            provider,
                            execution_mode,
                            provider_version,
                            adapter_contract_version,
                        )
            })
            .filter(ProviderCompatibilityAdmission::is_active))
    }

    pub fn teams(&self) -> StoreResult<Vec<AgentTeam>> {
        self.all_agent_teams()
    }

    /// Latest canonical AgentTeam projection keyed by team id. A physical
    /// multi-space recovery store must not use this map for mutation routing.
    pub fn latest_teams(&self) -> StoreResult<std::collections::BTreeMap<String, AgentTeam>> {
        Ok(latest_by_id(self.teams()?, |team| team.id.clone()))
    }

    pub fn runtimes(&self) -> StoreResult<Vec<ProviderProcess>> {
        self.read_jsonl("provider_processes.jsonl")
    }

    pub fn proposals(&self) -> StoreResult<Vec<Proposal>> {
        self.read_jsonl("proposals.jsonl")
    }

    pub fn messages(&self) -> StoreResult<Vec<RegistryMessage>> {
        self.read_jsonl("messages.jsonl")
    }

    pub fn evidence(&self) -> StoreResult<Vec<Evidence>> {
        self.read_jsonl("evidence.jsonl")
    }

    pub fn decisions(&self) -> StoreResult<Vec<Decision>> {
        self.read_jsonl("decisions.jsonl")
    }

    pub fn reviews(&self) -> StoreResult<Vec<Review>> {
        self.read_jsonl("reviews.jsonl")
    }

    pub fn gaps(&self) -> StoreResult<Vec<Gap>> {
        self.read_jsonl("gaps.jsonl")
    }

    pub fn visions(&self) -> StoreResult<Vec<Vision>> {
        self.read_jsonl("visions.jsonl")
    }

    pub fn provider_child_threads(&self) -> StoreResult<Vec<ProviderChildThread>> {
        self.read_jsonl("provider_child_threads.jsonl")
    }

    /// Typed historical compatibility reader. Archive/export code must read
    /// the ledger as opaque bytes so unknown or malformed rows are preserved.
    pub fn workflow_runs(&self) -> StoreResult<Vec<WorkflowRun>> {
        self.read_jsonl("workflow_runs.jsonl")
    }

    /// Typed historical compatibility reader; never use it for archive truth.
    pub fn workflow_steps(&self) -> StoreResult<Vec<WorkflowStep>> {
        self.read_jsonl("workflow_steps.jsonl")
    }

    /// Typed historical compatibility reader; never use it for archive truth.
    pub fn workflow_patches(&self) -> StoreResult<Vec<WorkflowPatch>> {
        self.read_jsonl("workflow_patches.jsonl")
    }

    /// Typed historical compatibility reader; never use it for archive truth.
    pub fn workflow_artifact_manifests(&self) -> StoreResult<Vec<WorkflowArtifactManifest>> {
        self.read_jsonl("workflow_artifact_manifests.jsonl")
    }

    pub fn team_runs(&self) -> StoreResult<Vec<AgentTeamRun>> {
        self.read_jsonl("team_runs.jsonl")
    }

    /// Decode the shared ledger, then retain one TeamRun before callers apply
    /// latest-wins projection work. The JSONL layout has no per-run index, so
    /// raw deserialization remains store-wide.
    pub fn team_run_rows(&self, team_run_id: &str) -> StoreResult<Vec<AgentTeamRun>> {
        Ok(self
            .read_jsonl::<AgentTeamRun>("team_runs.jsonl")?
            .into_iter()
            .filter(|run| run.id == team_run_id)
            .collect())
    }

    pub fn latest_execution_nodes(&self) -> StoreResult<Vec<ExecutionNode>> {
        Ok(latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .into_values()
        .collect())
    }

    pub fn latest_execution_node(&self, node_id: &str) -> StoreResult<Option<ExecutionNode>> {
        Ok(latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?
                .into_iter()
                .filter(|node| node.id == node_id)
                .collect(),
            |node| node.id.clone(),
        )
        .remove(node_id))
    }

    pub fn execution_nodes(&self) -> StoreResult<Vec<ExecutionNode>> {
        self.read_jsonl("execution_nodes.jsonl")
    }

    pub fn latest_node_project_registrations(&self) -> StoreResult<Vec<NodeProjectRegistration>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        )
        .into_values()
        .collect())
    }

    pub fn latest_node_project_registrations_for_binding(
        &self,
        node_id: &str,
        project_binding_id: &str,
    ) -> StoreResult<Vec<NodeProjectRegistration>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?
                .into_iter()
                .filter(|row| {
                    row.node_id == node_id && row.project_binding_id == project_binding_id
                })
                .collect(),
            node_project_registration_identity,
        )
        .into_values()
        .collect())
    }

    /// Monotonic revision of one exact Node + Execution Space + Project
    /// registration identity. This is used to bind remote operational actions
    /// to the registry row they were projected from rather than merely to its
    /// latest value.
    pub fn node_project_registration_revision(
        &self,
        node_id: &str,
        execution_space_id: &str,
        project_binding_id: &str,
    ) -> StoreResult<u64> {
        Ok(self
            .read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?
            .into_iter()
            .filter(|registration| {
                registration.node_id == node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.project_binding_id == project_binding_id
            })
            .count() as u64)
    }

    pub fn latest_node_daemon_lease(&self, node_id: &str) -> StoreResult<Option<NodeDaemonLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id))
    }

    pub fn latest_node_daemon_leases(&self) -> StoreResult<Vec<NodeDaemonLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .into_values()
        .collect())
    }

    pub fn member_runs(&self) -> StoreResult<Vec<ProviderRuntimeProjection>> {
        let rows: Vec<ProviderRuntimeProjection> = self.read_jsonl("member_runs.jsonl")?;
        for row in &rows {
            row.validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
        }
        Ok(rows)
    }

    /// Validate only rows retained for this TeamRun. Raw JSONL decoding still
    /// visits the complete ledger; malformed rows outside the scope are not
    /// part of this scope-local projection validation.
    pub fn member_run_rows_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<ProviderRuntimeProjection>> {
        let rows = self
            .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
            .into_iter()
            .filter(|row| row.team_run_id == team_run_id)
            .collect::<Vec<_>>();
        for row in &rows {
            row.validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
        }
        Ok(rows)
    }

    /// Read the append-only pre-canonical message projection for explicit
    /// migration, export, and historical inspection only.
    ///
    /// Current runtime, delivery, reply lineage, and status projections must
    /// use the identity-first canonical Message fabric instead.
    pub fn legacy_team_messages(&self) -> StoreResult<Vec<TeamMessageProjection>> {
        self.read_jsonl("team_messages.jsonl")
    }

    pub fn work_operations(&self) -> StoreResult<Vec<WorkOperation>> {
        self.work_operations_unlocked()
    }

    pub fn latest_works(&self) -> StoreResult<Vec<Work>> {
        Ok(self.latest_works_unlocked()?.into_values().collect())
    }

    pub fn latest_works_for_team_run(&self, team_run_id: &str) -> StoreResult<Vec<Work>> {
        Ok(self
            .latest_works_and_ids_for_team_run_unlocked(team_run_id)?
            .0)
    }

    fn latest_works_and_ids_for_team_run_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<(Vec<Work>, std::collections::HashSet<String>)> {
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .filter(|work| work.team_run_id == team_run_id)
            .collect::<Vec<_>>();
        let work_ids = works.iter().map(|work| work.id.clone()).collect();
        Ok((works, work_ids))
    }

    pub fn latest_works_and_events_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<(Vec<Work>, Vec<WorkEvent>)> {
        // Work.team_run_id names the current execution attempt and may change
        // on retarget. Fold ownership once, then retain every operation and
        // trust event for those identities so history and provenance remain
        // complete without repeating the store-wide latest-Work fold.
        let (works, work_ids) = self.latest_works_and_ids_for_team_run_unlocked(team_run_id)?;
        let mut events = self
            .work_operations_for_ids_unlocked(&work_ids)?
            .into_iter()
            .map(|operation| operation.event)
            .collect::<Vec<_>>();
        events.extend(self.trust_work_events_for_ids_unlocked(&work_ids)?);
        Ok((works, events))
    }

    pub fn work_delegation_events(&self) -> StoreResult<Vec<WorkDelegationEvent>> {
        Ok(self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .map(|revision| revision.event)
            .collect())
    }

    pub fn latest_work_delegations(&self) -> StoreResult<Vec<WorkDelegation>> {
        Ok(self
            .latest_work_delegations_unlocked()?
            .into_values()
            .collect())
    }

    pub fn latest_work_delegations_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<WorkDelegation>> {
        Ok(self
            .latest_work_delegations_for_team_run_unlocked(team_run_id)?
            .into_values()
            .collect())
    }

    pub fn work_delegation_events_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<WorkDelegationEvent>> {
        Ok(self
            .work_delegation_revisions_for_team_run_unlocked(Some(team_run_id))?
            .into_iter()
            .map(|revision| revision.event)
            .collect())
    }

    /// Fold target Work state into Delegation state without changing the
    /// source Work. Repeated reconciliation is idempotent when no state change
    /// is required.
    pub fn transition_work_and_roll_up_delegation(
        &self,
        target_work_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Vec<WorkDelegation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let target = self
            .latest_works_unlocked()?
            .remove(target_work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {target_work_id}")))?;
        let revisions = self.work_delegation_rollup_revisions_unlocked(&target, &context)?;
        let mut changed = Vec::new();
        for revision in revisions {
            let current = self
                .latest_work_delegations_unlocked()?
                .remove(&revision.delegation.id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "delegation not found: {}",
                        revision.delegation.id
                    ))
                })?;
            changed.push(self.append_work_delegation_transition_unlocked(
                &current,
                revision.delegation,
                revision.event,
            )?);
        }
        Ok(changed)
    }

    pub fn cancel_work_delegation(
        &self,
        delegation_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<WorkDelegation> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "DELEGATION_CANCEL_REASON_REQUIRED".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = self
            .latest_work_delegations_unlocked()?
            .remove(delegation_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("delegation not found: {delegation_id}"))
            })?;
        self.require_work_delegation_actor_unlocked(
            &context.performed_by_actor,
            &current.source_work_ref.team_run_id,
            &current.source_owner_member_id,
            "cancel",
        )?;
        // Authorization precedes replay for the same reason as delegation
        // creation: an idempotency key is not an authority token.
        if let Some(existing) = self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .find(|revision| revision.event.idempotency_key == context.idempotency_key)
        {
            if existing.delegation.id == delegation_id
                && existing.event.transition == WorkDelegationTransition::Cancelled
                && existing.event.payload["reason"].as_str() == Some(reason)
            {
                return Ok(existing.delegation);
            }
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {} already belongs to Delegation event {}",
                context.idempotency_key, existing.event.id
            )));
        }
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_VERSION_CONFLICT: {} is version {}, expected {expected_version}",
                current.id, current.version
            )));
        }
        let mut next = current.clone();
        next.state = WorkDelegationState::Cancelled;
        next.version = next.version.saturating_add(1);
        next.updated_at = context.created_at.clone();
        next.blocker_reason = None;
        next.resolution_summary = Some(reason.to_string());
        let event = WorkDelegationEvent {
            id: context.event_id,
            delegation_id: current.id.clone(),
            sequence: next.version,
            transition: WorkDelegationTransition::Cancelled,
            expected_version: current.version,
            resulting_version: next.version,
            performed_by_actor: context.performed_by_actor,
            causation_ref: context.causation_ref,
            idempotency_key: context.idempotency_key,
            payload: serde_json::json!({"reason": reason}),
            created_at: context.created_at,
        };
        self.append_work_delegation_transition_unlocked(&current, next, event)
    }

    pub fn work_condition_records(&self) -> StoreResult<Vec<WorkConditionRecord>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.condition_records)
            .collect())
    }

    pub fn work_reports(&self) -> StoreResult<Vec<WorkReport>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.reports)
            .collect())
    }

    pub fn work_evidence(&self) -> StoreResult<Vec<WorkEvidence>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.evidence_records)
            .collect())
    }

    pub fn work_operational_decisions(&self) -> StoreResult<Vec<WorkOperationalDecision>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.decisions)
            .collect())
    }

    pub fn work_events(&self) -> StoreResult<Vec<WorkEvent>> {
        let mut events = self
            .work_operations_unlocked()?
            .into_iter()
            .map(|operation| operation.event)
            .collect::<Vec<_>>();
        events.extend(self.trust_work_events_unlocked()?);
        Ok(events)
    }

    pub fn work_events_for_team_run(&self, team_run_id: &str) -> StoreResult<Vec<WorkEvent>> {
        Ok(self.latest_works_and_events_for_team_run(team_run_id)?.1)
    }

    pub fn team_supervisor_leases(&self) -> StoreResult<Vec<TeamSupervisorLease>> {
        self.read_jsonl("team_supervisor_leases.jsonl")
    }

    pub fn team_supervisor_lease_rows(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<TeamSupervisorLease>> {
        Ok(self
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?
            .into_iter()
            .filter(|row| row.team_run_id == team_run_id)
            .collect())
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

    pub fn team_member_close_request_rows(
        &self,
        member_run_ids: &std::collections::HashSet<String>,
    ) -> StoreResult<Vec<TeamMemberCloseRequest>> {
        Ok(self
            .read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?
            .into_iter()
            .filter(|row| member_run_ids.contains(&row.member_run_id))
            .collect())
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

    pub fn member_action_rows_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<MemberAction>> {
        Ok(self
            .read_jsonl::<MemberAction>("member_actions.jsonl")?
            .into_iter()
            .filter(|row| row.team_run_id == team_run_id)
            .collect())
    }

    pub fn delegation_runs(&self) -> StoreResult<Vec<DelegationRun>> {
        self.read_jsonl("delegation_runs.jsonl")
    }

    pub fn delegation_run_rows_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Vec<DelegationRun>> {
        Ok(self
            .read_jsonl::<DelegationRun>("delegation_runs.jsonl")?
            .into_iter()
            .filter(|row| row.team_run_id == team_run_id)
            .collect())
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
