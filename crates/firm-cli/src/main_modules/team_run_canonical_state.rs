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

// TODO(#726 follow-up): each call re-reads `member_runs`, `work_operations`,
// `fabric_messages`, `runtime_commands` and — since #779 — `fabric_agent_sessions`
// for the whole Store. A scan that holds N runs therefore pays N whole-Store
// passes. That is already the existing shape of `scan_and_adopt`
// (`team_run_has_active_member` reads the same member-run collection per run),
// and the fingerprint is only computed for runs that actually carry a hold, so
// it strictly replaces a far more expensive Supervisor spawn. Hoisting all five
// collections to one read per scan pass means threading a borrowed snapshot
// through `team_run_adoption_is_held`, `drive_prepared_team_run` and the reap
// path, which is a wider change than this review; recorded rather than
// half-done.

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

    // Count this run's Work progress through the one Work journal. Counting
    // only `work_operations.jsonl` made an accept, a cancellation or a
    // dependency change look like no canonical progress at all, so a hold that
    // those settled kept holding (#949 item 2); since the writer cutover that
    // file holds no current transition at all, so a ledger-only count would
    // see none of them.
    //
    // The filter is the Work event's own `team_run_id`, which is the run that
    // performed the transition — a Work retargeted to a successor run stops
    // moving its predecessor's counter and starts moving the successor's,
    // which is what a per-run hold wants. Pinned by
    // `work_journal_is_one_reader_across_both_journals`.
    let work_operations = store.work_journal_cursors_for_team_run(run_id)?.watermark;

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
            // Scoped twice on purpose. An AgentSession carries no TeamRun, so
            // the run's own AgentMembers are the tightest scope available —
            // without it a Space-wide read would hash a lane belonging to
            // another run into this run's hold. `Closed` sessions are excluded
            // because they are history: a member that has been closed and
            // reopened would otherwise carry its retired lanes in every
            // fingerprint forever, and none of them can ever change again.
            let mut agent_session_lanes = store
                .fabric_agent_sessions(space_id)?
                .into_iter()
                .filter(|session| {
                    agent_member_ids.contains(&session.agent_member_id)
                        && session.lifecycle
                            != harness_core::agentfirm_api::AgentSessionStatus::Closed
                })
                .map(|session| agent_session_lane_projection(&session))
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
        &canonical_state_document(
            // Frozen durable hash input, like every key below.
            serde_json::json!({
                "id": run.id,
                "status": run.status,
                "completed": run.completed_at.is_some(),
                "member_run_ids": run.member_run_ids,
            }),
            members,
            work_operations.total(),
            execution_space_id,
            messages,
            runtime_commands,
            agent_session_lanes,
        ),
    ))
}

/// The hashed shape of one AgentSession lane.
///
/// **Every key here is a frozen durable hash input, not vocabulary.** The
/// fingerprint this feeds is written into the TeamRun ledger as a
/// `team-run-canonical-state:` evidence ref on a `team_supervisor_no_progress`
/// hold, and a later NodeDaemon generation — possibly a different binary —
/// recomputes it and compares the two for equality. `canonical_json_fingerprint`
/// hashes object keys, so renaming one silently invalidates every hold written
/// before the rename: adoption is re-enabled on an unchanged run and burns a
/// fresh TeamSupervisor generation (#671, #704), on the heartbeat-starvation
/// path (#836). `in_turn` therefore keeps its spelling even though the field it
/// reflects is now `current_cycle_marker` (ADR 0070) — the key names a durable
/// hash slot, and the value is still exactly "this lane has a cycle open".
/// `canonical_state_document_keys_are_frozen_durable_hash_inputs` pins this.
fn agent_session_lane_projection(
    session: &harness_core::agentfirm_api::AgentSession,
) -> serde_json::Value {
    serde_json::json!({
        "id": session.id,
        "lifecycle": session.lifecycle,
        "runtime_generation": session.runtime_generation,
        "node_daemon_generation": session.node_daemon_generation,
        "runtime_residency": session.control_state.runtime_residency,
        "activity": session.control_state.activity,
        "continuation_activation": session.control_state.continuation.activation,
        "handoff_state": session.control_state.handoff_state,
        "in_turn": session.current_cycle_marker.is_some(),
        "queued_input_count": session.queued_input_count,
    })
}

