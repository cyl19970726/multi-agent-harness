use super::*;

impl HarnessStore {
    pub fn validate_runtime_command(
        &self,
        command: &ControlCommandEnvelope,
        now_unix_ms: u64,
    ) -> StoreResult<()> {
        required(&command.id, "ControlCommandEnvelope.id")?;
        required(
            &command.idempotency_key,
            "ControlCommandEnvelope.idempotency_key",
        )?;
        required(
            &command.required_capability,
            "ControlCommandEnvelope.required_capability",
        )?;
        if command.payload_fingerprint != canonical_json_fingerprint(&command.payload) {
            return Err(trust_error(
                TrustErrorCode::IdempotencyKeyReused,
                "runtime command payload fingerprint is invalid",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        if command.postcondition.status != RuntimePostconditionStatus::Unknown {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "a RuntimeCommand may request a postcondition but cannot claim it satisfied before provider observation",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        if command.authenticated_actor.kind == ActorKind::External {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "external actors cannot issue machine runtime commands",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        if command.expires_unix_ms <= now_unix_ms {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "runtime command expired before NodeDaemon admission",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        self.require_current_node_daemon_unlocked(
            &command.execution_space_id,
            &command.target_node_id,
            &command.target_node_daemon_id,
            command.target_node_daemon_generation,
            &ActorRef {
                kind: ActorKind::Service,
                id: command.target_node_daemon_id.clone(),
            },
            "runtime_command",
            &command.id,
        )
    }

    pub fn runtime_commands(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<RuntimeCommandRecord>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "runtime_command")?
            .values()
            .map(event_projection)
            .collect()
    }

    /// Persist command admission before a provider or process effect. Replay is
    /// resolved by the canonical operation ledger before current-state checks,
    /// while ambiguous prior effects fail closed as RecoveryRequired.
    pub fn prepare_runtime_command(
        &self,
        context: &MutationContext,
        command: &ControlCommandEnvelope,
        now_unix_ms: u64,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let command_fingerprint = runtime_command_envelope_fingerprint(command)?;
        if context.request_fingerprint.as_deref() != Some(command_fingerprint.as_str()) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand full envelope fingerprint was not server-bound",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        // Resolve exact replay before mutable lease/session checks. This
        // returns the original durable result without repeating an effect;
        // changing any envelope field under the same key conflicts.
        if let Some(replay) =
            self.trust_operation_envelopes_unlocked()?
                .into_iter()
                .find(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                        && envelope.authenticated_actor_id == context.authenticated_actor.id
                        && envelope.command_name == context.command_name
                        && envelope.operation.event.idempotency_key == context.idempotency_key
                })
        {
            if replay.operation.event.canonical_request_fingerprint != command_fingerprint
                || replay.operation.event.aggregate_kind != "runtime_command"
                || replay.operation.event.aggregate_id != command.id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "RuntimeCommand idempotency key was reused with a different full envelope",
                    "runtime_command",
                    &command.id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            let latest = self
                .trust_operation_envelopes_unlocked()?
                .into_iter()
                .filter(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.operation.event.aggregate_kind == "runtime_command"
                        && envelope.operation.event.aggregate_id == command.id
                })
                .max_by_key(|envelope| envelope.operation.event.sequence)
                .unwrap_or(replay);
            return Ok(CanonicalMutationResult {
                projection: event_projection(&latest)?,
                event: latest.operation.event,
                replayed: true,
            });
        }
        self.validate_runtime_command(command, now_unix_ms)?;
        if command.execution_space_id != context.execution_space_id
            || command.authenticated_actor
                != context
                    .authority_actor
                    .clone()
                    .unwrap_or_else(|| context.authenticated_actor.clone())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand authority or fingerprint was not server-bound",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        let expected_capability = runtime_command_capability(command.command);
        if command.required_capability != expected_capability {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand capability is not the server-owned capability for this command",
                "runtime_command",
                &command.id,
                None,
            ));
        }
        let requested_start_session = if command.command == RuntimeCommandKind::StartSession
            && command.payload.get("session").is_some()
        {
            Some(
                serde_json::from_value::<AgentSession>(command.payload["session"].clone())
                    .map_err(|error| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            format!("StartSession payload is invalid: {error}"),
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?,
            )
        } else {
            None
        };
        let target_session_id = command.payload["session_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                requested_start_session
                    .as_ref()
                    .map(|session| session.id.clone())
            });
        let target_session_generation =
            command.payload["session_generation"].as_u64().or_else(|| {
                requested_start_session
                    .as_ref()
                    .map(|session| session.runtime_generation)
            });
        if command.command != RuntimeCommandKind::AuthorMessage {
            let session = if let Some(session) = requested_start_session.as_ref() {
                session.clone()
            } else {
                let session_id = target_session_id.as_deref().ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "RuntimeCommand requires an exact target AgentSession",
                        "runtime_command",
                        &command.id,
                        None,
                    )
                })?;
                self.fabric_agent_sessions(&context.execution_space_id)?
                    .into_iter()
                    .find(|session| session.id == session_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "RuntimeCommand target AgentSession does not exist",
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?
            };
            if session.node_id != command.target_node_id
                || session.node_daemon_id != command.target_node_daemon_id
                || session.node_daemon_generation != command.target_node_daemon_generation
                || target_session_generation != Some(session.runtime_generation)
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "RuntimeCommand does not bind the exact current AgentSession and NodeDaemon generation",
                    "runtime_command",
                    &command.id,
                    Some(session.version),
                ));
            }
            if runtime_command_requires_exact_binding(command.command) {
                self.require_live_runtime_binding_unlocked(
                    &session,
                    &command.binding,
                    false,
                    "runtime_command",
                    &command.id,
                    Some(session.version),
                )?;
            }
            Self::require_runtime_command_precondition_unlocked(
                &session,
                command.command,
                &command.precondition,
                false,
                "runtime_command",
                &command.id,
                Some(session.version),
            )?;
            let actor = &command.authenticated_actor;
            let exact_self =
                actor.kind == ActorKind::AgentMember && actor.id == session.agent_member_id;
            let exact_operator = actor.kind == ActorKind::Service
                && (actor.id == session.node_id || actor.id == session.node_daemon_id);
            if !exact_self && !exact_operator {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "AgentSession RuntimeCommand requires exact self or exact machine NodeDaemon/Operator authority; Team Host authority is Team-scoped only",
                    "runtime_command",
                    &command.id,
                    None,
                ));
            }
            if let Some(requested) = requested_start_session.as_ref() {
                let identity = self
                    .fabric_agent_identities(&context.execution_space_id)?
                    .into_iter()
                    .find(|identity| identity.id == requested.agent_member_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "StartSession target AgentIdentity does not exist",
                            "runtime_command",
                            &command.id,
                            None,
                        )
                    })?;
                if requested.effective_permission_ceiling > identity.permission_ceiling {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "StartSession cannot widen the frozen AgentIdentity permission ceiling",
                        "runtime_command",
                        &command.id,
                        None,
                    ));
                }
            }
            let active_bindings = self
                .fabric_work_execution_bindings(&context.execution_space_id)?
                .into_iter()
                .filter(|binding| {
                    binding.agent_session_id == session.id
                        && binding.agent_session_generation == session.runtime_generation
                        && binding.status == WorkExecutionBindingStatus::Active
                })
                .collect::<Vec<_>>();
            match command.command {
                RuntimeCommandKind::DispatchProvider
                | RuntimeCommandKind::StartCycle
                | RuntimeCommandKind::InjectCurrentCycle
                | RuntimeCommandKind::QueueAtNativeBoundary => {
                    if session.lifecycle != AgentSessionStatus::Active {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider dispatch requires the exact active AgentSession",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::CancelProviderTurn
                | RuntimeCommandKind::InterruptCurrentCycle
                | RuntimeCommandKind::CancelPendingInput => {
                    if session.lifecycle != AgentSessionStatus::Active
                        || session.current_turn_id.is_none()
                    {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider cancel requires an exact active provider turn",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::StopSession => {
                    if !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Cold
                            | AgentSessionStatus::Active
                            | AgentSessionStatus::Idle
                            | AgentSessionStatus::Waiting
                            | AgentSessionStatus::Interrupted
                    ) {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "AgentSession stop cannot target a terminal session",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                    if !active_bindings.is_empty() {
                        return Err(trust_error(
                            TrustErrorCode::WorkExecutionBindingActive,
                            format!(
                                "AgentSession stop requires explicit release, rebind, or quiesce of active WorkExecutionBindings first: {}",
                                active_bindings
                                    .iter()
                                    .map(|binding| binding.id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                            "agent_session",
                            &session.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::ReleaseRuntime
                | RuntimeCommandKind::CloseMember
                | RuntimeCommandKind::QuiesceExecutionLane
                | RuntimeCommandKind::DrainRuntime
                | RuntimeCommandKind::InhibitContinuation => {
                    if !matches!(
                        session.lifecycle,
                        AgentSessionStatus::Cold
                            | AgentSessionStatus::Active
                            | AgentSessionStatus::Idle
                            | AgentSessionStatus::Waiting
                            | AgentSessionStatus::Interrupted
                    ) {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "AgentSession stop cannot target a terminal session",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::StartSession
                | RuntimeCommandKind::ResumeSession
                | RuntimeCommandKind::OpenRuntime
                | RuntimeCommandKind::ResumeNativeSession
                | RuntimeCommandKind::ReopenMember
                | RuntimeCommandKind::ReattachLiveRuntime => {
                    if matches!(
                        session.lifecycle,
                        AgentSessionStatus::Closed | AgentSessionStatus::RecoveryRequired
                    ) {
                        return Err(trust_error(
                            TrustErrorCode::InvalidStateTransition,
                            "provider process start/resume cannot target a terminal or recovery-required AgentSession",
                            "runtime_command",
                            &command.id,
                            Some(session.version),
                        ));
                    }
                }
                RuntimeCommandKind::RetireMember
                | RuntimeCommandKind::DeleteNativeSession
                | RuntimeCommandKind::InspectContinuation
                | RuntimeCommandKind::ActivateContinuation
                | RuntimeCommandKind::ResumeContinuation
                | RuntimeCommandKind::ReplaceContinuationCondition
                | RuntimeCommandKind::ClearContinuation
                | RuntimeCommandKind::StopBackgroundTask
                | RuntimeCommandKind::TransferExecutionDriver
                | RuntimeCommandKind::InspectCommandEffect
                | RuntimeCommandKind::ReconcileUnknownEffect
                | RuntimeCommandKind::AbortIfNotApplied
                | RuntimeCommandKind::AuthorMessage => {}
            }
            let ambiguous = self
                .runtime_commands(&context.execution_space_id)?
                .into_iter()
                .any(|prior| {
                    prior.id != command.id
                        && prior.target_session_id.as_deref() == Some(session.id.as_str())
                        && matches!(
                            prior.status,
                            RuntimeCommandStatus::Accepted
                                | RuntimeCommandStatus::Quiesced
                                | RuntimeCommandStatus::RecoveryRequired
                        )
                        && prior.effect_certainty == RuntimeEffectCertainty::Unknown
                        && !matches!(
                            (command.command, prior.command),
                            (
                                RuntimeCommandKind::CancelProviderTurn
                                    | RuntimeCommandKind::InterruptCurrentCycle
                                    | RuntimeCommandKind::StopSession,
                                RuntimeCommandKind::DispatchProvider
                                    | RuntimeCommandKind::StartCycle
                            )
                        )
                });
            if ambiguous {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession has an ambiguous in-flight RuntimeCommand; reconciliation is required",
                    "runtime_command",
                    &command.id,
                    None,
                ));
            }
        }
        let record = RuntimeCommandRecord {
            id: command.id.clone(),
            execution_space_id: command.execution_space_id.clone(),
            target_node_id: command.target_node_id.clone(),
            target_node_daemon_id: command.target_node_daemon_id.clone(),
            target_node_daemon_generation: command.target_node_daemon_generation,
            authenticated_actor: command.authenticated_actor.clone(),
            command: command.command,
            required_capability: command.required_capability.clone(),
            idempotency_key: command.idempotency_key.clone(),
            request_fingerprint: command_fingerprint,
            status: RuntimeCommandStatus::Accepted,
            phase: RuntimeCommandPhase::Prepared,
            effect_certainty: RuntimeEffectCertainty::Unknown,
            postcondition_status: RuntimePostconditionStatus::Unknown,
            binding: command.binding.clone(),
            precondition: command.precondition.clone(),
            postcondition: command.postcondition.clone(),
            target_session_id,
            target_session_generation,
            source_record_id: command.payload["delivery_id"].as_str().map(str::to_string),
            result: None,
            failure_code: None,
            version: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };
        self.commit_trust_projection_unlocked(
            context,
            "runtime_command",
            &record.id,
            "accepted",
            serde_json::to_value(command)?,
            &record,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Resolve an Unknown provider effect without blindly repeating it. The
    /// exact current machine Operator asks the current NodeDaemon to record an
    /// evidence-backed certainty decision for one immutable command/session
    /// generation. Exact replay returns the original decision; changed
    /// semantics under the same key conflict before mutable-state checks.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_runtime_command_recovery(
        &self,
        context: &MutationContext,
        command_id: &str,
        node_id: &str,
        daemon_id: &str,
        daemon_generation: u64,
        resolution: RuntimeRecoveryResolution,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(evidence_ref, "RuntimeCommand recovery evidence_ref")?;
        let fingerprint = canonical_json_fingerprint(&serde_json::json!({
            "transport_request_fingerprint": context.request_fingerprint,
            "command_id": command_id,
            "node_id": node_id,
            "daemon_id": daemon_id,
            "daemon_generation": daemon_generation,
            "resolution": resolution,
            "evidence_ref": evidence_ref,
        }));
        let existing = self.trust_operation_envelopes_unlocked()?;
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "runtime_command"
                || replay.operation.event.aggregate_id != command_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "RuntimeCommand recovery key was reused with different semantics",
                    "runtime_command",
                    command_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            node_id,
            daemon_id,
            daemon_generation,
            &context.authenticated_actor,
            "runtime_command",
            command_id,
        )?;
        if context.authority_actor.as_ref()
            != Some(&ActorRef {
                kind: ActorKind::Service,
                id: node_id.to_string(),
            })
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "RuntimeCommand recovery requires the exact Execution Node Operator",
                "runtime_command",
                command_id,
                None,
            ));
        }
        let mut record = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "runtime_command")?
            .remove(command_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "RuntimeCommand recovery target does not exist",
                    "runtime_command",
                    command_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<RuntimeCommandRecord>(&envelope))?;
        if record.target_node_id != node_id || context.expected_version != record.version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "RuntimeCommand recovery requires the exact command Node and revision",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        if record.status != RuntimeCommandStatus::RecoveryRequired
            || record.effect_certainty != RuntimeEffectCertainty::Unknown
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only an Unknown RecoveryRequired RuntimeCommand can be resolved",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        match resolution {
            RuntimeRecoveryResolution::ConfirmApplied => {
                if runtime_command_requires_exact_binding(record.command) {
                    let session_id = record.target_session_id.as_deref().ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "provider-facing RuntimeCommand recovery has no exact target AgentSession",
                            "runtime_command",
                            command_id,
                            Some(record.version),
                        )
                    })?;
                    let session = self
                        .fabric_agent_sessions(&context.execution_space_id)?
                        .into_iter()
                        .find(|session| session.id == session_id)
                        .ok_or_else(|| {
                            trust_error(
                                TrustErrorCode::MemberRunGenerationFenced,
                                "RuntimeCommand target AgentSession disappeared before recovery resolution",
                                "runtime_command",
                                command_id,
                                Some(record.version),
                            )
                        })?;
                    if record.target_session_generation != Some(session.runtime_generation)
                        || session.node_id != record.target_node_id
                        || session.node_daemon_id != record.target_node_daemon_id
                        || session.node_daemon_generation != record.target_node_daemon_generation
                    {
                        return Err(trust_error(
                            TrustErrorCode::MemberRunGenerationFenced,
                            "RuntimeCommand recovery no longer owns the exact AgentSession/NodeDaemon generation",
                            "runtime_command",
                            command_id,
                            Some(record.version),
                        ));
                    }
                    self.require_live_runtime_binding_unlocked(
                        &session,
                        &record.binding,
                        matches!(
                            record.command,
                            RuntimeCommandKind::StartSession | RuntimeCommandKind::OpenRuntime
                        ),
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )?;
                    Self::require_runtime_command_precondition_unlocked(
                        &session,
                        record.command,
                        &record.precondition,
                        true,
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )?;
                }
                record.status = RuntimeCommandStatus::Applied;
                record.phase = RuntimeCommandPhase::Settled;
                record.effect_certainty = RuntimeEffectCertainty::Applied;
                record.postcondition_status = RuntimePostconditionStatus::Unknown;
                record.failure_code = None;
            }
            RuntimeRecoveryResolution::ConfirmNotApplied => {
                record.status = RuntimeCommandStatus::Failed;
                record.phase = RuntimeCommandPhase::Rejected;
                record.effect_certainty = RuntimeEffectCertainty::NotApplied;
                record.postcondition_status = RuntimePostconditionStatus::Unsatisfied;
                record.failure_code = Some("RECOVERY_CONFIRMED_NOT_APPLIED".into());
            }
            RuntimeRecoveryResolution::KeepRecoveryRequired => {
                record.phase = RuntimeCommandPhase::RecoveryRequired;
                record.failure_code = Some("RECOVERY_EVIDENCE_INSUFFICIENT".into());
            }
        }
        record.result = Some(serde_json::json!({
            "resolution": resolution,
            "evidence_ref": evidence_ref,
            "blind_replay": false,
        }));
        record.version += 1;
        record.updated_at = updated_at.to_string();
        let aggregate_version = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "runtime_command"
                    && envelope.operation.event.aggregate_id == command_id
            })
            .map(|envelope| envelope.operation.event.resulting_version)
            .max()
            .unwrap_or(0);
        let mut commit_context = context.clone();
        commit_context.expected_version = aggregate_version;
        commit_context.request_fingerprint = Some(fingerprint);
        self.commit_trust_projection_unlocked(
            &commit_context,
            "runtime_command",
            command_id,
            "recovery_resolved",
            serde_json::json!({
                "resolution": resolution,
                "evidence_ref": evidence_ref,
                "daemon_generation": daemon_generation,
            }),
            &record,
            Vec::new(),
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle_runtime_command(
        &self,
        context: &MutationContext,
        command_id: &str,
        status: RuntimeCommandStatus,
        effect_certainty: RuntimeEffectCertainty,
        result: Option<Value>,
        failure_code: Option<String>,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.settle_runtime_command_with_postcondition(
            context,
            command_id,
            status,
            effect_certainty,
            RuntimePostconditionStatus::Unknown,
            result,
            failure_code,
            now,
        )
    }

    /// Settle a provider effect and, when the adapter has separately observed
    /// it, the semantic postcondition requested by the durable command.
    /// Keeping this explicit prevents a transport ACK from being silently
    /// promoted to proof of quiescence, release, or cycle termination.
    #[allow(clippy::too_many_arguments)]
    pub fn settle_runtime_command_with_postcondition(
        &self,
        context: &MutationContext,
        command_id: &str,
        status: RuntimeCommandStatus,
        effect_certainty: RuntimeEffectCertainty,
        postcondition_status: RuntimePostconditionStatus,
        result: Option<Value>,
        failure_code: Option<String>,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut record = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "runtime_command")?
            .remove(command_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "RuntimeCommand was not durably accepted",
                    "runtime_command",
                    command_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<RuntimeCommandRecord>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &record.target_node_id,
            &record.target_node_daemon_id,
            record.target_node_daemon_generation,
            &context.authenticated_actor,
            "runtime_command",
            command_id,
        )?;
        if runtime_command_requires_exact_binding(record.command) {
            let session_id = record.target_session_id.as_deref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "provider-facing RuntimeCommand has no exact target AgentSession binding",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                )
            })?;
            let session_generation = record.target_session_generation.ok_or_else(|| {
                trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "provider-facing RuntimeCommand has no exact target runtime generation",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                )
            })?;
            let session = self
                .fabric_agent_sessions(&context.execution_space_id)?
                .into_iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "RuntimeCommand target AgentSession disappeared before settlement",
                        "runtime_command",
                        command_id,
                        Some(record.version),
                    )
                })?;
            if session.runtime_generation != session_generation
                || session.node_id != record.target_node_id
                || session.node_daemon_id != record.target_node_daemon_id
                || session.node_daemon_generation != record.target_node_daemon_generation
            {
                return Err(trust_error(
                    TrustErrorCode::MemberRunGenerationFenced,
                    "RuntimeCommand settlement no longer owns the exact AgentSession/NodeDaemon generation",
                    "runtime_command",
                    command_id,
                    Some(record.version),
                ));
            }
            self.require_live_runtime_binding_unlocked(
                &session,
                &record.binding,
                matches!(
                    record.command,
                    RuntimeCommandKind::StartSession | RuntimeCommandKind::OpenRuntime
                ),
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
            Self::require_runtime_command_precondition_unlocked(
                &session,
                record.command,
                &record.precondition,
                true,
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
        }
        if record.target_node_daemon_id != context.authenticated_actor.id
            || context.authenticated_actor.kind != ActorKind::Service
            || !matches!(
                record.status,
                RuntimeCommandStatus::Accepted
                    | RuntimeCommandStatus::Quiesced
                    | RuntimeCommandStatus::RecoveryRequired
            )
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "only the exact target NodeDaemon can settle an admitted RuntimeCommand",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        if !matches!(
            status,
            RuntimeCommandStatus::Applied
                | RuntimeCommandStatus::Failed
                | RuntimeCommandStatus::RecoveryRequired
                | RuntimeCommandStatus::Quiesced
        ) {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "invalid RuntimeCommand settlement",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        let postcondition_combination_is_valid = match postcondition_status {
            RuntimePostconditionStatus::Satisfied => {
                status == RuntimeCommandStatus::Applied
                    && effect_certainty == RuntimeEffectCertainty::Applied
                    && result.is_some()
            }
            RuntimePostconditionStatus::Unsatisfied => {
                status == RuntimeCommandStatus::Failed
                    && effect_certainty == RuntimeEffectCertainty::NotApplied
            }
            RuntimePostconditionStatus::Unknown => true,
        };
        if !postcondition_combination_is_valid {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "RuntimeCommand postcondition status is not proven by this settlement",
                "runtime_command",
                command_id,
                Some(record.version),
            ));
        }
        record.status = status;
        record.phase = match status {
            RuntimeCommandStatus::Applied => RuntimeCommandPhase::Settled,
            RuntimeCommandStatus::Failed => RuntimeCommandPhase::Rejected,
            RuntimeCommandStatus::Quiesced => RuntimeCommandPhase::Observed,
            RuntimeCommandStatus::RecoveryRequired => RuntimeCommandPhase::RecoveryRequired,
            RuntimeCommandStatus::Requested | RuntimeCommandStatus::Accepted => {
                RuntimeCommandPhase::Prepared
            }
        };
        record.effect_certainty = effect_certainty;
        record.postcondition_status = postcondition_status;
        record.result = result;
        record.failure_code = failure_code;
        record.version += 1;
        record.updated_at = now.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "runtime_command",
            command_id,
            "settled",
            serde_json::json!({
                "status": status,
                "effect_certainty": effect_certainty,
                "result": record.result,
                "failure_code": record.failure_code,
            }),
            &record,
            Vec::new(),
            Vec::new(),
        )
    }
}
