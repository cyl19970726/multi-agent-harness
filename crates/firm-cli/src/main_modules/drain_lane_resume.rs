//! Returning a lane killed by a NodeDaemon drain to the ordinary lane, and
//! keeping the refusal that guards it attempt-scoped (#779).
//!
//! DEV-171 (#748) admitted exactly one exit from `Interrupted`: back to `Idle`,
//! and only while the killed runtime is still provably gone — detached,
//! disarmed, at a terminal turn boundary, with no ambiguous RuntimeCommand.
//! The fence is correct and is not weakened here. What was wrong is *when* the
//! successor generation used it.
//!
//! The member runner opens its provider handle first and publishes
//! `RuntimeResidency::Attached` before the first cycle projects the Session
//! `Active` ([`crate::run_codex_member_shared`] and its siblings). On a drained
//! member that ordering is self-defeating: the lane is `Interrupted`, the
//! runner has just made it non-`Detached`, and the cycle's own
//! `Interrupted -> Idle -> Active` projection is then refused by the very fence
//! that exists to prove the dead runtime is gone. The refusal is about the
//! attempt's ordering, never about the member — so this module does two things:
//!
//! 1. resumes the lane at the adoption seam, before any provider effect is
//!    prepared and while the drain settlement's own evidence still proves the
//!    lane dead ([`resume_drained_lane_for_adoption`]); and
//! 2. classifies the refusal as attempt-scoped if a runner still meets it, so
//!    the member stays startable for the next Supervisor pass instead of being
//!    journalled `Blocked` — which the successor generation reads as operator
//!    lifecycle control and never retries.

use super::*;

use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionStatus, DriverHandoffState, MutationContext,
    NativeContinuationActivation, RuntimeActivity, RuntimeCommandStatus, RuntimeEffectCertainty,
    RuntimeResidency, TrustErrorCode, AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE,
};

/// Whether this exact error is the drain fence saying the lane is not
/// resumable *yet*.
///
/// Typed first, and never a substring of the whole chain: the leading
/// `CliError` variant must carry a `TrustError` whose code, resource kind and
/// message are all the fence's own. A refusal that merely quotes the fence
/// while reporting something structural must still hold.
pub(super) fn is_drain_resume_not_yet_resumable(error: &CliError) -> bool {
    let CliError::Store(store_error) = error else {
        return false;
    };
    store_error.trust_error().is_some_and(|trust_error| {
        trust_error.code == TrustErrorCode::InvalidStateTransition
            && trust_error.resource_kind == "agent_session"
            && trust_error.message == AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE
    })
}

/// Whether a lane still proves the runtime that owned it is gone.
///
/// This is the Store fence's own predicate, read from outside the writer lock
/// so a caller can decide *whether to try* without guessing. It never grants
/// the transition: the Store re-proves all of it under its lock.
pub(super) fn lane_proves_runtime_is_terminated(
    store: &HarnessStore,
    execution_space_id: &str,
    session: &AgentSession,
) -> CliResult<bool> {
    if session.control_state.runtime_residency != RuntimeResidency::Detached
        || session.control_state.activity != RuntimeActivity::Idle
        || session.control_state.handoff_state != DriverHandoffState::None
        || session.control_state.continuation.activation != NativeContinuationActivation::Disarmed
        || session.current_turn_id.is_some()
        || session.queued_input_count != 0
    {
        return Ok(false);
    }
    Ok(!store
        .runtime_commands(execution_space_id)?
        .into_iter()
        .any(|command| {
            command.target_session_id.as_deref() == Some(session.id.as_str())
                && command.target_session_generation == Some(session.runtime_generation)
                && matches!(
                    command.status,
                    RuntimeCommandStatus::Accepted
                        | RuntimeCommandStatus::Quiesced
                        | RuntimeCommandStatus::RecoveryRequired
                )
                && command.effect_certainty == RuntimeEffectCertainty::Unknown
        }))
}

