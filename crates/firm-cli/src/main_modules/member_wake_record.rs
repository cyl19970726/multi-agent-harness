//! ADR 0078: every non-Sleep wake decision is a durable coordination fact.
//!
//! The member wake loop used to leave no trace of WHY a member ran a cycle.
//! The provider turn, the Work transition and the message ACK were all durable;
//! the decision that caused them was a `WakeDecision` value that lived for one
//! stack frame. Reconstructing "why did this member wake at 03:14?" meant
//! inferring it from the side effects, which is how a wake arm with a
//! misleading name survived long enough to be nearly deleted.
//!
//! This module owns that one fact and nothing else: it is called by
//! `poll_idle_member_wake` and writes team_run events. It reads no store and
//! decides no policy.

use super::*;

/// ADR 0078. The arm that produced this wake, named exactly.
///
/// Most wakes are unambiguous from the wake alone. Three are not, and each
/// ambiguity is load-bearing:
///
/// - `Work` is produced twice: by the eager claim at the top of the poll,
///   before `decide_wake` runs at all, and by the `DeliverPending` arm. Only
///   the presence of a decision tells them apart.
/// - `ActiveWorkContinuation` is produced by THREE arms, one of which is
///   `ClaimBoardWork`. A row calling that one `Continue` would assert the
///   member resumed a Work it owned — it owned nothing a moment ago, and the
///   prompt it gets is the shared-board one. Calling this arm a continuation
///   is precisely how it became invisible enough to nearly delete.
/// - `Acceptance` is produced INSIDE the `Sleep` arm: the pure view cannot see
///   a durable acceptance, so the driver checks for one before sleeping and
///   wakes on it. The decision said Sleep and the member did not sleep, so the
///   wake wins over the decision here.
///
/// `None` means no row: `Degraded` and `CloseRequested` already write their own
/// events at this same instant (`degraded` below, `updated` in
/// `close_member_runtime`), and a second row would break "exactly once per
/// decision". `TestRetired` is not a wake at all.
pub(super) fn wake_decision_arm(
    wake: &IdleMemberWake,
    decision: Option<&supervisor_wake::WakeDecision>,
) -> Option<&'static str> {
    match wake {
        IdleMemberWake::Acceptance(_) => Some("Acceptance"),
        IdleMemberWake::HostAttentions(_) => Some("HostAttentions"),
        IdleMemberWake::Work(_) => Some(match decision {
            None => "EagerClaim",
            Some(_) => "DeliverPending",
        }),
        IdleMemberWake::Messages { .. } => Some(match decision {
            None => "CanonicalMessages",
            Some(supervisor_wake::WakeDecision::DeliverInformational) => "DeliverInformational",
            Some(_) => "DeliverPending",
        }),
        IdleMemberWake::ActiveWorkContinuation(_) => Some(match decision {
            Some(supervisor_wake::WakeDecision::ClaimBoardWork) => "ClaimBoardWork",
            Some(supervisor_wake::WakeDecision::Continue(_)) => "Continue",
            _ => "DeliverPending",
        }),
        IdleMemberWake::Degraded(_)
        | IdleMemberWake::CloseRequested { .. }
        | IdleMemberWake::TestRetired => None,
    }
}

/// ADR 0078. The durable thing this wake is about, in Harness-owned ids only.
///
/// Never provider content: not a prompt, a message body, a Work title, or a
/// completion criterion. A message batch has no id of its own, so it is keyed
/// by its size and its first delivery — enough to find the batch in
/// `team_messages`, nothing quoted from it.
fn wake_trigger_key(wake: &IdleMemberWake) -> String {
    match wake {
        IdleMemberWake::Work(claimed) => {
            format!("work={} version={}", claimed.work.id, claimed.work.version)
        }
        IdleMemberWake::ActiveWorkContinuation(work) => {
            format!("work={} version={}", work.id, work.version)
        }
        IdleMemberWake::Acceptance(wake) => format!(
            "acceptance={} work={}",
            wake.acceptance_event_id, wake.accepted_work_id
        ),
        IdleMemberWake::Messages { messages, .. } => format!(
            "messages={} first={}",
            messages.len(),
            messages
                .first()
                .map(|message| message.id.as_str())
                .unwrap_or("none"),
        ),
        IdleMemberWake::HostAttentions(attentions) => format!(
            "attentions={} first_work={}",
            attentions.len(),
            attentions
                .first()
                .map(|attention| attention.work_id.as_str())
                .unwrap_or("none"),
        ),
        IdleMemberWake::Degraded(_)
        | IdleMemberWake::CloseRequested { .. }
        | IdleMemberWake::TestRetired => String::new(),
    }
}

/// ADR 0078. One row per wake decision, plus the row that closes the idle
/// episode this wake ended.
///
/// The episode row is a separate fact from the decision row and carries what no
/// other row can: how many polls the member spent finding nothing. Without it
/// an idle stretch leaves only its cap row and the reader cannot tell a
/// two-poll gap from a two-hour one.
pub(super) fn record_wake_decision(
    ledger: &TeamRunLedger,
    member_row: &ProviderRuntimeProjection,
    wake: &IdleMemberWake,
    decision: Option<&supervisor_wake::WakeDecision>,
    episode_polls: u32,
) -> CliResult<()> {
    let Some(arm) = wake_decision_arm(wake, decision) else {
        return Ok(());
    };
    if episode_polls > 0 {
        ledger.fold_event(
            TeamRunEventSourceKind::Member,
            Some(member_row.id.clone()),
            "member_run",
            &member_row.id,
            "wake_idle_ended",
            &format!("idle episode ended after {episode_polls} polls: {arm}"),
        )?;
    }
    ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member_row.id.clone()),
        "member_run",
        &member_row.id,
        "wake_decided",
        &format!("{arm} {}", wake_trigger_key(wake)),
    )?;
    Ok(())
}

/// ADR 0078. The one row an idle episode writes while it is still going.
///
/// Written when the backoff first reaches `backoff_max_ms` and not once per
/// tick: past the cap every poll is identical, so sixty rows would say what one
/// says. The latch that makes it once-per-episode lives on the backoff, which
/// is also what clears it when a wake ends the episode.
pub(super) fn record_idle_episode_capped(
    ledger: &TeamRunLedger,
    member_row: &ProviderRuntimeProjection,
    policy: &supervisor_wake::WakePolicy,
    episode_polls: u32,
) -> CliResult<()> {
    ledger.fold_event(
        TeamRunEventSourceKind::Member,
        Some(member_row.id.clone()),
        "member_run",
        &member_row.id,
        "wake_idle_capped",
        &format!(
            "idle episode reached its {}ms poll ceiling after {episode_polls} polls",
            policy.backoff_max_ms
        ),
    )?;
    Ok(())
}
