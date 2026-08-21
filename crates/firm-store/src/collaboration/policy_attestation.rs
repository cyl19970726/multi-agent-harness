use super::*;

impl HarnessStore {
    /// Persist a server-authored proof of exact source Work authority. The
    /// source WorkApplicationService is the only writer; a Host request can
    /// subsequently reference only this immutable attestation ID.
    pub fn put_source_work_attestation(
        &self,
        context: &CollaborationMutationContext,
        attestation: &SourceWorkAttestation,
        resolved_work_application_service: &ActorRef,
        current_source_gateway_generation: u64,
    ) -> StoreResult<CollaborationMutationResult<SourceWorkAttestation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if context.expected_revision != 0
            || context.company_id != attestation.company_id
            || !exact_actor(
                &context.authenticated_actor,
                resolved_work_application_service,
            )
            || attestation.work_application_service_ref != *resolved_work_application_service
            || attestation.source_gateway_generation != current_source_gateway_generation
            || current_source_gateway_generation == 0
            || attestation.source_work_ref.placement_generation != 1
            || attestation.source_work_ref.team_id.is_empty()
            || attestation.source_work_ref.node_id.is_empty()
            || attestation.attestation_digest != source_work_attestation_digest(attestation)?
        {
            return Err(collaboration_error(
                FabricErrorCode::SourceWorkAttestationInvalid,
                "source Work attestation is not server-authored for the exact current Work, Team, and gateway generation",
                "source_work_attestation",
                &attestation.id,
                None,
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "source_work_attestation",
            &attestation.id,
            serde_json::to_value(attestation)?,
            attestation,
            Vec::new(),
        )
    }

    pub fn list_collaboration_delegations(
        &self,
        company_id: &str,
        filter: &CollaborationDelegationFilter,
        cursor: Option<CollaborationCursor>,
        limit: usize,
    ) -> StoreResult<CollaborationPage<WorkDelegationV1>> {
        if limit == 0 || limit > 500 {
            return Err(collaboration_error(
                FabricErrorCode::ProtocolMismatch,
                "collaboration list limit must be between 1 and 500",
                "collaboration_cursor",
                company_id,
                None,
            ));
        }
        let operations = self.collaboration_operations_unlocked()?;
        let latest_sequence = operations
            .iter()
            .map(|operation| operation.store_sequence)
            .max()
            .unwrap_or(0);
        let as_of = cursor
            .map(|value| value.as_of_store_sequence)
            .unwrap_or(latest_sequence);
        if as_of > latest_sequence {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "collaboration cursor points beyond the current Store sequence",
                "collaboration_cursor",
                company_id,
                Some(latest_sequence),
            ));
        }
        let mut latest = BTreeMap::<String, WorkDelegationV1>::new();
        for operation in operations.into_iter().filter(|operation| {
            operation.company_id == company_id
                && operation.aggregate_kind == "work_delegation_v1"
                && operation.store_sequence <= as_of
        }) {
            let delegation: WorkDelegationV1 =
                serde_json::from_value(operation.resulting_projection)?;
            latest.insert(delegation.id.clone(), delegation);
        }
        let filtered = latest
            .into_values()
            .filter(|delegation| {
                filter
                    .source_team_id
                    .as_ref()
                    .is_none_or(|team_id| &delegation.source_team_id == team_id)
                    && filter
                        .target_team_id
                        .as_ref()
                        .is_none_or(|team_id| &delegation.target_placement.team_id == team_id)
                    && filter.node_id.as_ref().is_none_or(|node_id| {
                        &delegation.source_node_id == node_id
                            || &delegation.target_placement.node_id == node_id
                    })
                    && filter.state.is_none_or(|state| delegation.state == state)
            })
            .collect::<Vec<_>>();
        let offset = cursor.map(|value| value.offset).unwrap_or(0);
        if offset > filtered.len() {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                "collaboration cursor offset is outside the frozen snapshot",
                "collaboration_cursor",
                company_id,
                Some(as_of),
            ));
        }
        let items = filtered
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset + items.len();
        Ok(CollaborationPage {
            items,
            as_of_store_sequence: as_of,
            next_cursor: (next_offset < filtered.len()).then_some(CollaborationCursor {
                as_of_store_sequence: as_of,
                offset: next_offset,
            }),
        })
    }

    pub fn put_collaboration_inbound_policy(
        &self,
        context: &CollaborationMutationContext,
        policy: &DelegationInboundPolicy,
        resolved_target_host: &ActorRef,
    ) -> StoreResult<CollaborationMutationResult<DelegationInboundPolicy>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if policy.company_id != context.company_id
            || policy.created_by_target_host != *resolved_target_host
            || !exact_actor(&context.authenticated_actor, resolved_target_host)
        {
            return Err(collaboration_error(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved target Host may author inbound policy",
                "delegation_inbound_policy",
                &policy.id,
                None,
            ));
        }
        if policy.revision != context.expected_revision + 1
            || policy.max_active_delegations == 0
            || policy.allowed_outcome_classes.is_empty()
            || policy.target_team_id == policy.source_team_id
        {
            return Err(collaboration_error(
                FabricErrorCode::DelegationPolicyRejected,
                "inbound policy revision, scope, outcome classes, or active limit is invalid",
                "delegation_inbound_policy",
                &policy.id,
                Some(context.expected_revision),
            ));
        }
        self.commit_collaboration_projection_unlocked(
            context,
            "delegation_inbound_policy",
            &policy.id,
            serde_json::to_value(policy)?,
            policy,
            Vec::new(),
        )
    }

    pub(super) fn commit_collaboration_projection_unlocked<T>(
        &self,
        context: &CollaborationMutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
    ) -> StoreResult<CollaborationMutationResult<T>>
    where
        T: Clone + Serialize + serde::de::DeserializeOwned,
    {
        let fingerprint = canonical_json_fingerprint(&request_payload);
        let mut operations = self.collaboration_operations_unlocked()?;
        if let Some(existing) = operations.iter().find(|operation| {
            operation.company_id == context.company_id
                && operation.authenticated_actor == context.authenticated_actor
                && operation.command_name == context.command_name
                && operation.idempotency_key == context.idempotency_key
        }) {
            if existing.request_fingerprint != fingerprint
                || existing.aggregate_kind != aggregate_kind
                || existing.aggregate_id != aggregate_id
            {
                return Err(collaboration_error(
                    FabricErrorCode::IdempotencyConflict,
                    "idempotency key was reused for a different collaboration mutation",
                    aggregate_kind,
                    aggregate_id,
                    Some(existing.resulting_revision),
                ));
            }
            // Rewriting the complete durable frames also removes a possible
            // non-newline torn tail left by a crash. Exact replay is therefore
            // both effect-idempotent and a bounded recovery path.
            self.write_collaboration_operations_atomic_unlocked(&operations)?;
            return Ok(CollaborationMutationResult {
                projection: serde_json::from_value(existing.resulting_projection.clone())?,
                operation: existing.clone(),
                replayed: true,
            });
        }
        let current_revision = operations
            .iter()
            .filter(|operation| {
                operation.company_id == context.company_id
                    && operation.aggregate_kind == aggregate_kind
                    && operation.aggregate_id == aggregate_id
            })
            .map(|operation| operation.resulting_revision)
            .max()
            .unwrap_or(0);
        if current_revision != context.expected_revision {
            return Err(collaboration_error(
                FabricErrorCode::RevisionConflict,
                format!(
                    "expected revision {}, current revision is {current_revision}",
                    context.expected_revision
                ),
                aggregate_kind,
                aggregate_id,
                Some(current_revision),
            ));
        }
        let operation = CollaborationOperation {
            store_version: COLLABORATION_STORE_VERSION.into(),
            company_id: context.company_id.clone(),
            command_name: context.command_name.clone(),
            authenticated_actor: context.authenticated_actor.clone(),
            idempotency_key: context.idempotency_key.clone(),
            request_fingerprint: fingerprint,
            aggregate_kind: aggregate_kind.into(),
            aggregate_id: aggregate_id.into(),
            store_sequence: operations
                .iter()
                .map(|operation| operation.store_sequence)
                .max()
                .unwrap_or(0)
                + 1,
            resulting_revision: current_revision + 1,
            resulting_projection: serde_json::to_value(resulting_projection)?,
            immutable_side_records,
            created_at: context.occurred_at.clone(),
        };
        operations.push(operation.clone());
        self.write_collaboration_operations_atomic_unlocked(&operations)?;
        Ok(CollaborationMutationResult {
            projection: resulting_projection.clone(),
            operation,
            replayed: false,
        })
    }
}