/// The hashed document itself. Same frozen-key rule as the lane projection
/// above: these spellings are durable hash inputs compared across daemon
/// generations, so changing one is a behaviour change, not a rename.
fn canonical_state_document(
    team_run: serde_json::Value,
    member_runs: Vec<serde_json::Value>,
    work_operations: u64,
    execution_space_id: Option<&str>,
    messages: Option<usize>,
    runtime_commands: Option<usize>,
    agent_session_lanes: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    serde_json::json!({
        "team_run": team_run,
        "member_runs": member_runs,
        "work_operations": work_operations,
        "execution_space_id": execution_space_id,
        "messages": messages,
        "runtime_commands": runtime_commands,
        "agent_session_lanes": agent_session_lanes,
    })
}

/// Build the evidence ref that binds a durable outcome to one fingerprint.
#[cfg(test)]
use crate::daemon_support::canonical_state_evidence_ref;

/// Recover the fingerprint a durable outcome was bound to, if any.
#[cfg(test)]
use crate::daemon_support::canonical_state_from_evidence;

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

    /// The hashed key spellings are durable: a `team_supervisor_no_progress`
    /// hold stores this fingerprint in the TeamRun ledger and a later daemon
    /// generation recomputes and compares it. Renaming a key silently
    /// invalidates every hold written before the rename, re-enabling adoption
    /// on an unchanged run. This test is the trip-wire so that lands here
    /// rather than in a dogfood run.
    #[test]
    fn canonical_state_document_keys_are_frozen_durable_hash_inputs() {
        let session: harness_core::agentfirm_api::AgentSession =
            serde_json::from_value(serde_json::json!({
                "id": "agent-session:member:node:1:1",
                "agent_member_id": "member",
                "node_id": "node",
                "execution_space_id": "space",
                "node_daemon_id": "node-daemon:node",
                "node_daemon_generation": 1,
                "provider_kind": "codex",
                "provider_profile_ref": "codex-default",
                "permission_envelope_ref": "agent-member:member:permission",
                "effective_permission_ceiling": "workspace_write",
                "lifecycle": "active",
                "runtime_generation": 1,
                "current_cycle_marker": "harness-cycle:agent-session:member:node:1:1:4",
                "queued_input_count": 0,
                "version": 4,
                "opened_at": "t1",
                "last_active_at": "t1"
            }))
            .expect("lane fixture decodes");

        let lane = agent_session_lane_projection(&session);
        assert_eq!(
            lane.as_object()
                .expect("lane is an object")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "activity",
                "continuation_activation",
                "handoff_state",
                "id",
                "in_turn",
                "lifecycle",
                "node_daemon_generation",
                "queued_input_count",
                "runtime_generation",
                "runtime_residency",
            ],
            "renaming a hashed lane key invalidates every stored hold; \
             `in_turn` stays spelled that way even though the field it \
             reflects is now `current_cycle_marker` (ADR 0070)"
        );
        assert_eq!(
            lane["in_turn"],
            serde_json::json!(true),
            "the value is still exactly `this lane has a cycle open`"
        );

        let document = canonical_state_document(
            serde_json::json!({"id": "run-1"}),
            vec![],
            0,
            Some("space"),
            Some(0),
            Some(0),
            Some(vec![lane]),
        );
        assert_eq!(
            document
                .as_object()
                .expect("document is an object")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "agent_session_lanes",
                "execution_space_id",
                "member_runs",
                "messages",
                "runtime_commands",
                "team_run",
                "work_operations",
            ],
            "renaming a hashed top-level key invalidates every stored hold"
        );
    }
}
