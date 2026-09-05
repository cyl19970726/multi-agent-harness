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
    AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE,
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

/// Whether this exact error is the `RecoveryRequired` fence saying the lane
/// is not resumable *yet* (typed first, like the drain fence above).
pub(super) fn is_recovery_required_not_yet_resumable(error: &CliError) -> bool {
    let CliError::Store(store_error) = error else {
        return false;
    };
    store_error.trust_error().is_some_and(|trust_error| {
        trust_error.code == TrustErrorCode::InvalidStateTransition
            && trust_error.resource_kind == "agent_session"
            && trust_error.message == AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE
    })
}

/// The terminated-lane proof as one reading: the first clause the lane fails,
/// plus the dormant residue a caller chose to tolerate.
pub(super) struct LaneTerminationProof {
    /// The first failing clause, or `None` when the lane proves its runtime
    /// gone (modulo the tolerated residue).
    pub blocker: Option<String>,
    /// Residue that nothing will ever consume and the caller accepted instead
    /// of refusing: an armed native continuation or queued native input on a
    /// lane whose TeamRun is Completed. Always empty unless tolerated.
    pub dormant_residue: Vec<String>,
}

/// The terminated-lane proof. This is the Store fence's own predicate
/// (residency, activity, handoff, continuation, turn, queued input, ambiguous
/// RuntimeCommand), read from outside the writer lock so a caller can decide
/// *whether to try* and can say *why not*. It never grants a transition: the
/// Store re-proves all of it under its lock. Every reader of the proof
/// derives from this one function, so the reason named and the decision
/// taken cannot drift apart (GitHub #841).
///
/// `tolerate_dormant_continuation` is for the coordination Close of a Completed
/// TeamRun's member (#812): an armed native continuation on that lane will
/// never be driven, so refusing the Close on it would strand the member
/// forever; the residue is recorded on the Close receipt instead. A driver
/// handoff, an open turn, queued input, or an ambiguous command is never
/// tolerated.
pub(super) fn lane_termination_proof(
    store: &HarnessStore,
    execution_space_id: &str,
    session: &AgentSession,
    tolerate_dormant_continuation: bool,
) -> CliResult<LaneTerminationProof> {
    let mut dormant_residue = Vec::new();
    let blocked = |blocker: String| {
        Ok(LaneTerminationProof {
            blocker: Some(blocker),
            dormant_residue: Vec::new(),
        })
    };
    if session.control_state.runtime_residency != RuntimeResidency::Detached {
        return blocked(format!(
            "AgentSession {} still holds an attached runtime handle",
            session.id
        ));
    }
    if session.control_state.activity != RuntimeActivity::Idle {
        return blocked(format!(
            "AgentSession {} runtime activity is {:?}, not idle",
            session.id, session.control_state.activity
        ));
    }
    if session.control_state.handoff_state != DriverHandoffState::None {
        return blocked(format!(
            "AgentSession {} is mid driver handoff ({:?})",
            session.id, session.control_state.handoff_state
        ));
    }
    if session.control_state.continuation.activation != NativeContinuationActivation::Disarmed {
        let residue = format!(
            "AgentSession {} still has an armed native continuation",
            session.id
        );
        if tolerate_dormant_continuation {
            // A record, not a latch: the next Supervisor bind at adoption sets
            // the activation back to Disarmed before any reopened cycle
            // (`member_orchestration.rs`, the driver bind), and a detached
            // lane has no process that could drive the continuation meanwhile.
            dormant_residue.push(residue);
        } else {
            return blocked(residue);
        }
    }
    if let Some(turn) = session.current_turn_id.as_deref() {
        return blocked(format!(
            "AgentSession {} still has an open turn {turn}",
            session.id
        ));
    }
    // Nothing in the tree increments `queued_input_count` today (it is only
    // reset and decremented), so this clause is a fail-closed guard for a
    // future writer, never a tolerated residue.
    if session.queued_input_count != 0 {
        return blocked(format!(
            "AgentSession {} still has {} queued native input(s)",
            session.id, session.queued_input_count
        ));
    }
    let ambiguous = store
        .runtime_commands(execution_space_id)?
        .into_iter()
        .find(|command| {
            command.target_session_id.as_deref() == Some(session.id.as_str())
                && command.target_session_generation == Some(session.runtime_generation)
                && matches!(
                    command.status,
                    RuntimeCommandStatus::Accepted
                        | RuntimeCommandStatus::Quiesced
                        | RuntimeCommandStatus::RecoveryRequired
                )
                && command.effect_certainty == RuntimeEffectCertainty::Unknown
        })
        .map(|command| {
            format!(
                "ambiguous RuntimeCommand {} ({:?}) still has an unknown provider effect; reconcile it first",
                command.id, command.command
            )
        });
    Ok(LaneTerminationProof {
        blocker: ambiguous,
        dormant_residue,
    })
}