/// Whether this member's one current AgentSession proves no runtime can be
/// driving it right now.
///
/// Fail closed on every uncertainty: no current session, more than one, a
/// lifecycle that still claims a live or closing lane, or an unreadable Store
/// all answer `false`. This is a read-only Host-side proof used to decide
/// whether a coordination-only correction is admissible; it grants nothing by
/// itself and every durable write still passes the Store's own fences.
pub(super) fn member_lane_proves_runtime_gone(
    store: &HarnessStore,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> bool {
    let Ok(sessions) = store.fabric_agent_sessions(execution_space_id) else {
        return false;
    };
    let mut current = sessions.into_iter().filter(|session| {
        session.agent_member_id == member.agent_member_id
            && session.lifecycle != AgentSessionStatus::Closed
    });
    let Some(session) = current.next() else {
        return false;
    };
    if current.next().is_some() {
        return false;
    }
    if !matches!(
        session.lifecycle,
        AgentSessionStatus::Cold | AgentSessionStatus::Idle | AgentSessionStatus::Interrupted
    ) {
        return false;
    }
    lane_proves_runtime_is_terminated(store, execution_space_id, &session).unwrap_or(false)
}

/// Re-read one AgentSession by id after a write that bumped its version.
pub(super) fn current_agent_session(
    store: &HarnessStore,
    execution_space_id: &str,
    session_id: &str,
) -> CliResult<AgentSession> {
    store
        .fabric_agent_sessions(execution_space_id)?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "AGENT_SESSION_RECOVERY_REQUIRED: AgentSession {session_id} is no longer current in Execution Space {execution_space_id}"
            ))
        })
}

/// What the adoption seam did about one drained lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DrainedLaneResume {
    /// The lane was not left `Interrupted` by a drain. Nothing was written.
    NotDrained,
    /// The lane re-entered the ordinary lane as `Idle` under this generation.
    Resumed,
    /// The lane is still `Interrupted` because it does not yet prove the killed
    /// runtime is gone. Nothing was written; a later pass retries.
    NotYetResumable,
}

/// Return one drained lane to `Idle` at the adoption seam.
///
/// This is the only correct moment for the hop. The drain settlement already
/// detached the lane, disarmed its continuation, cleared its turn and settled
/// every RuntimeCommand of the dead generation, and
/// `reattach_agent_session_to_node_daemon` has just moved it onto this live
/// NodeDaemon generation — so every clause of the DEV-171 fence holds and is
/// re-proved by the Store under its own lock. Nothing has been spawned yet, so
/// no provider handle can be attached, no cycle can be open, and no killed
/// cycle can be replayed: the successor generation simply opens a fresh cycle
/// on the same provider-native session, which is exactly what ADR 0032
/// requires of resume.
///
/// A lane that does not yet prove its runtime dead is left `Interrupted`
/// untouched. That is an attempt-scoped observation, not a verdict about the
/// member, so it never fails adoption.
pub(super) fn resume_drained_lane_for_adoption(
    store: &HarnessStore,
    execution_space_id: &str,
    daemon_id: &str,
    session: &AgentSession,
    timestamp: &str,
) -> CliResult<DrainedLaneResume> {
    if session.lifecycle != AgentSessionStatus::Interrupted {
        return Ok(DrainedLaneResume::NotDrained);
    }
    if !lane_proves_runtime_is_terminated(store, execution_space_id, session)? {
        return Ok(DrainedLaneResume::NotYetResumable);
    }
    let context = MutationContext {
        execution_space_id: execution_space_id.to_string(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon_id.to_string(),
        },
        authority_actor: None,
        command_name: "node_daemon.agent_session.resume_after_drain".into(),
        idempotency_key: format!(
            "session-drain-resume:{}:{}:{}",
            session.id, session.node_daemon_generation, session.version
        ),
        expected_version: session.version,
        request_fingerprint: None,
    };
    match store.transition_agent_session(&context, &session.id, AgentSessionStatus::Idle, timestamp)
    {
        Ok(_) => Ok(DrainedLaneResume::Resumed),
        Err(error) => {
            let error = CliError::Store(error);
            // The lane changed under us, or the Store's own re-proof disagrees
            // with the read above. Either way this attempt learned nothing
            // durable about the member; the next pass observes the lane again.
            if is_drain_resume_not_yet_resumable(&error) {
                Ok(DrainedLaneResume::NotYetResumable)
            } else {
                Err(error)
            }
        }
    }
}

