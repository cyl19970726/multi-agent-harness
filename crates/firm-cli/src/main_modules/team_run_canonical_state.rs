//! One fingerprint of the canonical coordination state of a single TeamRun.
//!
//! Re-adopting a `Running` TeamRun is only worth doing when something a new
//! Supervisor generation could act on has changed. This module derives that
//! "something" from the coordination rows Harness actually owns — the TeamRun
//! and MemberRun rows plus the three independent planes named by the
//! execution-foundation contract: `Work`, `Message` and `RuntimeCommand`. It
//! owns no state, writes nothing, and is never an authority of its own: it
//! only lets the NodeDaemon say "this exact observed state already produced
//! its outcome".
//!
//! The fingerprint deliberately ignores TeamRunEvent and MemberAction rows.
//! Those are the daemon's own journal: an adoption that achieved nothing still
//! appends them, so including them would make every no-progress adoption look
//! like a canonical change and restore the re-adoption loop this fingerprint
//! exists to close (#704, #671).

use harness_core::agentfirm_api::RuntimeDriverRef;
use harness_store::HarnessStore;

use crate::CliResult;

/// Evidence-ref prefix that binds a durable adoption outcome to the exact
/// canonical state it was observed under.
pub(super) const CANONICAL_STATE_EVIDENCE_PREFIX: &str = "team-run-canonical-state:";

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
    let mut members = crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == run_id)
        .map(|member| {
            serde_json::json!({
                "id": member.id,
                "status": member.status,
                "coordination_status": member.coordination_status,
                "runtime_generation": member.runtime_generation,
                "zero_output_streak": member.zero_output_streak,
                "last_event_at": member.last_event_at,
                "finished_at": member.finished_at,
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

    let (messages, runtime_commands) = match execution_space_id {
        Some(space_id) => {
            let messages = store
                .fabric_messages(space_id)?
                .into_iter()
                .filter(|message| message.team_run_id.as_deref() == Some(run_id))
                .count();
            let runtime_commands = store
                .runtime_commands(space_id)?
                .into_iter()
                .filter(|command| {
                    matches!(
                        &command.binding.target_driver,
                        RuntimeDriverRef::TeamSupervisor { team_run_id, .. }
                            if team_run_id == run_id
                    )
                })
                .count();
            (Some(messages), Some(runtime_commands))
        }
        None => (None, None),
    };

    Ok(harness_store::canonical_json_fingerprint(
        &serde_json::json!({
            "team_run": {
                "id": run.id,
                "status": run.status,
                "updated_at": run.updated_at,
                "completed_at": run.completed_at,
                "member_run_ids": run.member_run_ids,
            },
            "member_runs": members,
            "work_operations": work_operations,
            "execution_space_id": execution_space_id,
            "messages": messages,
            "runtime_commands": runtime_commands,
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