/// The first clause of the terminated-lane proof this lane fails, or `None`
/// when the lane proves the runtime that owned it is gone (nothing tolerated).
pub(super) fn lane_termination_blocker(
    store: &HarnessStore,
    execution_space_id: &str,
    session: &AgentSession,
) -> CliResult<Option<String>> {
    Ok(lane_termination_proof(store, execution_space_id, session, false)?.blocker)
}

/// Whether a lane still proves the runtime that owned it is gone.
pub(super) fn lane_proves_runtime_is_terminated(
    store: &HarnessStore,
    execution_space_id: &str,
    session: &AgentSession,
) -> CliResult<bool> {
    Ok(lane_termination_blocker(store, execution_space_id, session)?.is_none())
}

/// The one definition of "this lane sits at a terminal turn boundary with no
/// cycle open" shared by `team-run recover` (may a coordination-only repair
/// touch it?) and the detached-recovery Close fence (may the Host close it?).
/// Both verbs must agree, or recover reports a lane as repairable that Close
/// then refuses (GitHub #841). `RecoveryRequired` belongs here since GitHub
/// #755: the Store admits its exit to `Idle` under the terminated-lane proof.
pub(super) fn lane_is_at_terminal_turn_boundary(session: &AgentSession) -> bool {
    session.is_at_terminal_turn_boundary()
}

