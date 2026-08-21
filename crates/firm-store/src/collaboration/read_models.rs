use super::*;

impl HarnessStore {
    pub fn list_collaboration_delegations_for_actor(
        &self,
        company_id: &str,
        actor: &ActorRef,
        filter: &CollaborationDelegationFilter,
        cursor: Option<CollaborationScopedCursor>,
        limit: usize,
    ) -> StoreResult<CollaborationScopedPage<WorkDelegationV1>> {
        if limit == 0 || limit > 500 {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_INVALID: limit must be between 1 and 500".into(),
            ));
        }
        let operations = self.collaboration_operations_unlocked()?;
        let latest_sequence = operations
            .iter()
            .map(|row| row.store_sequence)
            .max()
            .unwrap_or(0);
        let actor_digest = canonical_json_fingerprint(&serde_json::to_value(actor)?);
        let filter_digest = canonical_json_fingerprint(&serde_json::to_value(filter)?);
        let as_of = cursor
            .as_ref()
            .map(|value| value.as_of_store_sequence)
            .unwrap_or(latest_sequence);
        if as_of > latest_sequence
            || cursor.as_ref().is_some_and(|value| {
                value.company_id != company_id
                    || value.actor_digest != actor_digest
                    || value.filter_digest != filter_digest
            })
        {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_SCOPE_MISMATCH: cursor actor, Company, filter or snapshot is invalid".into(),
            ));
        }
        let mut attestations = BTreeMap::<String, SourceWorkAttestation>::new();
        let mut latest = BTreeMap::<String, WorkDelegationV1>::new();
        for operation in operations
            .into_iter()
            .filter(|row| row.company_id == company_id && row.store_sequence <= as_of)
        {
            if operation.aggregate_kind == "source_work_attestation" {
                let value: SourceWorkAttestation =
                    serde_json::from_value(operation.resulting_projection)?;
                attestations.insert(value.id.clone(), value);
            } else if operation.aggregate_kind == "work_delegation_v1" {
                let value: WorkDelegationV1 =
                    serde_json::from_value(operation.resulting_projection)?;
                latest.insert(value.id.clone(), value);
            }
        }
        let rows = latest.into_values().collect::<Vec<_>>();
        let mut raw_offset = cursor.as_ref().map(|value| value.raw_offset).unwrap_or(0);
        let mut visible_progress = cursor
            .as_ref()
            .map(|value| value.visible_progress)
            .unwrap_or(0);
        if raw_offset > rows.len() {
            return Err(StoreError::Conflict(
                "COLLABORATION_CURSOR_INVALID: raw offset is outside the frozen snapshot".into(),
            ));
        }
        let mut items = Vec::new();
        // Bound raw work independently of visible results. A page containing
        // only rows hidden from this actor is still a valid advancing page;
        // clients follow its opaque cursor until visible rows or EOF. This
        // prevents a hostile Company history from turning one scoped request
        // into an unbounded scan without letting hidden rows consume the
        // caller's visible item limit.
        let raw_scan_budget = limit.saturating_mul(4).max(limit);
        let raw_scan_end = raw_offset.saturating_add(raw_scan_budget).min(rows.len());
        while raw_offset < raw_scan_end && items.len() < limit {
            let delegation = &rows[raw_offset];
            raw_offset += 1;
            let source_host = attestations
                .get(&delegation.source_work_attestation_id)
                .map(|value| &value.source_host_ref);
            let visible = actor == &delegation.source_owner_ref
                || source_host == Some(actor)
                || actor == &delegation.target_host_ref;
            let matches = filter
                .source_team_id
                .as_ref()
                .is_none_or(|value| value == &delegation.source_team_id)
                && filter
                    .target_team_id
                    .as_ref()
                    .is_none_or(|value| value == &delegation.target_placement.team_id)
                && filter.node_id.as_ref().is_none_or(|value| {
                    value == &delegation.source_node_id
                        || value == &delegation.target_placement.node_id
                })
                && filter.state.is_none_or(|value| value == delegation.state);
            if visible && matches {
                items.push(delegation.clone());
                visible_progress += 1;
            }
        }
        Ok(CollaborationScopedPage {
            items,
            as_of_store_sequence: as_of,
            next_cursor: (raw_offset < rows.len()).then_some(CollaborationScopedCursor {
                company_id: company_id.into(),
                actor_digest,
                filter_digest,
                as_of_store_sequence: as_of,
                raw_offset,
                visible_progress,
            }),
        })
    }
    pub fn collaboration_artifact_import(
        &self,
        company_id: &str,
        artifact_id: &str,
    ) -> StoreResult<Option<ArtifactImport>> {
        self.latest_collaboration_projection_unlocked(company_id, "artifact_import", artifact_id)
    }

    pub fn read_collaboration_artifact_import_bytes(
        &self,
        company_id: &str,
        artifact_id: &str,
    ) -> StoreResult<Vec<u8>> {
        let import = self
            .collaboration_artifact_import(company_id, artifact_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!("ARTIFACT_IMPORT_NOT_FOUND: {artifact_id}"))
            })?;
        let path = self
            .root()
            .join("collaboration-artifact-imports")
            .join(&import.artifact_digest);
        let bytes = std::fs::read(path)?;
        if bytes.len() as u64 != import.size_bytes
            || firm_fabric::sha256_hex(&bytes) != import.artifact_digest
        {
            return Err(StoreError::Conflict(
                "ARTIFACT_IMPORT_TAMPERED: imported bytes disagree with the canonical import"
                    .into(),
            ));
        }
        Ok(bytes)
    }

    pub fn persist_collaboration_artifact_import(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        bytes: &[u8],
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &import.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact import requires the exact current Delegation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact import requires the exact source Work attestation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        self.persist_collaboration_artifact_import_unlocked(
            context,
            import,
            bytes,
            &delegation,
            &attestation,
        )
    }

    /// Persist source-owned artifact bytes using the immutable central
    /// Delegation/attestation snapshot already authenticated by the routed
    /// operation. The source Node never copies the central relationship into
    /// its local collaboration ledger.
    pub fn persist_collaboration_artifact_import_with_frozen_authority(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        bytes: &[u8],
        delegation: &WorkDelegationV1,
        attestation: &SourceWorkAttestation,
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.persist_collaboration_artifact_import_unlocked(
            context,
            import,
            bytes,
            delegation,
            attestation,
        )
    }

    pub(super) fn persist_collaboration_artifact_import_unlocked(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        bytes: &[u8],
        delegation: &WorkDelegationV1,
        attestation: &SourceWorkAttestation,
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        if import.company_id != context.company_id
            || delegation.company_id != context.company_id
            || attestation.company_id != context.company_id
            || delegation.id != import.delegation_id
            || delegation.source_work_attestation_id != attestation.id
            || delegation.source_work_ref != attestation.source_work_ref
            || delegation.source_owner_ref != attestation.source_owner_ref
            || import.revision != 1
            || context.authenticated_actor.kind != ActorKind::Service
            || context.authenticated_actor.id != import.source_node_daemon_id
            || import.source_node_id != delegation.source_node_id
            || import.source_node_daemon_id.trim().is_empty()
            || import.source_node_daemon_generation == 0
            || import.source_team_id != delegation.source_team_id
            || import.source_host_ref != attestation.source_host_ref
            || import.source_work_ref != delegation.source_work_ref
            || import.size_bytes != bytes.len() as u64
            || import.artifact_digest != firm_fabric::sha256_hex(bytes)
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact import bytes or source authority disagree with the current Delegation",
                "artifact_import",
                &import.artifact_id,
                None,
            ));
        }
        if let Some(existing) = self.latest_collaboration_projection_unlocked::<ArtifactImport>(
            &context.company_id,
            "artifact_import",
            &import.artifact_id,
        )? {
            if existing != *import {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "artifact import replay changed immutable bytes or authority",
                    "artifact_import",
                    &import.artifact_id,
                    Some(existing.revision),
                ));
            }
            return self.commit_collaboration_projection_unlocked(
                context,
                "artifact_import",
                &import.artifact_id,
                serde_json::to_value(&existing)?,
                &existing,
                Vec::new(),
            );
        }
        let directory = self.root().join("collaboration-artifact-imports");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(&import.artifact_digest);
        let next = directory.join(format!("{}.next", import.artifact_digest));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&next, &path)?;
        std::fs::File::open(&directory)?.sync_all()?;
        self.commit_collaboration_projection_unlocked(
            context,
            "artifact_import",
            &import.artifact_id,
            serde_json::to_value(import)?,
            import,
            Vec::new(),
        )
    }

    /// Fold a target Node's terminal import into central relationship state.
    /// Artifact bytes remain solely in the source Execution Space.
    pub fn record_collaboration_artifact_import(
        &self,
        context: &CollaborationMutationContext,
        import: &ArtifactImport,
        routed_operation_id: &str,
        resolved_control_plane_actor: &ActorRef,
    ) -> StoreResult<CollaborationMutationResult<ArtifactImport>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let delegation = self
            .latest_collaboration_projection_unlocked::<WorkDelegationV1>(
                &context.company_id,
                "work_delegation_v1",
                &import.delegation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::RevisionConflict,
                    "artifact import result references no central Delegation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        let attestation = self
            .latest_collaboration_projection_unlocked::<SourceWorkAttestation>(
                &context.company_id,
                "source_work_attestation",
                &delegation.source_work_attestation_id,
            )?
            .ok_or_else(|| {
                collaboration_error(
                    FabricErrorCode::SourceWorkAttestationInvalid,
                    "artifact import result has no source Work attestation",
                    "artifact_import",
                    &import.artifact_id,
                    None,
                )
            })?;
        if !exact_actor(&context.authenticated_actor, resolved_control_plane_actor)
            || import.company_id != context.company_id
            || import.operation_id != routed_operation_id
            || import.source_node_id != delegation.source_node_id
            || import.source_team_id != delegation.source_team_id
            || import.source_host_ref != attestation.source_host_ref
            || import.source_work_ref != delegation.source_work_ref
            || import.source_node_daemon_id.trim().is_empty()
            || import.source_node_daemon_generation == 0
            || import.revision != 1
        {
            return Err(collaboration_error(
                FabricErrorCode::ArtifactScopeUnauthorized,
                "artifact import result changed Delegation, source Host/Work/Node, operation, or daemon generation",
                "artifact_import",
                &import.artifact_id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "artifact_import",
            &import.artifact_id,
            serde_json::to_value(import)?,
            import,
            Vec::new(),
        )
    }

    /// Hold the canonical collaboration writer lock across a caller-supplied
    /// authority check and its downstream durable commit. This is the only
    /// supported lock order for cross-store routing: collaboration first,
    /// Fabric second. The callback cannot mutate collaboration state.
    #[allow(clippy::result_large_err)]
    pub fn with_collaboration_authority_fence<T>(
        &self,
        validate: impl FnOnce(&Self) -> Result<(), firm_fabric::FabricError>,
        commit: impl FnOnce() -> Result<T, firm_fabric::FabricError>,
    ) -> Result<T, firm_fabric::FabricError> {
        self.init().map_err(|error| {
            firm_fabric::FabricError::none(
                firm_fabric::FabricErrorCode::StoreUnavailable,
                error.to_string(),
            )
        })?;
        let _lock = self.acquire_write_lock().map_err(|error| {
            firm_fabric::FabricError::none(
                firm_fabric::FabricErrorCode::StoreUnavailable,
                error.to_string(),
            )
        })?;
        validate(self)?;
        commit()
    }

    pub(super) fn collaboration_operations_unlocked(
        &self,
    ) -> StoreResult<Vec<CollaborationOperation>> {
        let path = self.root().join(COLLABORATION_OPERATIONS_LEDGER);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(path)?;
        let durable_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut operations = Vec::new();
        for row in bytes[..durable_len].split(|byte| *byte == b'\n') {
            if row.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            operations.push(serde_json::from_slice(row)?);
        }
        Ok(operations)
    }

    pub(super) fn write_collaboration_operations_atomic_unlocked(
        &self,
        operations: &[CollaborationOperation],
    ) -> StoreResult<()> {
        let path = self.root().join(COLLABORATION_OPERATIONS_LEDGER);
        let next = self
            .root()
            .join("agentfirm_collaboration_operations.jsonl.next");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&next)?;
        for operation in operations {
            serde_json::to_writer(&mut file, operation)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&next, &path)?;
        std::fs::File::open(self.root())?.sync_all()?;
        Ok(())
    }

    pub fn collaboration_operations(&self) -> StoreResult<Vec<CollaborationOperation>> {
        self.collaboration_operations_unlocked()
    }

    pub(super) fn latest_collaboration_projection_unlocked<T: serde::de::DeserializeOwned>(
        &self,
        company_id: &str,
        aggregate_kind: &str,
        aggregate_id: &str,
    ) -> StoreResult<Option<T>> {
        self.collaboration_operations_unlocked()?
            .into_iter()
            .filter(|operation| {
                operation.company_id == company_id
                    && operation.aggregate_kind == aggregate_kind
                    && operation.aggregate_id == aggregate_id
            })
            .max_by_key(|operation| operation.resulting_revision)
            .map(|operation| serde_json::from_value(operation.resulting_projection))
            .transpose()
            .map_err(StoreError::from)
    }

    pub(super) fn latest_collaboration_delegations_unlocked(
        &self,
        company_id: &str,
    ) -> StoreResult<BTreeMap<String, WorkDelegationV1>> {
        let mut latest = BTreeMap::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id == company_id
                && operation.aggregate_kind == "work_delegation_v1"
            {
                let delegation: WorkDelegationV1 =
                    serde_json::from_value(operation.resulting_projection)?;
                latest.insert(delegation.id.clone(), delegation);
            }
        }
        Ok(latest)
    }

    pub(super) fn latest_cancellation_request_unlocked(
        &self,
        company_id: &str,
        delegation_id: &str,
        request_id: &str,
    ) -> StoreResult<Option<DelegationCancellationRequest>> {
        let mut latest = None;
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "work_delegation_v1"
                || operation.aggregate_id != delegation_id
            {
                continue;
            }
            for record in operation.immutable_side_records {
                let Ok(request) = serde_json::from_value::<DelegationCancellationRequest>(record)
                else {
                    continue;
                };
                if request.id == request_id
                    && latest
                        .as_ref()
                        .is_none_or(|current: &DelegationCancellationRequest| {
                            request.revision > current.revision
                        })
                {
                    latest = Some(request);
                }
            }
        }
        Ok(latest)
    }

    pub fn collaboration_cancellation_requests(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Vec<DelegationCancellationRequest>> {
        let mut latest = BTreeMap::<String, DelegationCancellationRequest>::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "work_delegation_v1"
                || operation.aggregate_id != delegation_id
            {
                continue;
            }
            for record in operation.immutable_side_records {
                let Ok(request) = serde_json::from_value::<DelegationCancellationRequest>(record)
                else {
                    continue;
                };
                if latest
                    .get(&request.id)
                    .is_none_or(|current| request.revision > current.revision)
                {
                    latest.insert(request.id.clone(), request);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    pub fn collaboration_delegations(
        &self,
        company_id: &str,
    ) -> StoreResult<Vec<WorkDelegationV1>> {
        Ok(self
            .latest_collaboration_delegations_unlocked(company_id)?
            .into_values()
            .collect())
    }

    pub fn collaboration_delegation(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Option<WorkDelegationV1>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "work_delegation_v1",
            delegation_id,
        )
    }

    pub fn collaboration_source_work_attestation(
        &self,
        company_id: &str,
        attestation_id: &str,
    ) -> StoreResult<Option<SourceWorkAttestation>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "source_work_attestation",
            attestation_id,
        )
    }

    pub fn collaboration_inbound_policy(
        &self,
        company_id: &str,
        policy_id: &str,
    ) -> StoreResult<Option<DelegationInboundPolicy>> {
        self.latest_collaboration_projection_unlocked(
            company_id,
            "delegation_inbound_policy",
            policy_id,
        )
    }

    pub fn collaboration_cancellation_request(
        &self,
        company_id: &str,
        delegation_id: &str,
        request_id: &str,
    ) -> StoreResult<Option<DelegationCancellationRequest>> {
        self.latest_cancellation_request_unlocked(company_id, delegation_id, request_id)
    }

    pub fn collaboration_publications(
        &self,
        company_id: &str,
        delegation_id: &str,
    ) -> StoreResult<Vec<RemoteFactPublication>> {
        let mut latest = BTreeMap::<String, RemoteFactPublication>::new();
        for operation in self.collaboration_operations_unlocked()? {
            if operation.company_id != company_id
                || operation.aggregate_kind != "remote_fact_publication"
            {
                continue;
            }
            let Ok(publication) =
                serde_json::from_value::<RemoteFactPublication>(operation.resulting_projection)
            else {
                continue;
            };
            if publication.delegation_id != delegation_id {
                continue;
            }
            if latest
                .get(&publication.id)
                .is_none_or(|current| publication.fact_revision > current.fact_revision)
            {
                latest.insert(publication.id.clone(), publication);
            }
        }
        Ok(latest.into_values().collect())
    }
}
