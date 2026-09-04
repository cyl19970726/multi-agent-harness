//! One fingerprint of the canonical coordination state of a single TeamRun.
//!
//! Re-adopting a `Running` TeamRun is only worth doing when something a new
//! Supervisor generation could act on has changed. This module derives that
//! "something" from the coordination rows Harness actually owns — the TeamRun
//! and MemberRun rows, the three independent planes named by the
//! execution-foundation contract (`Work`, `Message` and `RuntimeCommand`), and
//! the AgentSession control state that decides whether a member's lane can be
//! driven at all. It owns no state, writes nothing, and is never an authority
//! of its own: it only lets the NodeDaemon say "this exact observed state
//! already produced its outcome".
//!
//! Two exclusions carry the whole design, and both exist because a fingerprint
//! that any failing adoption can move is worthless:
//!
//! 1. **No journal rows.** TeamRunEvent and MemberAction are the daemon's own
//!    log; an adoption that achieved nothing still appends them.
//! 2. **No clock stamps.** `last_event_at`, `updated_at` and `finished_at` are
//!    wall-clock observations, not coordination facts.
//!    `claim_member_provider_start` stamps `last_event_at = now()` before the
//!    transport is even attempted, so including it made every provider-start
//!    failure — the unreleased AgentSession, stale permission ceiling and
//!    provider start-error classes of #671 — look like canonical progress and
//!    earn another Supervisor generation. `finished_at` is reduced to the
//!    boolean coordination fact it stands for.
//!
//! `runtime_generation` is deliberately kept: it moves only on an explicit
//! Reopen, which is precisely the Host intent that should re-enable adoption.
//! `zero_output_streak` is deliberately dropped: it is bookkeeping for the
//! degradation ladder whose only adoption-relevant end state, `Blocked`, is
//! already carried by `status`.
//!
//! Known limit: a `Message` whose `team_run_id` is `None` is invisible here,
//! so it cannot by itself lift a hold. Such a message is not addressed to this
//! run's execution attempt, and any delivery it produces moves a MemberRun row
//! or a WorkDelivery that this fingerprint does see.
//!
//! A member start that fails *after* its status CAS legitimately changes
//! `status` and so costs one further adoption; the next one finds the member
//! unclaimable, writes nothing, and holds. That is still at most one adoption
//! per distinct canonical state, which is the invariant (#704, #671).

use harness_core::agentfirm_api::RuntimeDriverRef;
use harness_store::HarnessStore;

use crate::CliResult;

/// Evidence-ref prefix that binds a durable adoption outcome to the exact
/// canonical state it was observed under.
pub(super) const CANONICAL_STATE_EVIDENCE_PREFIX: &str = "team-run-canonical-state:";

// TODO(#726 follow-up): each call re-reads `member_runs`, `work_operations`,
// `fabric_messages` and `runtime_commands` for the whole Store. A scan that
// holds N runs therefore pays N whole-Store passes. That is already the
// existing shape of `scan_and_adopt` (`team_run_has_active_member` reads the
// same member-run collection per run), and the fingerprint is only computed
// for runs that actually carry a hold, so it strictly replaces a far more
// expensive Supervisor spawn. Hoisting all four collections to one read per
// scan pass means threading a borrowed snapshot through `team_run_adoption_is_held`,
// `drive_prepared_team_run` and the reap path, which is a wider change than
// this review; recorded rather than half-done.

