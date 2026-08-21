use super::*;

impl HarnessStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            process_write_lock: process_write_lock_for(&root),
            root,
            provider_compatibility_scope: None,
        }
    }

    /// Bind compatibility admissions to the Project Binding and execution
    /// store selected by the caller. The scope is deliberately explicit and
    /// is never inferred from a path hash: moving/migrating a store must not
    /// silently transfer operational authority.
    pub fn with_provider_compatibility_scope(
        mut self,
        project_id: impl Into<String>,
        store_id: impl Into<String>,
    ) -> Self {
        self.provider_compatibility_scope = Some((project_id.into(), store_id.into()));
        self
    }

    pub fn provider_compatibility_scope(&self) -> Option<(&str, &str)> {
        self.provider_compatibility_scope
            .as_ref()
            .map(|(project_id, store_id)| (project_id.as_str(), store_id.as_str()))
    }

    pub(super) fn require_provider_compatibility_scope(&self) -> StoreResult<(&str, &str)> {
        self.provider_compatibility_scope().ok_or_else(|| {
            StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_SCOPE_REQUIRED: provider compatibility authority requires an explicitly configured project/store scope"
                    .to_string(),
            )
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> StoreResult<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("prompts"))?;
        fs::create_dir_all(self.root.join("runtimes"))?;
        Ok(())
    }

    /// Hold the store's ordinary writer lock for the complete lifetime of a
    /// migration snapshot.
    ///
    /// The guard intentionally exposes no store operations. Callers must not
    /// invoke a write method on this same `HarnessStore` while it is alive:
    /// those methods acquire `.store.lock` themselves and would be a
    /// re-entrant lock attempt. Direct, read-only filesystem snapshots are the
    /// intended use while the guard is held.
    pub fn acquire_exclusive_migration_guard(&self) -> StoreResult<StoreExclusiveMigrationGuard> {
        Ok(StoreExclusiveMigrationGuard {
            _write_lock: self.acquire_write_lock()?,
        })
    }

    /// Test-seed helper for historical Mission rows. Mission write authority
    /// is retired (DOC-108); production has no Mission author or restore path,
    /// and every caller lives in test code. Kept un-gated only because
    /// integration tests in other crates link the non-test build.
    #[doc(hidden)]
    pub fn append_mission(&self, value: &Mission) -> StoreResult<()> {
        self.append_jsonl("missions.jsonl", value)
    }

    pub fn append_member(&self, value: &ProviderLaunchProfile) -> StoreResult<()> {
        self.append_jsonl("provider_launch_profiles.jsonl", value)
    }

    /// Retired pre-vNext direct Team writer. Durable Team and Membership
    /// authority must be committed through the canonical trust kernel.
    pub fn append_team(&self, value: &AgentTeam) -> StoreResult<()> {
        Err(StoreError::Conflict(format!(
            "RETIRED_TEAM_WRITER: AgentTeam {} must use create_agent_team/transition_agent_team",
            value.id
        )))
    }

    /// Retired Mission-owned Team writer retained only as a fail-closed source
    /// compatibility hook. It never mutates `teams.jsonl`.
    pub fn insert_agent_team_with_unique_mission(&self, value: &AgentTeam) -> StoreResult<()> {
        Err(StoreError::Conflict(format!(
            "RETIRED_MISSION_TEAM_WRITER: AgentTeam {} is independent of Mission; use create_agent_team",
            value.id
        )))
    }

    /// Append a new active operational admission for one exact provider tuple.
    ///
    /// Admission ids are stable command ids: replaying an identical record is
    /// idempotent, while reusing an id for different content is a conflict.
    /// Only one row for an exact tuple may be active at a time.
    pub fn append_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.admit_provider_compatibility_admission(value)
    }

    pub fn admit_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Active {
            return Err(StoreError::Conflict(
                "provider compatibility admission must have active lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    /// Atomically create or reuse the active admission represented by a
    /// command request.
    ///
    /// Generated ids and timestamps are deliberately excluded from replay
    /// identity. Evidence references are a set: ordering and duplicates do
    /// not change command semantics, and newly appended rows store them in
    /// sorted, deduplicated order. Any other difference remains a conflict.
    pub fn ensure_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<EnsureProviderCompatibilityAdmissionResult> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Active {
            return Err(StoreError::Conflict(
                "provider compatibility admission must have active lifecycle".to_string(),
            ));
        }
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        if value.project_id != project_id || value.store_id != store_id {
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission scope mismatch: current project/store is {project_id}/{store_id}, record is {}/{}",
                value.project_id, value.store_id
            )));
        }
        let mut candidate = value.clone();
        candidate.evidence_refs =
            canonical_provider_admission_evidence_refs(&candidate.evidence_refs);
        candidate
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.provider_compatibility_admissions()?;

        if let Some(existing) = rows.iter().find(|row| row.id == candidate.id) {
            if existing == &candidate {
                return Ok(EnsureProviderCompatibilityAdmissionResult {
                    admission: existing.clone(),
                    created: false,
                });
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission id {} already has different content",
                candidate.id
            )));
        }

        let current = rows.iter().rev().find(|row| {
            row.project_id == candidate.project_id
                && row.store_id == candidate.store_id
                && row.exact_key() == candidate.exact_key()
        });
        if let Some(active) = current.filter(|row| row.is_active()) {
            if provider_admission_replay_matches(active, &candidate) {
                return Ok(EnsureProviderCompatibilityAdmissionResult {
                    admission: active.clone(),
                    created: false,
                });
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility tuple already has semantically different active admission {}",
                active.id
            )));
        }

        self.append_jsonl_unlocked(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, &candidate)?;
        Ok(EnsureProviderCompatibilityAdmissionResult {
            admission: candidate,
            created: true,
        })
    }

    /// Compatibility alias for callers that name the operation, rather than
    /// the ledger record.
    pub fn admit_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.admit_provider_compatibility_admission(value)
    }

    pub fn revoke_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Revoked {
            return Err(StoreError::Conflict(
                "provider compatibility revocation must have revoked lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    pub fn revoke_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.revoke_provider_compatibility_admission(value)
    }

    pub fn supersede_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Superseded {
            return Err(StoreError::Conflict(
                "provider compatibility supersession must have superseded lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    pub fn supersede_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.supersede_provider_compatibility_admission(value)
    }

    pub(super) fn append_provider_compatibility_admission_checked(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        if value.project_id != project_id || value.store_id != store_id {
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission scope mismatch: current project/store is {project_id}/{store_id}, record is {}/{}",
                value.project_id, value.store_id
            )));
        }
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.provider_compatibility_admissions()?;

        if let Some(existing) = rows.iter().find(|row| row.id == value.id) {
            if existing == value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission id {} already has different content",
                value.id
            )));
        }

        let current = rows.iter().rev().find(|row| {
            row.project_id == value.project_id
                && row.store_id == value.store_id
                && row.exact_key() == value.exact_key()
        });
        match value.lifecycle {
            ProviderCompatibilityAdmissionLifecycle::Active => {
                if let Some(active) = current.filter(|row| row.is_active()) {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility tuple already has active admission {}",
                        active.id
                    )));
                }
            }
            ProviderCompatibilityAdmissionLifecycle::Revoked
            | ProviderCompatibilityAdmissionLifecycle::Superseded => {
                let predecessor_id = value
                    .predecessor_admission_id
                    .as_deref()
                    .expect("validated terminal admission has predecessor");
                let predecessor = current.filter(|row| row.is_active()).ok_or_else(|| {
                    StoreError::Conflict(
                        "provider compatibility transition has no current active predecessor"
                            .to_string(),
                    )
                })?;
                if predecessor.id != predecessor_id {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility predecessor is stale: expected {}, got {}",
                        predecessor.id, predecessor_id
                    )));
                }
                if predecessor.project_id != value.project_id
                    || predecessor.store_id != value.store_id
                    || predecessor.policy != value.policy
                {
                    return Err(StoreError::Conflict(
                        "provider compatibility transition must preserve predecessor scope and policy"
                            .to_string(),
                    ));
                }
            }
        }
        self.append_jsonl_unlocked(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, value)
    }

    pub fn append_runtime(&self, value: &ProviderProcess) -> StoreResult<()> {
        self.append_jsonl("provider_processes.jsonl", value)
    }

    pub fn append_proposal(&self, value: &Proposal) -> StoreResult<()> {
        self.append_jsonl("proposals.jsonl", value)
    }

    pub fn append_message(&self, value: &RegistryMessage) -> StoreResult<()> {
        self.append_jsonl("messages.jsonl", value)
    }

    pub fn append_evidence(&self, value: &Evidence) -> StoreResult<()> {
        self.append_jsonl("evidence.jsonl", value)
    }

    pub fn append_decision(&self, value: &Decision) -> StoreResult<()> {
        self.append_jsonl("decisions.jsonl", value)
    }

    pub fn append_review(&self, value: &Review) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if self
            .read_jsonl::<Review>("reviews.jsonl")?
            .iter()
            .any(|review| review.id == value.id)
        {
            return Err(StoreError::Conflict(format!(
                "review already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("reviews.jsonl", value)
    }

    /// Record a Review bound to the exact current Work candidate. Identity and
    /// binding fields are derived from trusted Store context rather than caller
    /// supplied payload.
    pub fn append_gap(&self, value: &Gap) -> StoreResult<()> {
        self.append_jsonl("gaps.jsonl", value)
    }

    pub fn append_vision(&self, value: &Vision) -> StoreResult<()> {
        self.append_jsonl("visions.jsonl", value)
    }

    pub fn append_provider_child_thread(&self, value: &ProviderChildThread) -> StoreResult<()> {
        self.append_jsonl("provider_child_threads.jsonl", value)
    }

    pub fn append_workflow_run(&self, value: &WorkflowRun) -> StoreResult<()> {
        self.append_jsonl("workflow_runs.jsonl", value)
    }

    pub fn append_workflow_step(&self, value: &WorkflowStep) -> StoreResult<()> {
        self.append_jsonl("workflow_steps.jsonl", value)
    }

    pub fn append_workflow_patch(&self, value: &WorkflowPatch) -> StoreResult<()> {
        self.append_jsonl("workflow_patches.jsonl", value)
    }

    pub fn append_workflow_artifact_manifest(
        &self,
        value: &WorkflowArtifactManifest,
    ) -> StoreResult<()> {
        self.append_jsonl("workflow_artifact_manifests.jsonl", value)
    }

    /// Reconstruct one raw historical TeamRun projection during an explicit
    /// Legacy import. Current callers must use the combined TeamRun/member
    /// admission APIs so a non-empty run is never published without its
    /// canonical MemberRuns.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn legacy_import_append_team_run_projection(
        &self,
        value: &AgentTeamRun,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(current) =
            latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
                run.id.clone()
            })
            .remove(&value.id)
        {
            if current == *value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REVISION_REQUIRES_CAS: raw TeamRun revision {} cannot change identity, Host binding, scope, lifecycle, or membership",
                value.id
            )));
        }
        self.append_jsonl_unlocked("team_runs.jsonl", value)
    }
}
