//! The one writer seam for Work.
//!
//! Every current Work transition commits one `work` aggregate envelope to the
//! trust journal, carrying its complete [`WorkOperation`] — event, resulting
//! projection, and the condition records, reports, evidence and decisions the
//! ledger row used to carry — as an immutable side record.
//! `work_operations.jsonl` is no longer written by any production writer; it is
//! legacy read-only input, and a store created after this slice may never have
//! the file at all.
//!
//! Two obligations shape this module.
//!
//! **Idempotency runs before the version fence.** A retry of a command whose
//! first attempt already committed must return that committed result, not
//! `VERSION_CONFLICT` from the revision its own first attempt produced. The
//! entrance therefore resolves the Work without a version check, builds the
//! canonical [`MutationContext`], and asks the trust kernel's replay check —
//! all before the command's guards run. The replay is exact: the same key with
//! different request content is `IdempotencyKeyReused`, never a silent
//! substitution.
//!
//! **The performer is an identity, not a runtime generation.** A member write
//! arrives signed by the exact MemberRun that was fenced
//! (`ProviderRuntimeProjection/<member-run-id>`, which every `require_member_actor`
//! gate still checks against the caller context). What gets persisted is the
//! durable AgentMember, with the MemberRun recorded as evidence in
//! [`WorkEvent::executed_by_member_run_id`]. Legacy rows keep the old shape and
//! every predicate reads both through `WorkEvent::executing_member_run_id`.
use super::*;
use firm_core::agentfirm_api::{ActorKind, ActorRef, MutationContext};

/// Marks a `ChangesRequested` payload written by the exact-Team-peer reviewer
/// gate, and nothing else.
///
/// Peer review of Host-owned Work re-authorizes the next execution admission
/// exactly like the Host's own review does. Before W4 that was keyed on the
/// performer being a `ProviderRuntimeProjection`, which only worked because the
/// runtime generation was the performer. Now that a member write persists its
/// AgentMember identity, "a non-Host AgentMember requested changes" would admit
/// every member, so re-authorization keys on this marker: the peer arm of
/// `request_work_changes_by_reviewer` is its only writer, and it has already
/// proven the exact active non-owner Team peer before it is set.
pub(super) const PEER_REVIEW_MARKER: &str = "peer_reviewed";

/// The Execution Space a Work command is scoped to.
///
/// One accessor, so no writer can drift from the scope its own entrance
/// resolved and its caller read.
pub(super) fn command_space(context: &MutationContext) -> ExecutionSpaceId {
    ExecutionSpaceId::new(&context.execution_space_id)
}

/// What the Work command entrance decided.
pub(super) enum WorkCommandEntrance {
    /// This exact request already committed. Its committed Work is the answer;
    /// no guard runs again and nothing is appended.
    Replayed(Box<Work>),
    /// The command may proceed. Guards run next, then exactly one commit
    /// through [`HarnessStore::append_work_transition_with_records_unlocked`].
    Admitted(Box<MutationContext>),
}

impl HarnessStore {
    /// The canonical actor a Work command's caller context authenticates as.
    ///
    /// | caller `TeamActorRef`                        | canonical `ActorRef`         |
    /// |----------------------------------------------|------------------------------|
    /// | `Host/<host-agent-id>`                       | `AgentMember/<host-agent-id>` |
    /// | `ProviderRuntimeProjection/<member-run-id>`  | `AgentMember/<agent-member-id>` |
    /// | `AgentMember/<agent-member-id>`              | `AgentMember/<agent-member-id>` |
    /// | `Operator/<id>`                              | `Human/<id>`                 |
    /// | `Service/<id>`                               | `Service/<id>`               |
    ///
    /// The Host row is an identity, not a widening: `require_exact_team_run_host_actor`
    /// has already proven that this id is the AgentTeam's `host_agent_id` and
    /// holds the one active Host membership. `Operator -> Human` is the exact
    /// inverse of the acceptance rollup in `trust_work_acceptance`, which reads
    /// `Human | External` back as `Operator`.
    ///
    /// A `ProviderRuntimeProjection` whose MemberRun cannot be resolved in this
    /// TeamRun keeps its caller id rather than inventing one; the caller-context
    /// gates refuse such an actor before any commit, so this arm is only ever
    /// reached by a read of an unauthorized request.
    pub(super) fn canonical_work_actor_unlocked(
        &self,
        actor: &TeamActorRef,
        team_run_id: &str,
    ) -> ActorRef {
        let id = match actor.kind {
            TeamActorKind::ProviderRuntimeProjection => self
                .require_member_run_unlocked(&actor.id, team_run_id)
                .map(|member| member.agent_member_id)
                .unwrap_or_else(|_| actor.id.clone()),
            _ => actor.id.clone(),
        };
        ActorRef {
            kind: match actor.kind {
                TeamActorKind::Service => ActorKind::Service,
                TeamActorKind::Host
                | TeamActorKind::AgentMember
                | TeamActorKind::ProviderRuntimeProjection => ActorKind::AgentMember,
                TeamActorKind::Operator => ActorKind::Human,
            },
            id,
        }
    }