/// Fingerprint the canonical coordination state of one TeamRun.
///
/// `execution_space_id` is `None` only where the caller genuinely has no
/// Execution Space scope; the Message and RuntimeCommand planes are then
/// omitted rather than guessed, and the fingerprint says so explicitly so two
/// differently-scoped observations can never compare equal.
pub(super) fn team_run_canonical_state_fingerprint(
    store: &HarnessStore,
    execution_space_id: Option<&str>,
    run_id: &str,
) -> CliResult<String> {
    let run = crate::latest_team_run(store, run_id)?;
    let member_rows = crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run_id)
        .collect::<Vec<_>>();
    let agent_member_ids = member_rows
        .iter()
        .map(|member| member.agent_member_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut members = member_rows
        .iter()
        .map(|member| {
            serde_json::json!({
                "id": member.id,
                "status": member.status,
                "coordination_status": member.coordination_status,
                "runtime_generation": member.runtime_generation,
                "finished": member.finished_at.is_some(),
                "native_session": member.native_session.as_ref().map(|session| {
                    serde_json::json!({
                        "provider": session.provider,
                        "execution_mode": session.execution_mode,
                        "native_session_id": session.native_session_id,
                        "availability": session.availability,
                    })
                }),
            })
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));

    let work_operations = store
        .work_operations()?
        .into_iter()
        .filter(|operation| operation.work.team_run_id == run_id)
        .count();

    let member_run_ids = members
        .iter()
        .filter_map(|member| member["id"].as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();
    let (messages, runtime_commands, agent_session_lanes) = match execution_space_id {
        Some(space_id) => {
            let messages = store
                .fabric_messages(space_id)?
                .into_iter()
                .filter(|message| message.team_run_id.as_deref() == Some(run_id))
                .count();
            // A RuntimeCommand reaches this TeamRun either through its
            // TeamSupervisor driver or by binding one of the run's MemberRuns.
            // Counting only the first missed every command a Host issued
            // against a member directly, leaving those invisible to the hold.
            let runtime_commands = store
                .runtime_commands(space_id)?
                .into_iter()
                .filter(|command| {
                    matches!(
                        &command.binding.target_driver,
                        RuntimeDriverRef::TeamSupervisor { team_run_id, .. }
                            if team_run_id == run_id
                    ) || command
                        .binding
                        .target_member_run_id
                        .as_deref()
                        .is_some_and(|member_run_id| member_run_ids.contains(member_run_id))
                })
                .count();
            // Whether each member's lane can be driven at all is canonical
            // state this hold must see. A lane a NodeDaemon drain left
            // `Interrupted`, or one still carrying a dead runtime's attached
            // residency, becomes resumable without any MemberRun, Work,
            // Message or RuntimeCommand row changing — and a hold that could
            // not observe that stood until a Host poked the run (#779). Only
            // the fields that decide resumability are read: transcript truth
            // stays with the provider (ADR 0032).
            let mut agent_session_lanes = store
                .fabric_agent_sessions(space_id)?
                .into_iter()
                .filter(|session| agent_member_ids.contains(&session.agent_member_id))
                .map(|session| {
                    serde_json::json!({
                        "id": session.id,
                        "lifecycle": session.lifecycle,
                        "runtime_generation": session.runtime_generation,
                        "node_daemon_generation": session.node_daemon_generation,
                        "runtime_residency": session.control_state.runtime_residency,
                        "activity": session.control_state.activity,
                        "continuation_activation": session.control_state.continuation.activation,
                        "handoff_state": session.control_state.handoff_state,
                        "in_turn": session.current_turn_id.is_some(),
                        "queued_input_count": session.queued_input_count,
                    })
                })
                .collect::<Vec<_>>();
            agent_session_lanes
                .sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
            (
                Some(messages),
                Some(runtime_commands),
                Some(agent_session_lanes),
            )
        }
        None => (None, None, None),
    };

    Ok(harness_store::canonical_json_fingerprint(
        &serde_json::json!({
            "team_run": {
                "id": run.id,
                "status": run.status,
                "completed": run.completed_at.is_some(),
                "member_run_ids": run.member_run_ids,
            },
            "member_runs": members,
            "work_operations": work_operations,
            "execution_space_id": execution_space_id,
            "messages": messages,
            "runtime_commands": runtime_commands,
            "agent_session_lanes": agent_session_lanes,
        }),
    ))
}

/// Build the evidence ref that binds a durable outcome to one fingerprint.
pub(super) fn canonical_state_evidence_ref(fingerprint: &str) -> String {
    format!("{CANONICAL_STATE_EVIDENCE_PREFIX}{fingerprint}")
}

/// Recover the fingerprint a durable outcome was bound to, if any.
pub(super) fn canonical_state_from_evidence(evidence_refs: &[String]) -> Option<&str> {
    evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix(CANONICAL_STATE_EVIDENCE_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_state_evidence_round_trips_exactly_one_fingerprint() {
        let reference = canonical_state_evidence_ref("sha256:abc");
        assert_eq!(reference, "team-run-canonical-state:sha256:abc");
        assert_eq!(
            canonical_state_from_evidence(&[
                "unrelated-evidence".to_string(),
                reference.clone(),
                canonical_state_evidence_ref("sha256:def"),
            ]),
            Some("sha256:abc"),
            "the first bound fingerprint is the one the outcome was written under"
        );
        assert_eq!(
            canonical_state_from_evidence(&["unrelated-evidence".to_string()]),
            None
        );
        assert_eq!(canonical_state_from_evidence(&[]), None);
    }
}
