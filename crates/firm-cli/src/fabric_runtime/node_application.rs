use super::*;

pub(super) struct Wave4cApplication {
    pub(super) probe: ProbeApplication,
    pub(super) firm_home: PathBuf,
    pub(super) node_id: String,
    pub(super) daemon_id: String,
    pub(super) daemon_generation: u64,
}

impl Wave4cApplication {
    pub(super) fn target_store(
        &self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, HarnessStore), FabricError> {
        let execution_space_id =
            operation
                .target_execution_space_id
                .as_deref()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::TargetNotPlaced,
                        "routed application has no target Execution Space",
                    )
                })?;
        let space = crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
            .map_err(|error| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    format!("Execution Space registry failed: {error}"),
                )
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::TargetNotPlaced,
                    "target Execution Space is not registered on this Node",
                )
            })?;
        Ok((
            execution_space_id.into(),
            HarnessStore::new(space.store_root),
        ))
    }

    pub(super) fn persist_message(
        &self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        let message = crate::remote_fabric::resolved_message_from_operation(operation)?;
        let (execution_space_id, store) = self.target_store(operation)?;
        let context = harness_core::agentfirm_api::MutationContext {
            execution_space_id,
            authenticated_actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: self.daemon_id.clone(),
            },
            authority_actor: None,
            command_name: "remote_message_persist".into(),
            idempotency_key: operation.id.clone(),
            expected_version: 0,
            request_fingerprint: Some(harness_fabric::json_digest(operation).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?),
        };
        let persisted = store
            .persist_remote_message(
                &context,
                operation,
                message,
                &self.node_id,
                &self.daemon_id,
                self.daemon_generation,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::UnauthorizedActor, error.to_string())
            })?;
        Ok((
            "agentfirm.remote_fabric.message_persisted.v1".into(),
            serde_json::json!({
                "message_id": persisted.projection.id,
                "canonical_event_id": persisted.event.id,
                "replayed": persisted.replayed,
            }),
            harness_fabric::EffectCertainty::Applied,
        ))
    }
}

