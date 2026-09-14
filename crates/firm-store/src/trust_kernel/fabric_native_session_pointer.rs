//! The provider-native session pointer: binding its ONE authority
//! (`AgentSession.native_session_ref`), projecting it onto the MemberRun copies,
//! and resolving which copy a reader may decide from (ADR 0072).
//!
//! Split out of `fabric_identity_sessions.rs` when that file crossed the 1,500
//! line ceiling: this is its own seam, not session lifecycle.

use super::*;

impl HarnessStore {
    /// Whether this member's `MemberRun.native_session` is a **requested**
    /// pointer rather than a projection of an authority.
    ///
    /// The predicate is exact and decidable without reading the MemberRun at
    /// all: *requested iff no AgentSession for this member at this runtime
    /// generation owns a `native_session_ref`.* Prose alone would have left a
    /// reader unable to tell "requested" from "asserted" without loading the
    /// session anyway, so the rule is a function with a test.
    ///
    /// Two cases are requested and both must keep working: a `--resume-member`
    /// / `resume_native_session_id` seed lands before any session exists (#845
    /// pre-Open attachment), and an `external_interactive` Host never has an
    /// AgentSession at all, so its pointer is requested for the lane's whole
    /// life.
    pub fn member_run_native_session_is_requested(
        &self,
        execution_space_id: &str,
        agent_member_id: &str,
        runtime_generation: u64,
    ) -> StoreResult<bool> {
        Ok(self
            .deciding_native_session_unlocked(
                execution_space_id,
                agent_member_id,
                runtime_generation,
                None,
            )?
            .is_none())
    }
    /// The read-path entry to the authoritative provider-native pointer, for
    /// callers outside the Store. See `deciding_native_session_unlocked`: this
    /// takes no lock either, because it only reads the trust journal.
    pub fn deciding_native_session(
        &self,
        execution_space_id: &str,
        agent_member_id: &str,
        runtime_generation: u64,
        projected: Option<&NativeSessionRef>,
    ) -> StoreResult<Option<NativeSessionRef>> {
        self.deciding_native_session_unlocked(
            execution_space_id,
            agent_member_id,
            runtime_generation,
            projected,
        )
    }
    /// The pointer a decision must be made from, for one member at one runtime
    /// generation.
    ///
    /// Authority first (ADR 0071): when an AgentSession for this exact member
    /// and generation owns a pointer, that is the answer. The MemberRun
    /// projection answers only when no session owns one — before any session
    /// exists it is the `requested` pointer (a `--resume-member` seed, #845
    /// pre-Open attachment), and an `external_interactive` Host never has an
    /// AgentSession at all.
    ///
    /// Lifecycle is deliberately NOT filtered here. This answers "which
    /// provider-native conversation does the authority name", which survives
    /// Close; whether a lane is live enough to act on is a separate question
    /// each caller already asks in its own terms.
    ///
    /// Callers must already hold the Store write lock or be inside a read that
    /// tolerates none: this performs no locking of its own.
    pub(crate) fn deciding_native_session_unlocked(
        &self,
        execution_space_id: &str,
        agent_member_id: &str,
        runtime_generation: u64,
        projected: Option<&NativeSessionRef>,
    ) -> StoreResult<Option<NativeSessionRef>> {
        let mut owned = self
            .fabric_agent_sessions(execution_space_id)?
            .into_iter()
            .filter(|session| {
                session.agent_member_id == agent_member_id
                    && session.runtime_generation == runtime_generation
                    && session.native_session_ref.is_some()
            })
            .collect::<Vec<_>>();
        // A live lane's answer wins over a closed one's when both exist; a
        // reader must not fail on an ambiguity it cannot repair.
        owned.sort_by_key(|session| session.lifecycle == AgentSessionStatus::Closed);
        match owned.into_iter().next() {
            Some(session) => Ok(session.native_session_ref),
            None => Ok(projected.cloned()),
        }
    }
    /// The ONE entrance that writes a settled provider-native Session pointer.
    ///
    /// `AgentSession.native_session_ref` is the authority (ADR 0071). This
    /// binds it and, in the SAME atomic ledger rewrite, projects the exact same
    /// value onto the `MemberRun` that runs this session, then projects it onto
    /// the legacy `member_runs.jsonl` row under the same write lock. Callers
    /// cannot write a projection on their own, so a projection can never be
    /// observed without its authority, and no caller can pair the wrong
    /// MemberRun: it is resolved here from the session's own
    /// `agent_member_id` + `runtime_generation`.
    ///
    /// A fresh-start session is materialized before the provider thread exists
    /// (`native_session_ref` starts unset), so the settled binding lands later
    /// as its own CAS + generation-fenced mutation. Lifecycle and runtime
    /// generation are untouched. The write is idempotent for the same native id
    /// and rejects a conflicting rebind to another id — on either record.
    pub fn bind_agent_session_native_session(
        &self,
        context: &MutationContext,
        session_id: &str,
        expected_generation: u64,
        native_session_ref: NativeSessionRef,
    ) -> StoreResult<CanonicalMutationResult<AgentSession>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        required(
            &native_session_ref.native_session_id,
            "NativeSessionRef.native_session_id",
        )?;
        required(&native_session_ref.provider, "NativeSessionRef.provider")?;
        let mut session = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_session")?
            .remove(session_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession not found",
                    "agent_session",
                    session_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentSession>(&envelope))?;
        self.require_current_node_daemon_unlocked(
            &context.execution_space_id,
            &session.node_id,
            &session.node_daemon_id,
            session.node_daemon_generation,
            &context.authenticated_actor,
            "agent_session",
            session_id,
        )?;
        if session.lifecycle == AgentSessionStatus::Closed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "a closed AgentSession cannot bind a provider-native Session",
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if session.runtime_generation != expected_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                format!(
                    "AgentSession runtime generation is {}, the settled binding observed {expected_generation}",
                    session.runtime_generation
                ),
                "agent_session",
                session_id,
                Some(session.version),
            ));
        }
        if let Some(current) = session.native_session_ref.as_ref() {
            if current.native_session_id != native_session_ref.native_session_id {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentSession already binds another provider-native Session",
                    "agent_session",
                    session_id,
                    Some(session.version),
                ));
            }
        }
        session.native_session_ref = Some(native_session_ref.clone());
        session.version += 1;

        // Resolve the MemberRun this session runs under, and refuse a
        // projection that would disagree with the authority we are about to
        // write. `same_identity_as` is the test: availability and verification
        // timestamps are observations that legitimately differ between a
        // projection written earlier and this settlement.
        let paired = self.member_run_projection_for_session_unlocked(
            &context.execution_space_id,
            &session,
            &native_session_ref,
        )?;

        let committed = self.commit_trust_projection_with_paired_aggregate_unlocked(
            context,
            "agent_session",
            session_id,
            "native_session_bound",
            serde_json::json!({
                "session_id": session_id,
                "runtime_generation": expected_generation,
                "native_session_ref": native_session_ref,
            }),
            &session,
            Vec::new(),
            Vec::new(),
            paired.as_ref().map(|run| {
                let mut projected = run.clone();
                projected.native_session = Some(native_session_ref.clone());
                projected.version = run.version + 1;
                PairedAggregateTransition::MemberRun(PairedMemberRunProjection {
                    transition: "native_session_projected",
                    expected_version: run.version,
                    run: projected,
                })
            }),
        )?;

        // The two MemberRun ledgers are ONE record in two files, and the Store
        // already enforces that they move together:
        // `current_member_lifecycle_validation_mismatch_fields` fails closed
        // with `MEMBER_RUN_MATERIALIZATION_MISMATCH … differs … for fields
        // native_session` on the next read if they disagree. Writing only the
        // canonical half would therefore leave every later admission refusing
        // this TeamRun — proven by three trust-kernel tests.
        //
        // So the legacy row is appended here, under this same lock, exactly as
        // every other MemberRun writer in this Store does
        // (`transition_current_team_member_lifecycle`,
        // `compare_and_advance_member_run_generation`), accepting the
        // pre-existing `MEMBER_RUN_DUAL_LEDGER_COMMIT_INCOMPLETE` caveat that
        // a cross-file pair cannot be made atomic without a journal this Store
        // deliberately does not have.
        //
        // What this entrance DOES avoid is a second escape hatch on the shared
        // trust-commit primitive: the two trust aggregates land in one atomic
        // rewrite above, and only the established dual-ledger append follows.
        //
        // Deliberately unconditional, including on an idempotent replay: the
        // append is a no-op when the row already agrees, so a replay repairs a
        // row left stale by an earlier failure here.
        if paired.is_some() {
            self.project_native_session_onto_member_runs_jsonl_unlocked(
                &session.agent_member_id,
                session.runtime_generation,
                &native_session_ref,
            )?;
        }
        Ok(committed)
    }
    /// Find the MemberRun that projects this session's pointer, or `None` when
    /// this store holds no trust MemberRun for it (a session-only fixture
    /// store). The returned run is the CURRENT one; the caller derives the
    /// projected revision from it.
    ///
    /// Refuses a MemberRun that already names a different provider-native
    /// conversation, and a MemberRun that is no longer active — a closed run
    /// must not acquire a new binding just because its session can.
    fn member_run_projection_for_session_unlocked(
        &self,
        execution_space_id: &str,
        session: &AgentSession,
        native_session_ref: &NativeSessionRef,
    ) -> StoreResult<Option<firm_core::agentfirm_api::MemberRun>> {
        let mut candidates = self
            .latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .into_values()
            .map(|envelope| event_projection::<firm_core::agentfirm_api::MemberRun>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|run| {
                run.agent_member_id == session.agent_member_id
                    && run.runtime_generation == session.runtime_generation
            })
            .collect::<Vec<_>>();
        let run = match candidates.len() {
            0 => return Ok(None),
            1 => candidates.remove(0),
            found => {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    format!(
                        "MEMBER_RUN_PROJECTION_AMBIGUOUS: {} has {found} MemberRuns at runtime generation {}",
                        session.agent_member_id, session.runtime_generation
                    ),
                    "agent_session",
                    &session.id,
                    Some(session.version),
                ))
            }
        };
        if run.coordination_status != MemberCoordinationStatus::Active {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                format!(
                    "only an active MemberRun can project a provider-native Session; MemberRun {} is {:?}",
                    run.id, run.coordination_status
                ),
                "member_run",
                &run.id,
                Some(run.version),
            ));
        }
        if let Some(current) = run.native_session.as_ref() {
            if !current.same_identity_as(native_session_ref) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    format!(
                        "NATIVE_SESSION_PROJECTION_DISAGREES: MemberRun {} already names provider-native session {}, which is not the session AgentSession {} is binding",
                        run.id, current.native_session_id, session.id
                    ),
                    "member_run",
                    &run.id,
                    Some(run.version),
                ));
            }
        }
        Ok(Some(run))
    }
    /// Append the authority value onto the legacy runtime row, under the
    /// caller's lock. Absent rows are skipped: a store with no
    /// `member_runs.jsonl` row for this member has no pair to keep whole.
    fn project_native_session_onto_member_runs_jsonl_unlocked(
        &self,
        agent_member_id: &str,
        runtime_generation: u64,
        native_session_ref: &NativeSessionRef,
    ) -> StoreResult<()> {
        let Some(mut row) = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .into_values()
        .find(|row| {
            row.agent_member_id == agent_member_id && row.runtime_generation == runtime_generation
        }) else {
            return Ok(());
        };
        if row.native_session.as_ref() == Some(native_session_ref) {
            return Ok(());
        }
        row.native_session = Some(native_session_ref.clone());
        self.append_jsonl_unlocked("member_runs.jsonl", &row)
            .map_err(|error| {
                StoreError::Conflict(format!(
                    "NATIVE_SESSION_PROJECTION_INCOMPLETE: agent_member_id={agent_member_id} runtime_generation={runtime_generation}; the authoritative AgentSession binding and the canonical MemberRun are durable, the {} half is not; re-derive it with derive_member_runs_jsonl_native_session (no automatic replay): {error}",
                    self.root.join("member_runs.jsonl").display(),
                ))
            })
    }
    /// Rebuild the legacy `member_runs.jsonl` pointer from the authority.
    ///
    /// This is a DERIVATION, not a second write path: it reads the committed
    /// `AgentSession.native_session_ref` and makes the legacy row agree with
    /// it. Nothing decides from that row — every deciding reader resolves the
    /// pointer through `deciding_native_session_unlocked` — so this exists for
    /// display, export, and operator sanity rather than correctness, and a
    /// store that never calls it is not wrong, only stale.
    ///
    /// Returns whether a row was rewritten. Absent rows and already-agreeing
    /// rows are no-ops.
    pub fn derive_member_runs_jsonl_native_session(
        &self,
        execution_space_id: &str,
        member_run_id: &str,
    ) -> StoreResult<bool> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let Some(mut row) = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(member_run_id) else {
            return Ok(false);
        };
        let authority = self.deciding_native_session_unlocked(
            execution_space_id,
            &row.agent_member_id,
            row.runtime_generation,
            None,
        )?;
        let Some(authority) = authority else {
            // No AgentSession owns a pointer for this generation, so the row
            // still carries the `requested` pointer and there is nothing to
            // derive it from.
            return Ok(false);
        };
        if row.native_session.as_ref() == Some(&authority) {
            return Ok(false);
        }
        row.native_session = Some(authority);
        self.append_jsonl_unlocked("member_runs.jsonl", &row)?;
        Ok(true)
    }
}
