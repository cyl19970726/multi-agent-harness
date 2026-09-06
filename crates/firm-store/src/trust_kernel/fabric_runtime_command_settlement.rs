use super::fabric_foundation::{RuntimeBindingAdmission, RuntimeCommandPoststate};
use super::*;

impl HarnessStore {
    #[allow(clippy::too_many_arguments)]
    pub fn settle_runtime_command(
        &self,
        context: &MutationContext,
        command_id: &str,
        phase: RuntimeCommandPhase,
        effect_certainty: RuntimeEffectCertainty,
        result: Option<Value>,
        failure_code: Option<String>,
        now: &str,
    ) -> StoreResult<CanonicalMutationResult<RuntimeCommandRecord>> {
        self.settle_runtime_command_with_postcondition(
            context,
            command_id,
            phase,
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
        phase: RuntimeCommandPhase,
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
        self.require_node_daemon_settlement_authority_unlocked(
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
                RuntimeBindingAdmission::RuntimeCommand {
                    // Settlement records the outcome of an already admitted
                    // command. A command prepared before the native session id
                    // was known must still settle after the id attaches to the
                    // same exact session/runtime/driver generation, whatever its
                    // kind (GitHub #583: an Interrupt fenced by the bind race left
                    // an ambiguous command). Replacement stays fenced.
                    allow_native_session_attachment: true,
                    settlement_only: true,
                },
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
            Self::require_runtime_command_precondition_unlocked(
                &session,
                record.command,
                &record.precondition,
                // Fires under exactly the condition the admission above
                // tolerated, whatever the command kind (GitHub #583).
                if record.binding.native_session_ref.is_none()
                    && session.native_session_ref.is_some()
                {
                    RuntimeCommandPoststate::CommandWithNativeSessionAttachment
                } else {
                    RuntimeCommandPoststate::Command
                },
                "runtime_command",
                command_id,
                Some(record.version),
            )?;
        }
        if record.target_node_daemon_id != context.authenticated_actor.id
            || context.authenticated_actor.kind != ActorKind::Service
            || !matches!(
                record.phase,
                RuntimeCommandPhase::Prepared
                    | RuntimeCommandPhase::Observed
                    | RuntimeCommandPhase::RecoveryRequired
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
            phase,
            RuntimeCommandPhase::Settled
                | RuntimeCommandPhase::Rejected
                | RuntimeCommandPhase::RecoveryRequired
                | RuntimeCommandPhase::Observed
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
                phase == RuntimeCommandPhase::Settled
                    && effect_certainty == RuntimeEffectCertainty::Applied
                    && result.is_some()
            }
            RuntimePostconditionStatus::Unsatisfied => {
                phase == RuntimeCommandPhase::Rejected
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
        record.phase = phase;
        record.effect_certainty = effect_certainty;
        record.postcondition_status = postcondition_status;
        record.result = result;
        record.failure_code = failure_code;
        record.version += 1;
        record.updated_at = now.to_string();
        let payload = serde_json::json!({
            "phase": phase,
            "effect_certainty": effect_certainty,
            "result": record.result,
            "failure_code": record.failure_code,
        });
        let context = self.legacy_settlement_replay_context(context, &record, &payload)?;
        self.commit_trust_projection_unlocked(
            &context,
            "runtime_command",
            command_id,
            "settled",
            payload,
            &record,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Recognize an old request spelling only after settlement authority checks.
    /// Replay preserves the original event and fingerprint, never rewriting rows.
    fn legacy_settlement_replay_context(
        &self,
        context: &MutationContext,
        record: &RuntimeCommandRecord,
        payload: &Value,
    ) -> StoreResult<MutationContext> {
        let mut compatible = context.clone();
        if context.request_fingerprint.is_some() {
            return Ok(compatible);
        }
        let legacy_status = match record.phase {
            RuntimeCommandPhase::Settled => "applied",
            RuntimeCommandPhase::Rejected => "failed",
            RuntimeCommandPhase::Observed => "quiesced",
            RuntimeCommandPhase::RecoveryRequired => "recovery_required",
            _ => return Ok(compatible),
        };
        let mut legacy_payload = payload.clone();
        let object = legacy_payload.as_object_mut().expect("settlement object");
        object.remove("phase");
        object.insert("status".into(), Value::String(legacy_status.into()));
        let legacy_fingerprint = canonical_json_fingerprint(&legacy_payload);
        for envelope in self.trust_operation_envelopes_unlocked()? {
            let event = &envelope.operation.event;
            if envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && event.idempotency_key == context.idempotency_key
                && event.aggregate_kind == "runtime_command"
                && event.aggregate_id == record.id
                && event.transition == "settled"
                && event.payload == legacy_payload
                && event.canonical_request_fingerprint == legacy_fingerprint
            {
                let historical: RuntimeCommandRecord = event_projection(&envelope)?;
                if historical.phase == record.phase
                    && historical.postcondition_status == record.postcondition_status
                {
                    compatible.request_fingerprint = Some(legacy_fingerprint);
                }
                break;
            }
        }
        Ok(compatible)
    }
}