impl NodeApplication for Wave4cApplication {
    fn authorize_artifact_download(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
        capability: &harness_fabric::ArtifactCapability,
    ) -> Result<(), FabricError> {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "artifact download is not a collaboration operation",
            ));
        };
        if reference.business_kind != "artifact_grant" {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "artifact download is not an artifact grant",
            ));
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        validate_artifact_grant_authority(
            &payload,
            capability,
            &reference,
            operation,
            &self.node_id,
        )
    }

    fn replay_downloaded_artifact(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<Option<(String, serde_json::Value, harness_fabric::EffectCertainty)>, FabricError>
    {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Ok(None);
        };
        if reference.business_kind != "artifact_grant" {
            return Ok(None);
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let (_, store) = self.target_store(operation)?;
        let Some(import) = store
            .collaboration_artifact_import(&operation.company_id, &payload.manifest.id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
        else {
            return Ok(None);
        };
        if import.operation_id != operation.id
            || import.delegation_id != payload.delegation_id
            || import.artifact_digest != payload.manifest.sha256
            || import.source_node_id != self.node_id
            || import.source_node_daemon_id != self.daemon_id
            || import.source_node_daemon_generation != self.daemon_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "artifact import replay changed operation, bytes, or NodeDaemon generation",
            ));
        }
        store
            .read_collaboration_artifact_import_bytes(&operation.company_id, &payload.manifest.id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ArtifactTampered, error.to_string())
            })?;
        Ok(Some((
            "agentfirm.collaboration.artifact_imported.v1".into(),
            serde_json::json!({"artifact_import": import, "replayed": true}),
            harness_fabric::EffectCertainty::Applied,
        )))
    }

    fn apply(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::Probe(_)
            | harness_fabric::ClosedOperationBody::ReconcileProbe(_) => self.probe.apply(operation),
            harness_fabric::ClosedOperationBody::RuntimeCommand(_) => {
                let envelope = crate::remote_fabric::resolved_runtime_command_from_operation(
                    operation,
                    &self.node_id,
                    &self.daemon_id,
                    self.daemon_generation,
                )?;
                let (result, effect) = dispatch_resolved_runtime_command(
                    &self.firm_home,
                    operation,
                    &envelope,
                    &self.node_id,
                    &self.daemon_id,
                    self.daemon_generation,
                )?;
                Ok((
                    "agentfirm.remote_fabric.runtime_command_result.v1".into(),
                    result,
                    effect,
                ))
            }
            harness_fabric::ClosedOperationBody::Message(_) => self.persist_message(operation),
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => {
                match reference.business_kind.as_str() {
                    "target_work_create"
                    | "delegation_propose"
                    | "delegation_decide"
                    | "delegation_cancel_request"
                    | "delegation_cancel_decide"
                    | "remote_fact_publish" => {
                        let (_, store) = self.target_store(operation)?;
                        harness_store::apply_collaboration_target_operation(
                            &store,
                            operation,
                            &format!(
                                "unix-ms:{}",
                                harness_fabric::gateway_runtime::now_unix_ms()?
                            ),
                        )
                    }
                    "artifact_grant" => Err(FabricError::unknown(
                        operation.id.clone(),
                        "artifact grant cannot be applied before its one-use capability is consumed and source bytes are durably imported",
                    )),
                    "team_message_deliver" => self.persist_message(operation),
                    _ => Err(FabricError::none(
                        FabricErrorCode::FeatureIncompatible,
                        "target Node has no local business authority for this Control Plane-owned collaboration kind",
                    )),
                }
            }
            _ => Err(FabricError::none(
                FabricErrorCode::FeatureIncompatible,
                "Node application adapter does not own this routed reference kind",
            )),
        }
    }

    fn apply_downloaded_artifact(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
        artifact_id: &str,
        artifact_digest: &str,
        bytes: &[u8],
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "downloaded artifact is not a collaboration operation",
            ));
        };
        if reference.business_kind != "artifact_grant" {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "downloaded artifact is not an artifact grant",
            ));
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        validate_artifact_grant_authority(
            &payload,
            &payload.read_capability,
            &reference,
            operation,
            &self.node_id,
        )?;
        let delegation_id = payload.delegation_id.clone();
        let (_, store) = self.target_store(operation)?;
        let delegation = payload.delegation;
        let attestation = payload.source_work_attestation;
        if delegation.source_node_id != self.node_id
            || operation.target_node_id != self.node_id
            || payload.manifest.id != artifact_id
            || payload.manifest.sha256 != artifact_digest
            || payload.manifest.size_bytes != bytes.len() as u64
            || payload.read_capability.artifact_id != artifact_id
            || payload.read_capability.artifact_digest != artifact_digest
            || harness_fabric::sha256_hex(bytes) != artifact_digest
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactTampered,
                "downloaded artifact bytes or source Node disagree with canonical authority",
            ));
        }
        let imported_at = harness_fabric::gateway_runtime::now_unix_ms()?;
        let import = harness_core::collaboration::ArtifactImport {
            id: format!("artifact-import:{artifact_id}"),
            company_id: operation.company_id.clone(),
            delegation_id,
            artifact_id: artifact_id.into(),
            artifact_digest: artifact_digest.into(),
            size_bytes: bytes.len() as u64,
            source_node_id: self.node_id.clone(),
            source_node_daemon_id: self.daemon_id.clone(),
            source_node_daemon_generation: self.daemon_generation,
            source_team_id: delegation.source_team_id.clone(),
            source_host_ref: attestation.source_host_ref.clone(),
            source_work_ref: delegation.source_work_ref.clone(),
            operation_id: operation.id.clone(),
            imported_at_unix_ms: imported_at,
            revision: 1,
        };
        let persisted = store
            .persist_collaboration_artifact_import_with_frozen_authority(
                &harness_store::CollaborationMutationContext {
                    company_id: operation.company_id.clone(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: self.daemon_id.clone(),
                    },
                    command_name: "artifact_import.persist".into(),
                    idempotency_key: operation.id.clone(),
                    expected_revision: 0,
                    occurred_at: format!("unix-ms:{imported_at}"),
                },
                &import,
                bytes,
                &delegation,
                &attestation,
            )
            .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
        Ok((
            "agentfirm.collaboration.artifact_imported.v1".into(),
            serde_json::json!({"artifact_import": persisted.projection, "replayed": persisted.replayed}),
            harness_fabric::EffectCertainty::Applied,
        ))
    }
}