/// Why this member's one current AgentSession does NOT prove that no runtime
/// can be driving it, or `None` when it does. Fail closed on every
/// uncertainty: no current session, more than one, a lifecycle that still
/// claims a live or closing lane, or an unreadable Store all name a blocker.
pub(super) fn member_lane_blocker(
    store: &HarnessStore,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> Option<String> {
    let sessions = match store.fabric_agent_sessions(execution_space_id) {
        Ok(sessions) => sessions,
        Err(error) => return Some(format!("the Execution Space could not be read: {error}")),
    };
    let mut current = sessions.into_iter().filter(|session| {
        session.agent_member_id == member.agent_member_id
            && session.lifecycle != AgentSessionStatus::Closed
    });
    let Some(session) = current.next() else {
        return Some("no current AgentSession".into());
    };
    if current.next().is_some() {
        return Some("more than one current AgentSession".into());
    }
    if !lane_is_at_terminal_turn_boundary(&session) {
        return Some(format!(
            "AgentSession {} is not at a terminal turn boundary (lifecycle {:?}, activity {:?}, turn {})",
            session.id,
            session.lifecycle,
            session.control_state.activity,
            session.current_turn_id.as_deref().unwrap_or("none")
        ));
    }
    match lane_termination_blocker(store, execution_space_id, &session) {
        Ok(blocker) => blocker,
        Err(error) => Some(format!("RuntimeCommands could not be read: {error}")),
    }
}

/// Whether this member's one current AgentSession proves no runtime can be
/// driving it right now. This is a read-only Host-side proof used to decide
/// whether a coordination-only correction is admissible; it grants nothing by
/// itself and every durable write still passes the Store's own fences.
pub(super) fn member_lane_proves_runtime_gone(
    store: &HarnessStore,
    execution_space_id: &str,
    member: &ProviderRuntimeProjection,
) -> bool {
    member_lane_blocker(store, execution_space_id, member).is_none()
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
    /// The lane was left neither `Interrupted` by a drain nor
    /// `RecoveryRequired` by a runner. Nothing was written.
    NotDrained,
    /// The lane re-entered the ordinary lane as `Idle` under this generation.
    Resumed,
    /// The lane keeps its lifecycle because it does not yet prove the killed
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
///
/// A lane a runner left `RecoveryRequired` (#755) takes the same hop once an
/// operator has reconciled it: the drain skipped it as already settled, the
/// successor reattached it, and the Store admits `RecoveryRequired -> Idle`
/// under the same clauses, refusing with
/// `AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE` otherwise.
pub(super) fn resume_drained_lane_for_adoption(
    store: &HarnessStore,
    execution_space_id: &str,
    daemon_id: &str,
    session: &AgentSession,
    timestamp: &str,
) -> CliResult<DrainedLaneResume> {
    if !matches!(
        session.lifecycle,
        AgentSessionStatus::Interrupted | AgentSessionStatus::RecoveryRequired
    ) {
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
            if is_drain_resume_not_yet_resumable(&error)
                || is_recovery_required_not_yet_resumable(&error)
            {
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
    // Only a row this attempt itself left non-startable is corrected, and only
    // while no operator lifecycle decision is standing over it: a durable Close
    // latch outranks a retry note, and the ordinary control path — not this
    // function — is what applies it. A closed or retired coordination status is
    // the same rule after the fact.
    let close_latched = pending_member_close(&ledger.store, &latest.id)
        .ok()
        .flatten()
        .is_some();
    if !close_latched
        && latest.coordination_is_active()
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

/// Why a `Blocked` MemberRun is blocked, as far as its own typed provenance
/// says.
///
/// `MemberRunStatus::Blocked` is written by four different authorities and the
/// row itself does not name which one. Three of them leave typed provenance
/// behind and own their own recovery; only the fourth — a provider attempt that
/// met a runtime fence — leaves none. A repair verb that reads the status alone
/// therefore cannot tell "the drained lane is startable again" from "this
/// provider is on an unreviewed version": the first is a coordination
/// correction, the others are live diagnoses whose clearing belongs to the gate
/// that wrote them, and whose evidence a bare status flip would strand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BlockedMemberProvenance {
    /// `store_team_admission` wrote a typed `provider_compatibility_block_cause`.
    /// `ProviderRuntimeProjection::validate` binds that cause to `Blocked`, so
    /// moving the status without clearing the cause produces a row the Store
    /// refuses outright — which would abort the whole recovery run.
    ProviderCompatibility,
    /// The capacity preflight blocked on a known-unavailable account. Only a
    /// successful capacity probe may clear it, through
    /// `recover_capacity_origin_block`, which itself keys on the `Blocked`
    /// status a flip would erase.
    ProviderCapacity,
    /// The wake loop stopped a zero-output spiral. Its evidence is the member's
    /// own `zero_output_streak`, and only once that streak has actually reached
    /// the degradation threshold: below it the streak is ordinary bounded
    /// probation, not a verdict.
    ZeroOutputDegradation,
    /// No typed provenance: the class a runtime fence leaves behind, including
    /// the NodeDaemon drain refusal this module exists for.
    Untyped,
}

impl BlockedMemberProvenance {
    /// Why this block must not be cleared by a coordination-only repair.
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::ProviderCompatibility => {
                "a typed provider-compatibility cause owns this block; clear it through the provider review gate"
            }
            Self::ProviderCapacity => {
                "the capacity preflight owns this block; it clears on a successful capacity probe"
            }
            Self::ZeroOutputDegradation => {
                "a zero-output degradation owns this block; the Host must intervene"
            }
            Self::Untyped => "no typed provenance owns this block",
        }
    }
}

/// Read a member's block provenance from the member row alone.
///
/// Pure and total: it answers for any member, blocked or not, and the caller
/// decides what the answer authorizes. Ordering is by strength — a member
/// carrying more than one piece of provenance keeps the strongest, because that
/// is the gate whose own recovery must clear it.
pub(super) fn blocked_member_provenance(
    member: &ProviderRuntimeProjection,
) -> BlockedMemberProvenance {
    if member.provider_compatibility_block_cause.is_some() {
        return BlockedMemberProvenance::ProviderCompatibility;
    }
    if member
        .provider_capacity
        .as_ref()
        .is_some_and(|capacity| capacity.state.is_known_unavailable())
    {
        return BlockedMemberProvenance::ProviderCapacity;
    }
    // The threshold, never a bare non-zero streak. `decide_wake` degrades only
    // at `>= zero_output_degradation_threshold` and deliberately keeps waking a
    // member below it (its probation predicate exists so the threshold can be
    // reached at all), so a streak of 1 or 2 is a normal working state. Reading
    // it as a degradation verdict would recreate this module's own bug for
    // exactly the members it exists to repair: a drain-fenced member with a
    // streak of 1 would be refused by recover, told a degradation owns its
    // block, refused by the start claim for being Blocked — and its streak
    // could never reset, because only a productive round resets it and the
    // member can never run. The value is read from the same policy the wake
    // loop uses so the two cannot drift.
    if member.zero_output_streak >= zero_output_degradation_threshold() {
        return BlockedMemberProvenance::ZeroOutputDegradation;
    }
    BlockedMemberProvenance::Untyped
}

/// The streak at which the wake loop actually degrades a member.
///
/// One source: `effective_wake_policy()` is what `run_team_member_with_adapter`
/// constructs for every managed member, so this cannot disagree with the gate
/// it defers to.
pub(crate) fn zero_output_degradation_threshold() -> u32 {
    crate::supervisor_wake::effective_wake_policy().zero_output_degradation_threshold
}

/// Return one `Blocked` member to a startable status, on the proof that its own
/// lane is dead and its block carries no typed provenance.
///
/// Only `status` moves. Coordination status, runtime generation,
/// native-session binding and the AgentSession itself are untouched, so the
/// next Supervisor pass resumes the same provider-native session rather than
/// opening a second one, and no killed cycle can be replayed. It needs no
/// Supervisor lease: the lane's own detached, disarmed, unambiguous state is
/// the proof that nothing is driving the member, and the Store's compare-and-
/// append still refuses a row that changed underneath.
///
/// It lives here, next to the two proofs that authorize it, rather than in the
/// recovery command: the authority is the drain-lane reasoning, not the shape
/// of the CLI verb that happens to expose it.
/// The exact Host of this TeamRun as a trust actor, for a Host verb's own
/// canonical writes. `None` only when the run records no Host actor at all
/// (legacy rows); a Host-authority mismatch or an unreadable store is an
/// error, never a silent fall-back to daemon-attributed writes.
pub(super) fn team_run_host_authority(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Option<harness_core::agentfirm_api::ActorRef>> {
    match store.exact_team_run_host_actor(team_run_id) {
        Ok(actor) => Ok(Some(harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: actor.id,
        })),
        Err(harness_store::StoreError::Conflict(message))
            if message.starts_with("TEAM_RUN_HOST_AUTHORITY_REQUIRED:") =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// Return one Blocked member on a dead lane to a startable status, or say
/// exactly why not. A lane the runner left in `RecoveryRequired` first
/// re-enters the ordinary lane through `Idle` — the Store admits that hop only
/// under the terminated-lane proof (GitHub #755) — and only then does the
/// member's status move, so a member is never restarted onto a lane the next
/// start would be refused on (GitHub #841).
pub(super) fn restart_or_explain_blocked_member(
    store: &HarnessStore,
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    now: &str,
    json: bool,
) -> CliResult<Option<serde_json::Value>> {
    let (_, session) = provider_session_for_member(ledger, member)?;
    if session.lifecycle == AgentSessionStatus::RecoveryRequired {
        match transition_provider_session_for_member_as(
            ledger,
            member,
            AgentSessionStatus::Idle,
            team_run_host_authority(store, &ledger.run_id)?,
            "host.team_run_recover.agent_session.resume",
        ) {
            Ok(()) => {}
            Err(error)
                if is_recovery_required_not_yet_resumable(&error)
                    || matches!(&error, CliError::Usage(message) if message.starts_with("NODE_DAEMON_GENERATION_FENCED")) =>
            {
                let blocker = error.to_string();
                if !json {
                    println!(
                        "  {} ({}): blocked, not restarted — {blocker}",
                        member.name, member.provider
                    );
                }
                return Ok(Some(serde_json::json!({
                    "member_run_id": member.id,
                    "name": member.name,
                    "blocker": blocker,
                })));
            }
            Err(error) => return Err(error),
        }
    }
    if let Err(error) = restart_blocked_member_on_dead_lane(store, ledger, member, now) {
        // The lane already re-entered the ordinary lane; only the member row
        // moved under us. Report it and let the next recover finish the flip
        // instead of aborting every remaining member.
        if matches!(&error, CliError::Usage(message) if message.contains("changed concurrently")) {
            let blocker = format!(
                "member row changed concurrently after its lane resumed; run recover again ({error})"
            );
            if !json {
                println!(
                    "  {} ({}): blocked, not restarted — {blocker}",
                    member.name, member.provider
                );
            }
            return Ok(Some(serde_json::json!({
                "member_run_id": member.id,
                "name": member.name,
                "blocker": blocker,
            })));
        }
        return Err(error);
    }
    if !json {
        println!(
            "  {} ({}): blocked member returned to idle; its lane is detached and idle",
            member.name, member.provider
        );
    }
    Ok(None)
}

pub(super) fn restart_blocked_member_on_dead_lane(
    store: &HarnessStore,
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    now: &str,
) -> CliResult<()> {
    let expected = member.clone();
    let mut restarted = expected.clone();
    restarted.status = MemberRunStatus::Idle;
    restarted.finished_at = None;
    restarted.last_event_at = Some(now.to_string());
    store_conflict_as_usage(store.compare_and_append_member_run(&expected, &restarted))?;
    ledger.append_action(
        &member.id,
        "recovered",
        MemberActionStatus::Succeeded,
        "blocked member returned to a startable status",
        &format!(
            "host: the AgentSession lane is detached and idle with no ambiguous RuntimeCommand and the block carries no typed provenance, so runtime generation {} is startable again",
            member.runtime_generation
        ),
    )?;
    ledger.fold_event(
        TeamRunEventSourceKind::Host,
        Some(member.id.clone()),
        "member_run",
        &member.id,
        "recovered",
        &format!(
            "member {} returned from blocked to idle on a detached, idle AgentSession lane",
            member.name
        ),
    )?;
    Ok(())
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

    // `blocked_member_provenance` is exercised in
    // `crate::main_tests::team_run_recover`, next to the recovery-path
    // decisions it feeds and on that module's existing MemberRun fixture,
    // rather than duplicating a thirty-field projection literal here.

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