    /// The persisted performer for a Work transition, and the MemberRun
    /// evidence that carried it.
    ///
    /// Only the runtime-projection caller shape is rewritten. A Host write
    /// keeps `TeamActorKind::Host`, which is what the Host-suppression and
    /// re-authorization predicates read.
    pub(super) fn persisted_work_performer_unlocked(
        &self,
        actor: &TeamActorRef,
        team_run_id: &str,
    ) -> (TeamActorRef, Option<String>) {
        if actor.kind != TeamActorKind::ProviderRuntimeProjection {
            return (actor.clone(), None);
        }
        let Ok(member) = self.require_member_run_unlocked(&actor.id, team_run_id) else {
            return (actor.clone(), None);
        };
        (
            TeamActorRef {
                kind: TeamActorKind::AgentMember,
                id: member.agent_member_id,
                display_name: actor.display_name.clone(),
                authn_source: actor.authn_source.clone(),
            },
            Some(actor.id.clone()),
        )
    }

    /// Resolve the canonical mutation context for a Work command and answer an
    /// exact replay, both before the command's own guards and version fence.
    pub(super) fn enter_work_command_unlocked(
        &self,
        work_id: &str,
        expected_version: u64,
        kind: WorkEventKind,
        context: &WorkCommandContext,
        request_payload: &serde_json::Value,
    ) -> StoreResult<WorkCommandEntrance> {
        let loose = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        self.enter_work_command_for_run_unlocked(
            work_id,
            &loose.team_run_id,
            expected_version,
            kind,
            context,
            request_payload,
        )
    }

    /// The creation entrance: there is no Work yet, so the Execution Space
    /// comes from the TeamRun the Work is being created in.
    pub(super) fn enter_work_command_for_run_unlocked(
        &self,
        work_id: &str,
        team_run_id: &str,
        expected_version: u64,
        kind: WorkEventKind,
        context: &WorkCommandContext,
        request_payload: &serde_json::Value,
    ) -> StoreResult<WorkCommandEntrance> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        let execution_space_id = self.current_team_run_execution_space_unlocked(&run)?;
        let request_fingerprint = canonical_json_fingerprint(request_payload);
        let mutation_context = MutationContext {
            execution_space_id,
            authenticated_actor: self
                .canonical_work_actor_unlocked(&context.performed_by_actor, team_run_id),
            authority_actor: context
                .authority_actor
                .as_ref()
                .map(|actor| self.canonical_work_actor_unlocked(actor, team_run_id)),
            command_name: format!("work.{}", kind.canonical_transition()),
            idempotency_key: context.idempotency_key.clone(),
            expected_version,
            request_fingerprint: Some(request_fingerprint.clone()),
        };
        if let Some(replay) = self.replay_current_work_mutation_unlocked(
            &mutation_context,
            work_id,
            &request_fingerprint,
        )? {
            return Ok(WorkCommandEntrance::Replayed(Box::new(replay.projection)));
        }
        Ok(WorkCommandEntrance::Admitted(Box::new(mutation_context)))
    }

    /// The Work revisions this store holds for one Work across every journal,
    /// used for the next event sequence and for event-id availability.
    pub(super) fn work_journal_event_ids_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeSet<String>> {
        Ok(self
            .work_journal_unlocked()?
            .records
            .iter()
            .map(|record| record.event.id.clone())
            .collect())
    }

    /// Every WorkOperation this store holds: the legacy ledger rows plus the
    /// complete operations current writers commit into `work` envelopes.
    ///
    /// Order is source-major and deliberately so — the two files share no
    /// comparable clock (see [`crate::work_history`]). Every consumer of this
    /// list reads it as a *set* of immutable per-row records (condition
    /// records, reports, evidence, decisions, event ids); nothing here may
    /// depend on cross-source ordering.
    pub(super) fn work_record_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        let mut operations = self.work_operations_unlocked()?;
        operations.extend(self.trust_read_model()?.work_operations());
        Ok(operations)
    }
}