pub(super) fn dispatch_resolved_runtime_command(
    firm_home: &Path,
    operation: &harness_fabric::RoutedOperation,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
    target_node_id: &str,
    target_node_daemon_id: &str,
    target_node_daemon_generation: u64,
) -> Result<(serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
    use harness_core::agentfirm_api::{RuntimeCommandStatus, RuntimeEffectCertainty};

    crate::remote_fabric::validate_resolved_runtime_command(
        operation,
        envelope,
        target_node_id,
        target_node_daemon_id,
        target_node_daemon_generation,
    )?;
    let transport = crate::supervisor_daemon::runtime_command_via_socket(
        firm_home,
        &envelope.target_node_id,
        envelope,
    );
    let space = crate::execution_space::context_for_id(firm_home, &envelope.execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::TargetNotPlaced,
                "RuntimeCommand target Execution Space is not registered on this Node",
            )
        })?;
    let record = HarnessStore::new(space.store_root)
        .runtime_commands(&envelope.execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|record| record.id == envelope.id);
    let transport_detail = match transport {
        Ok(response) => response
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "NodeDaemon returned a non-terminal response".into()),
        Err(error) => format!("NodeDaemon transport ended before a response: {error}"),
    };
    match record {
        Some(record)
            if record.status == RuntimeCommandStatus::Applied
                && record.effect_certainty == RuntimeEffectCertainty::Applied =>
        {
            Ok((
                serde_json::json!({
                    "runtime_command_id": record.id,
                    "status": record.status,
                    "result": record.result,
                }),
                harness_fabric::EffectCertainty::Applied,
            ))
        }
        Some(record)
            if record.status == RuntimeCommandStatus::Failed
                && matches!(
                    record.effect_certainty,
                    RuntimeEffectCertainty::None | RuntimeEffectCertainty::NotApplied
                ) =>
        {
            Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                record.failure_code.unwrap_or(transport_detail),
            ))
        }
        Some(record)
            if record.status == RuntimeCommandStatus::RecoveryRequired
                || record.effect_certainty == RuntimeEffectCertainty::Unknown =>
        {
            let mut failure = FabricError::unknown(
                operation.id.clone(),
                record.failure_code.unwrap_or(transport_detail),
            );
            failure
                .details
                .insert("runtime_command_id".into(), envelope.id.clone());
            failure.details.insert(
                "reconciliation".into(),
                "resolve the durable target RuntimeCommand before any retry".into(),
            );
            Err(failure)
        }
        Some(record) => Err(FabricError::unknown(
            operation.id.clone(),
            format!(
                "RuntimeCommand remained non-terminal ({:?}/{:?}): {transport_detail}",
                record.status, record.effect_certainty
            ),
        )),
        None => Err(FabricError::unknown(
            operation.id.clone(),
            format!("RuntimeCommand has no durable target admission: {transport_detail}"),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn serve_host_http(
    addr: &str,
    trusted_origin: &str,
    host_token: &str,
    company_id: &str,
    instance_id: &str,
    generation: u64,
    store: Arc<harness_fabric::FabricStore>,
    artifact_key: [u8; 32],
    capability_key: [u8; 32],
    ca: &harness_fabric::pki::FabricCaMaterial,
    collaboration_root: PathBuf,
    stop: Arc<AtomicBool>,
) -> CliResult<()> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(company_id, artifact_key);
    let control = ControlPlane::new(company_id, instance_id, &store, &keys, capability_key);
    while !stop.load(Ordering::SeqCst) {
        let (stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let request = match read_http_request(stream, trusted_origin, host_token) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("Remote Fabric Host REST request rejected: {error}");
                continue;
            }
        };
        handle_host_http(
            request,
            trusted_origin,
            host_token,
            &control,
            generation,
            ca,
            &collaboration_root,
        )?;
    }
    Ok(())
}