/// Whether a failed provider attempt is only the drain fence refusing the
/// resume hop, with the lane already back where the next pass can retry.
///
/// The two live symptoms this covers are the raw `INVALID_STATE_TRANSITION`
/// from the `Interrupted -> Idle` hop and the same refusal reported through the
/// runner's `PROVIDER_EFFECT_ACCEPTED_NO_REPLAY` summary: the resume
/// RuntimeCommand *was* applied, but what failed afterwards is a canonical
/// projection, not a provider effect that might have to be replayed. Nothing is
/// replayed either way — the member is left startable and the Supervisor opens
/// a fresh attempt once the lane is `Idle`.
///
/// Fail closed: the lane must be readable and must currently prove the killed
/// runtime gone. A lane still holding an ambiguous RuntimeCommand, an attached
/// handle or an open turn keeps the ordinary `Blocked` diagnosis.
pub(super) fn provider_failure_awaits_drain_lane_resume(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    error: &CliError,
) -> bool {
    if !is_drain_resume_not_yet_resumable(error) {
        return false;
    }
    let Ok((execution_space_id, session)) = provider_session_for_member(ledger, member) else {
        return false;
    };
    if !matches!(
        session.lifecycle,
        AgentSessionStatus::Idle | AgentSessionStatus::Interrupted
    ) {
        return false;
    }
    lane_proves_runtime_is_terminated(&ledger.store, &execution_space_id, &session).unwrap_or(false)
}

/// Journal a member whose provider attempt met only the drain fence.
///
/// The caller has already journalled the ordinary post-bind disconnect, so this
/// only records *why* the attempt ended and guarantees the row the successor
/// generation reads is startable. `Disconnected` is the exact existing meaning:
/// the durable MemberRun and its native-session binding stand, and this
/// Supervisor has no healthy provider transport right now. It is one of the
/// three statuses [`crate::claim_member_provider_start`] will start, so the next
/// pass retries on its own — no Host verb, and no `Blocked` row for a successor
/// generation to misread as lifecycle control.
pub(super) fn journal_member_awaiting_drain_lane_resume(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    transport_attempt: u64,
    reason: &str,
) -> MemberOutcome {
    let summary = format!(
        "DRAIN_LANE_RESUME_PENDING: attempt {transport_attempt} met the NodeDaemon drain fence before the lane was resumable; the member stays startable and the next Supervisor pass retries once its AgentSession is Idle; refusal: {reason}"
    );
    let mut latest = ledger
        .latest_member_run(&member.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| member.clone());
    // Only a row this attempt itself left non-startable is corrected. A Close,
    // a Stop or any other lifecycle decision that landed meanwhile is
    // authoritative and is never overwritten by a retry note.
    if latest.coordination_is_active()
        && !matches!(
            latest.status,
            MemberRunStatus::Queued | MemberRunStatus::Idle | MemberRunStatus::Disconnected
        )
    {
        let expected = latest.clone();
        latest.status = MemberRunStatus::Disconnected;
        latest.finished_at = None;
        latest.last_event_at = Some(now_string());
        if ledger.save_member_run(&expected, &latest).is_err() {
            latest = expected;
        }
    }
    let _ = ledger.append_action(
        &latest.id,
        "drain_lane_resume_pending",
        MemberActionStatus::Progress,
        "drained AgentSession was not resumable yet; the member stays startable",
        &summary,
    );
    let _ = ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(latest.id.clone()),
        "member_run",
        &latest.id,
        "drain_lane_resume_pending",
        &summary,
    );
    MemberOutcome::new(&latest, latest.status, summary)
}

