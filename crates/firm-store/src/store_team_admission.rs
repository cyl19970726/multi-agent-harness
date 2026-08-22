use super::*;

impl HarnessStore {
    pub(super) fn validate_new_team_run_from_agent_team_unlocked(
        &self,
        value: &AgentTeamRun,
        execution_space_id: &str,
    ) -> StoreResult<()> {
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        if runs.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "team run already exists: {}",
                value.id
            )));
        }
        let team = latest_by_id(self.all_agent_teams()?, |team| team.id.clone())
            .remove(&value.agent_team_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} not found",
                    value.agent_team_id
                ))
            })?;
        if team.status != firm_core::AgentTeamStatus::Active {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} is {:?}",
                team.id, team.status
            )));
        }
        if value.execution_node_id != team.node_id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_NODE_MISMATCH: TeamRun {} names {}, Team {} is placed on {}",
                value.id, value.execution_node_id, team.id, team.node_id
            )));
        }
        let active_hosts = self
            .fabric_team_memberships(execution_space_id)?
            .into_iter()
            .filter(|membership| {
                membership.team_id == team.id
                    && membership.role == firm_core::agentfirm_api::TeamMembershipRole::Host
                    && membership.state == firm_core::agentfirm_api::TeamMembershipStatus::Active
            })
            .collect::<Vec<_>>();
        if active_hosts.len() != 1 {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_HOST_MEMBERSHIP: AgentTeam {} has {} active Host memberships",
                team.id,
                active_hosts.len()
            )));
        }
        let host_is_active = self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .any(|member| {
                member.id == active_hosts[0].agent_member_id
                    && member.organization_status
                        == firm_core::agentfirm_api::AgentMemberOrganizationStatus::Active
            });
        if !host_is_active {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_HOST_MEMBERSHIP: AgentTeam {} Host AgentMember is not Active",
                team.id
            )));
        }
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&team.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("NODE_NOT_ACTIVE: {} not found", team.node_id))
        })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {} is {:?}",
                node.id, node.status
            )));
        }
        let registrations = latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        );
        let matching_registrations = registrations
            .values()
            .filter(|registration| {
                registration.node_id == team.node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.project_binding_id == value.project_binding_id
                    && registration.status == NodeProjectRegistrationStatus::Active
            })
            .count();
        if matching_registrations != 1 {
            return Err(StoreError::Conflict(format!(
                "PROJECT_NOT_REGISTERED_ON_NODE: expected one active registration for {} on Node {}, found {matching_registrations}",
                value.project_binding_id, team.node_id
            )));
        }
        if let Some(previous_id) = value.previous_run_id.as_deref() {
            let previous = runs.get(previous_id).ok_or_else(|| {
                StoreError::Conflict(format!("previous team run not found: {previous_id}"))
            })?;
            if previous.agent_team_id != value.agent_team_id {
                return Err(StoreError::Conflict(format!(
                    "previous run {previous_id} belongs to AgentTeam {}",
                    previous.agent_team_id
                )));
            }
        }
        Ok(())
    }

    /// Reconstruct one raw historical ProviderRuntimeProjection during an
    /// explicit Legacy import. It is intentionally not a current admission
    /// path and never materializes the canonical MemberRun.
    #[cfg(test)]
    pub(crate) fn legacy_import_append_member_run_projection(
        &self,
        value: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if value.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: initial ProviderRuntimeProjection append cannot set a typed compatibility cause"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?;
        let current = latest_by_id(rows, |row| row.id.clone()).remove(&value.id);
        if let Some(current) = current {
            if current == *value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "MEMBER_REVISION_REQUIRES_CAS: ProviderRuntimeProjection {} already exists; use compare_and_append_member_run",
                value.id
            )));
        } else {
            // Initial team creation predeclares every runtime id in the first
            // TeamRun row. Materializing one of those rows cannot extend or
            // rewrite membership, and raw later TeamRun revisions are barred
            // from changing the list.
            let first_run = self
                .read_jsonl::<AgentTeamRun>("team_runs.jsonl")?
                .into_iter()
                .find(|run| run.id == value.team_run_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("team run not found: {}", value.team_run_id))
                })?;
            if !first_run.member_run_ids.iter().any(|id| id == &value.id) {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_ADMISSION_REQUIRED: ProviderRuntimeProjection {} was not declared by initial TeamRun {}",
                    value.id, value.team_run_id
                )));
            }
            let latest_run = self.require_team_run_unlocked(&value.team_run_id)?;
            self.ensure_unique_member_identity_unlocked(&latest_run, value)?;
        }
        self.append_jsonl_unlocked("member_runs.jsonl", value)
    }

    /// Compare-and-append one existing ProviderRuntimeProjection revision. Raw append cannot
    /// mutate lifecycle authority; all legitimate close/reopen/runtime updates
    /// must prove the exact revision they observed.
    pub fn compare_and_append_member_run(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&current)?;
        if next.runtime_generation != current.runtime_generation {
            return Err(StoreError::Conflict(format!(
                "MEMBER_GENERATION_TRANSITION_AUTHORITY_REQUIRED: ProviderRuntimeProjection {} generation changes must use compare_and_advance_member_run_generation",
                current.id
            )));
        }
        ensure_member_provenance_unchanged(&current, next)?;
        ensure_member_lifecycle_revision(&current, next)?;
        ensure_provider_compatibility_cause_unchanged(&current, next)?;
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let execution_space_id = self.require_current_member_mutation_scope_unlocked(&current)?;
        let canonical =
            self.prepare_current_member_runtime_sync_unlocked(&execution_space_id, next)?;
        serde_json::to_value(next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", next)?;
        if let Some(canonical) = canonical {
            self.commit_prepared_current_member_sync_unlocked(canonical)?;
        }
        Ok(())
    }

    /// Atomically enter a compatibility-owned Blocked state. This is the only
    /// Store API allowed to introduce a typed compatibility cause.
    pub fn block_member_run_for_provider_compatibility(
        &self,
        expected: &ProviderRuntimeProjection,
        profile: &ProviderIntegrationProfile,
        cause: ProviderCompatibilityBlockCause,
        last_event_at: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        cause
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&current)?;
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if !current.coordination_is_active()
            || current.finished_at.is_some()
            || !matches!(
                current.status,
                firm_core::MemberRunStatus::Idle
                    | firm_core::MemberRunStatus::Queued
                    | firm_core::MemberRunStatus::Disconnected
            )
        {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_LIFECYCLE_INVALID: ProviderRuntimeProjection {} must have active coordination, unfinished runtime, and idle, queued, or disconnected status",
                current.id
            )));
        }
        if current.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_ALREADY_OWNED: ProviderRuntimeProjection {} already has a typed cause",
                current.id
            )));
        }
        ensure_compatibility_cause_matches_profile(&current, profile, &cause)?;
        if cause.compatibility_status != profile.compatibility_status {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_STATUS_MISMATCH: typed cause status does not match the observed provider profile"
                    .to_string(),
            ));
        }
        require_non_empty_store(last_event_at, "compatibility block last_event_at")?;
        let mut next = current.clone();
        next.provider_profile = Some(profile.clone());
        next.status = firm_core::MemberRunStatus::Blocked;
        next.provider_compatibility_block_cause = Some(cause);
        next.last_event_at = Some(last_event_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let execution_space_id = self.require_current_member_mutation_scope_unlocked(&current)?;
        let canonical =
            self.prepare_current_member_runtime_sync_unlocked(&execution_space_id, &next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        if let Some(canonical) = canonical {
            self.commit_prepared_current_member_sync_unlocked(canonical)?;
        }
        Ok(next)
    }

    /// Atomically clear a compatibility-owned block after the current exact
    /// tuple is either source-reviewed or covered by an active admission.
    pub fn recover_member_run_from_provider_compatibility_block(
        &self,
        expected: &ProviderRuntimeProjection,
        profile: &ProviderIntegrationProfile,
        boundary: ProviderCompatibilityBlockBoundary,
        recovery_status: firm_core::MemberRunStatus,
        last_event_at: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        self.require_current_member_mutation_scope_unlocked(&current)?;
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if !current.coordination_is_active() || current.finished_at.is_some() {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_LIFECYCLE_INVALID: ProviderRuntimeProjection {} must have active coordination and unfinished runtime",
                current.id
            )));
        }
        let cause = current
            .provider_compatibility_block_cause
            .as_ref()
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "PROVIDER_COMPATIBILITY_BLOCK_CAUSE_REQUIRED: ProviderRuntimeProjection {} has no typed compatibility cause",
                    current.id
                ))
            })?;
        if current.status != firm_core::MemberRunStatus::Blocked {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_STATE_MISMATCH: ProviderRuntimeProjection {} is not Blocked",
                current.id
            )));
        }
        let blocked_profile = current.provider_profile.as_ref().ok_or_else(|| {
            StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_PROFILE_REQUIRED: ProviderRuntimeProjection {} has no durable blocked provider profile",
                current.id
            ))
        })?;
        ensure_compatibility_cause_matches_profile(&current, blocked_profile, cause)?;
        if cause.boundary != boundary {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_BOUNDARY_MISMATCH: typed cause boundary {:?} does not match current {:?} boundary",
                cause.boundary, boundary
            )));
        }
        if !matches!(
            recovery_status,
            firm_core::MemberRunStatus::Disconnected
                | firm_core::MemberRunStatus::Queued
                | firm_core::MemberRunStatus::Idle
        ) {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_RECOVERY_STATUS_INVALID: recovery target must be disconnected, queued, or idle"
                    .to_string(),
            ));
        }
        let authorized = if profile.compatibility_status == ProviderCompatibilityStatus::Current {
            profile.provider_version.as_ref().is_some_and(|version| {
                profile
                    .reviewed_provider_versions
                    .iter()
                    .any(|reviewed| reviewed == version)
            })
        } else if profile.compatibility_status == ProviderCompatibilityStatus::ReviewRequired {
            let (project_id, store_id) = self.provider_compatibility_scope().ok_or_else(|| {
                StoreError::Conflict(
                    "PROVIDER_COMPATIBILITY_SCOPE_REQUIRED: recovery requires an exact project/store scope"
                        .to_string(),
                )
            })?;
            let rows: Vec<ProviderCompatibilityAdmission> =
                self.read_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER)?;
            for row in &rows {
                row.validate()
                    .map_err(|error| StoreError::Conflict(error.to_string()))?;
            }
            validate_provider_compatibility_admission_ledger(&rows)?;
            rows.into_iter()
                .rev()
                .find(|row| {
                    row.project_id == project_id
                        && row.store_id == store_id
                        && row.exact_key()
                            == (
                                profile.provider.as_str(),
                                profile.execution_mode.as_str(),
                                profile.provider_version.as_deref().unwrap_or(""),
                                profile.adapter_contract_version.as_deref().unwrap_or(""),
                            )
                })
                .is_some_and(|row| row.is_active())
        } else {
            false
        };
        if !authorized {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_NOT_AUTHORIZED: exact tuple for ProviderRuntimeProjection {} is not source-reviewed or actively admitted",
                current.id
            )));
        }
        require_non_empty_store(last_event_at, "compatibility recovery last_event_at")?;
        let mut next = current.clone();
        next.provider_profile = Some(profile.clone());
        next.status = recovery_status;
        next.provider_compatibility_block_cause = None;
        next.finished_at = None;
        next.last_event_at = Some(last_event_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let execution_space_id = self.require_current_member_mutation_scope_unlocked(&current)?;
        let canonical =
            self.prepare_current_member_runtime_sync_unlocked(&execution_space_id, &next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        if let Some(canonical) = canonical {
            self.commit_prepared_current_member_sync_unlocked(canonical)?;
        }
        Ok(next)
    }

    /// Resolve the exact Execution Space for one *current* TeamRun and reject
    /// any partially materialized or cross-space member set. Legacy JSONL
    /// rows remain readable for diagnostics, but they cannot authorize current
    /// mutations or controls unless every declared member has one matching
    /// canonical MemberRun in the same Execution Space.
    pub(super) fn current_team_run_execution_space_unlocked(
        &self,
        run: &AgentTeamRun,
    ) -> StoreResult<String> {
        if run.member_run_ids.is_empty() {
            let registrations = latest_by_id(
                self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
                node_project_registration_identity,
            )
            .values()
            .filter(|registration| {
                registration.node_id == run.execution_node_id
                    && registration.project_binding_id == run.project_binding_id
                    && registration.status == NodeProjectRegistrationStatus::Active
            })
            .map(|registration| registration.execution_space_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
            return match registrations.len() {
                1 => Ok(registrations
                    .into_iter()
                    .next()
                    .expect("one active registration")),
                count => Err(StoreError::Conflict(format!(
                    "EXECUTION_SPACE_SCOPE_MISMATCH: empty TeamRun {} resolves to {count} active Execution Spaces",
                    run.id
                ))),
            };
        }

        let legacy_by_id = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        );
        let declared_ids = run
            .member_run_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut canonical_by_id = std::collections::BTreeMap::<
            String,
            Vec<(String, firm_core::agentfirm_api::MemberRun)>,
        >::new();
        for scope in self.canonical_execution_space_ids()? {
            for member in self.trust_member_runs(&scope)? {
                if declared_ids.contains(&member.id) {
                    canonical_by_id
                        .entry(member.id.clone())
                        .or_default()
                        .push((scope.clone(), member));
                }
            }
        }

        let mut resolved_scope = None::<String>;
        for member_run_id in &run.member_run_ids {
            let legacy = legacy_by_id.get(member_run_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but has no latest ProviderRuntimeProjection",
                    run.id, member_run_id
                ))
            })?;
            if legacy.team_run_id != run.id {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} declares MemberRun {} whose ProviderRuntimeProjection belongs to TeamRun {}",
                    run.id, member_run_id, legacy.team_run_id
                )));
            }
            let rows = canonical_by_id
                .get(member_run_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let (scope, canonical) = match rows {
                [row] => row,
                [] => {
                    return Err(StoreError::Conflict(format!(
                        "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} declares MemberRun {} but no canonical MemberRun exists",
                        run.id, member_run_id
                    )))
                }
                rows => {
                    return Err(StoreError::Conflict(format!(
                        "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} declares MemberRun {} with {} canonical Execution Space projections",
                        run.id,
                        member_run_id,
                        rows.len()
                    )))
                }
            };
            let mismatch_fields =
                current_member_lifecycle_validation_mismatch_fields(canonical, legacy)?;
            if !mismatch_fields.is_empty() {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_RUN_MATERIALIZATION_MISMATCH: TeamRun {} MemberRun {} legacy/canonical projection differs in Execution Space {} for fields {}; legacy_status={:?} canonical_status={:?} legacy_last_event_at={:?} canonical_last_event_at={:?}",
                    run.id,
                    member_run_id,
                    scope,
                    mismatch_fields.join(","),
                    legacy.status,
                    canonical.runtime_status,
                    legacy.last_event_at,
                    canonical.last_event_at
                )));
            }
            if let Some(expected) = resolved_scope.as_deref() {
                if expected != scope {
                    return Err(StoreError::Conflict(format!(
                        "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {} MemberRun {} belongs to Execution Space {}, not {}",
                        run.id, member_run_id, scope, expected
                    )));
                }
            } else {
                resolved_scope = Some(scope.clone());
            }
        }
        resolved_scope.ok_or_else(|| {
            StoreError::Conflict(format!(
                "MEMBER_RUN_MATERIALIZATION_INCOMPLETE: TeamRun {} has no resolvable current MemberRuns",
                run.id
            ))
        })
    }

    /// Public current-path resolver. The consistent fast path stays read-only.
    /// If it observes an incomplete/mismatched dual projection, it retries
    /// under the cross-process Store lock: this distinguishes a bounded
    /// legacy-append -> canonical-replace writer window from durable corrupt
    /// state without making healthy status reads contend with writers.
    pub fn current_team_run_execution_space(&self, run: &AgentTeamRun) -> StoreResult<String> {
        self.init()?;
        for _ in 0..20 {
            let current = self.require_team_run_unlocked(&run.id)?;
            if current != *run {
                return Err(StoreError::Conflict(format!(
                    "TEAM_RUN_CHANGED: TeamRun {} changed concurrently; retry scope resolution",
                    run.id
                )));
            }
            match self.current_team_run_execution_space_unlocked(&current) {
                Ok(scope) => return Ok(scope),
                Err(StoreError::Conflict(message))
                    if message.starts_with("MEMBER_RUN_MATERIALIZATION_INCOMPLETE:")
                        || message.starts_with("MEMBER_RUN_MATERIALIZATION_MISMATCH:") =>
                {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
        // A current read that observed the narrow Legacy-append -> canonical-
        // replace window waits for that writer independently of the mutation
        // timeout override. Tests and operators may intentionally reduce the
        // mutation timeout to surface a retryable 503; that must not make an
        // otherwise healthy concurrent status read report durable corruption.
        // Coherent reads still take the lock-free fast path above.
        let _lock =
            self.acquire_write_lock_with_policy(Duration::from_secs(2), Duration::from_millis(1))?;
        let locked_current = self.require_team_run_unlocked(&run.id)?;
        if locked_current != *run {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_CHANGED: TeamRun {} changed concurrently; retry scope resolution",
                run.id
            )));
        }
        self.current_team_run_execution_space_unlocked(&locked_current)
    }

    /// Fence a current MemberRun mutation against the complete owning
    /// TeamRun while the caller holds the Store write lock.
    pub(super) fn require_current_member_mutation_scope_unlocked(
        &self,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<String> {
        let run = self.require_team_run_unlocked(&member.team_run_id)?;
        if !run.member_run_ids.iter().any(|id| id == &member.id) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_RUN_SCOPE_MISMATCH: ProviderRuntimeProjection {} is not declared by TeamRun {}",
                member.id, run.id
            )));
        }
        self.current_team_run_execution_space_unlocked(&run)
    }

    pub(super) fn validate_member_run_admission_rows_unlocked(
        &self,
        team_run: &AgentTeamRun,
        runtimes: &[ProviderRuntimeProjection],
        canonical: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<()> {
        if runtimes.len() != canonical.len() || runtimes.len() != team_run.member_run_ids.len() {
            return Err(StoreError::Conflict(
                "MEMBER_ADMISSION_SET_MISMATCH: TeamRun, runtime, and canonical member counts differ"
                    .to_string(),
            ));
        }
        let declared_ids = team_run
            .member_run_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let existing_ids = self
            .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
            .into_iter()
            .map(|row| row.id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut runtime_ids = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        let canonical_by_id = canonical
            .iter()
            .map(|admission| (admission.run.id.as_str(), admission))
            .collect::<std::collections::BTreeMap<_, _>>();
        for runtime in runtimes {
            runtime
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            if runtime.provider_compatibility_block_cause.is_some() {
                return Err(StoreError::Conflict(
                    "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: member admission cannot set a typed compatibility cause"
                        .to_string(),
                ));
            }
            if runtime.team_run_id != team_run.id || !declared_ids.contains(&runtime.id) {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_ADMISSION_SET_MISMATCH: ProviderRuntimeProjection {} is not declared by TeamRun {}",
                    runtime.id, team_run.id
                )));
            }
            if existing_ids.contains(&runtime.id) || !runtime_ids.insert(runtime.id.clone()) {
                return Err(StoreError::Conflict(format!(
                    "member run already exists or is duplicated: {}",
                    runtime.id
                )));
            }
            if !identities.insert(member_identity(runtime)) {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_IDENTITY_CONFLICT: TeamRun {} proposes duplicate stable member identity",
                    team_run.id
                )));
            }
            let canonical = canonical_by_id.get(runtime.id.as_str()).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "MEMBER_ADMISSION_SET_MISMATCH: missing canonical MemberRun {}",
                    runtime.id
                ))
            })?;
            if canonical.run.team_run_id != runtime.team_run_id
                || canonical.run.agent_member_id != runtime.agent_member_id
                || canonical.run.role_snapshot != runtime.role
                || canonical.run.runtime_generation != runtime.runtime_generation
            {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_ADMISSION_SET_MISMATCH: canonical MemberRun {} does not match its runtime projection",
                    runtime.id
                )));
            }
        }
        if runtime_ids != declared_ids {
            return Err(StoreError::Conflict(
                "MEMBER_ADMISSION_SET_MISMATCH: TeamRun member ids do not match runtime projections"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Create a TeamRun and all of its initial current MemberRuns through one
    /// same-lock semantic admission boundary. Every legacy and canonical row is
    /// validated before the first durable append; the canonical rows are then
    /// published as one atomic trust-ledger replacement.
    pub fn create_team_run_with_member_runs_from_agent_team(
        &self,
        value: &AgentTeamRun,
        execution_space_id: &str,
        runtimes: &[ProviderRuntimeProjection],
        canonical: &[CanonicalMemberRunAdmission],
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        require_non_empty_store(execution_space_id, "Execution Space id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.validate_new_team_run_from_agent_team_unlocked(value, execution_space_id)?;
        self.validate_member_run_admission_rows_unlocked(value, runtimes, canonical)?;
        let team = latest_by_id(self.all_agent_teams()?, |team| team.id.clone())
            .remove(&value.agent_team_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} not found",
                    value.agent_team_id
                ))
            })?;
        let host_runtimes = runtimes
            .iter()
            .filter(|runtime| runtime.agent_member_id == team.host_agent_id)
            .collect::<Vec<_>>();
        let [host_runtime] = host_runtimes.as_slice() else {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_HOST_MEMBER_RUN: TeamRun {} has {} Host MemberRuns",
                value.id,
                host_runtimes.len()
            )));
        };
        if value
            .host_actor
            .as_ref()
            .is_none_or(|actor| actor.kind != TeamActorKind::Host || actor.id != team.host_agent_id)
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_MISMATCH: TeamRun {} must bind Host actor to AgentMember {}",
                value.id, team.host_agent_id
            )));
        }
        match value.host_control_mode {
            firm_core::HostControlMode::Managed if host_runtime.is_external_interactive() => {
                return Err(StoreError::Conflict(
                    "MANAGED_HOST_REQUIRES_TEAM_RUNTIME: Host MemberRun is external_interactive"
                        .to_string(),
                ));
            }
            firm_core::HostControlMode::ExternalInteractive
                if !host_runtime.is_external_interactive() =>
            {
                return Err(StoreError::Conflict(
                    "EXTERNAL_HOST_REQUIRES_USER_DRIVEN_MEMBER_RUN: Host MemberRun is managed"
                        .to_string(),
                ));
            }
            _ => {}
        }
        self.validate_new_trust_member_runs_unlocked(execution_space_id, value, canonical)?;
        self.append_jsonl_unlocked("team_runs.jsonl", value)?;
        for runtime in runtimes {
            self.append_jsonl_unlocked("member_runs.jsonl", runtime)?;
        }
        self.commit_new_trust_member_runs_unlocked(canonical)?;
        Ok(())
    }

    /// Admit one new current MemberRun through a same-lock semantic boundary.
    /// Legacy TeamRun/runtime CAS and exact-space canonical authority are both
    /// validated before the first durable append.
    pub fn admit_member_run_with_canonical(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
        member: &ProviderRuntimeProjection,
        execution_space_id: &str,
        canonical: &CanonicalMemberRunAdmission,
    ) -> StoreResult<()> {
        member
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if member.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: member admission cannot set a typed compatibility cause"
                    .to_string(),
            ));
        }
        require_non_empty_store(execution_space_id, "Execution Space id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently; retry member admission",
                expected.id
            )));
        }
        ensure_team_run_admission_revision(&current, next, member)?;
        if self
            .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
            .iter()
            .any(|row| row.id == member.id)
        {
            return Err(StoreError::Conflict(format!(
                "member run already exists: {}",
                member.id
            )));
        }
        self.ensure_member_admission_identity_unlocked(&current, member)?;
        let current_execution_space_id =
            self.current_team_run_execution_space_unlocked(&current)?;
        if current_execution_space_id != execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {} belongs to Execution Space {}, not {}",
                current.id, current_execution_space_id, execution_space_id
            )));
        }
        if canonical.run.id != member.id
            || canonical.run.team_run_id != member.team_run_id
            || canonical.run.agent_member_id != member.agent_member_id
            || canonical.run.role_snapshot != member.role
            || canonical.run.runtime_generation != member.runtime_generation
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_ADMISSION_SET_MISMATCH: canonical MemberRun {} does not match its runtime projection",
                canonical.run.id
            )));
        }
        self.validate_new_trust_member_runs_unlocked(
            execution_space_id,
            next,
            std::slice::from_ref(canonical),
        )?;
        self.append_jsonl_unlocked("team_runs.jsonl", next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", member)?;
        self.commit_new_trust_member_runs_unlocked(std::slice::from_ref(canonical))?;
        Ok(())
    }
}