/// Journal the terminal `Blocked` verdict for a provider attempt this
/// generation may not retry.
///
/// Lifted out of the member runner unchanged so the retry classifier above and
/// the verdict it guards read as one decision, and so a test can drive the
/// exact production write rather than a copy of it. The three summaries stay
/// distinct because they demand different operator action: an accepted effect
/// that must not be replayed, a rejected admission that needs new intent, and
/// exhausted transport retries.
pub(super) fn journal_provider_attempt_exhausted_block(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    error: &CliError,
    durable_process_outcome: &harness_application::ProviderEffectOutcome,
    transport_attempt: u64,
    reason: &str,
) -> MemberOutcome {
    let mut exhausted = ledger
        .latest_member_run(&member.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| member.clone());
    let expected = exhausted.clone();
    exhausted.status = MemberRunStatus::Blocked;
    exhausted.finished_at = None;
    exhausted.last_event_at = Some(now_string());
    let summary = if matches!(
        durable_process_outcome,
        harness_application::ProviderEffectOutcome::Accepted { .. }
    ) {
        format!(
            "PROVIDER_EFFECT_ACCEPTED_NO_REPLAY: the provider process effect was already accepted; explicit Host reconciliation is required; later error: {reason}"
        )
    } else if matches!(error, CliError::ProviderAdmissionRejected(_)) {
        format!(
            "PROVIDER_ADMISSION_REJECTED_NO_EFFECT: explicit Host correction or new intent is required; {reason}"
        )
    } else {
        format!(
            "PROVIDER_TRANSPORT_RETRY_EXHAUSTED: {transport_attempt} automatic attempts failed; explicit Host reconciliation is required; last error: {reason}"
        )
    };
    if ledger.save_member_run(&expected, &exhausted).is_ok() {
        let _ = ledger.append_action(
            &exhausted.id,
            "runtime_recovery_required",
            MemberActionStatus::Failed,
            "provider transport retries exhausted",
            &summary,
        );
        let _ = ledger.fold_event(
            TeamRunEventSourceKind::Member,
            Some(exhausted.id.clone()),
            "member_run",
            &exhausted.id,
            "recovery_required",
            &summary,
        );
    }
    MemberOutcome::new(&exhausted, MemberRunStatus::Blocked, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(trust_error: &harness_core::agentfirm_api::TrustError) -> CliError {
        CliError::Store(harness_store::StoreError::Conflict(
            serde_json::to_string(trust_error).expect("TrustError serializes"),
        ))
    }

    fn fence_error() -> harness_core::agentfirm_api::TrustError {
        harness_core::agentfirm_api::TrustError {
            code: TrustErrorCode::InvalidStateTransition,
            message: AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE.to_string(),
            retryable: false,
            resource_kind: "agent_session".into(),
            resource_id: "agent-session:1".into(),
            current_version: Some(7),
        }
    }

    #[test]
    fn only_the_exact_drain_fence_refusal_is_attempt_scoped() {
        assert!(is_drain_resume_not_yet_resumable(&conflict(&fence_error())));

        let mut other_resource = fence_error();
        other_resource.resource_kind = "runtime_command".into();
        assert!(
            !is_drain_resume_not_yet_resumable(&conflict(&other_resource)),
            "the fence is an AgentSession refusal, never any resource that quotes it"
        );

        let mut other_code = fence_error();
        other_code.code = TrustErrorCode::RuntimeEffectUnknown;
        assert!(
            !is_drain_resume_not_yet_resumable(&conflict(&other_code)),
            "an uncertain provider effect stays the stronger diagnosis"
        );

        let mut other_transition = fence_error();
        other_transition.message = "invalid AgentSession transition Idle->Interrupted".into();
        assert!(
            !is_drain_resume_not_yet_resumable(&conflict(&other_transition)),
            "an ordinary invalid transition is not the drain fence"
        );

        assert!(
            !is_drain_resume_not_yet_resumable(&CliError::Usage(format!(
                "INVALID_STATE_TRANSITION: {AGENT_SESSION_DRAIN_RESUME_NOT_YET_RESUMABLE}"
            ))),
            "classification is typed first; a flattened message is not the fence"
        );
    }
}
