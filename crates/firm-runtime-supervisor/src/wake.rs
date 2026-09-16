//! Pure wake-predicate decision function for the supervisor member loop.
//!
//! ADR 0050: the pull half of the ownership model — a member is only woken
//! when a state-change predicate holds. Idle members may be offered a
//! woken for an unclaimed, eligible team_claim Work on the board, but
//! ownership starts only at the explicit atomic claim.
//!
//! This module is free of I/O so it is unit-testable. View structs are built
//! from store reads in the caller; `decide_wake` produces a `WakeDecision`.

use std::time::Duration;

use harness_core::MemberRunStatus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Decision returned by the pure wake-predicate function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeDecision {
    /// Continue the member's active in_progress Work (work_id).
    Continue(String),
    /// Unconsumed canonical Work delivery or response-required messages are waiting.
    DeliverPending,
    /// The member has been idle with nothing else to run for at least
    /// `informational_idle_delivery_ms`, and informational mail is queued for
    /// it. The queued batch is delivered on its own Messages boundary rather
    /// than waiting for a cycle that may never come (ADR 0077).
    DeliverInformational,
    /// An idle member has an unclaimed, ready `team_claim` Work it is eligible
    /// for. This is the ONLY wake that reaches board Work: every other path —
    /// the eager claim, `DeliverPending`, `Continue` — filters on
    /// `owner_member_id == this member`, and an unclaimed board Work has no
    /// owner yet.
    ///
    /// It carried a `Vec<String>` of work ids until ADR 0078. The driver
    /// discarded them and re-derived the Work through the ordinary
    /// continuation path, so the payload described a board-discovery hint that
    /// was never delivered to anyone. The payload is gone; the wake is not.
    ClaimBoardWork,
    /// Nothing to do; sleep for this duration.
    Sleep(Duration),
    /// Member is degraded (zero-output spiral). Stop continuation injections;
    /// the Host must intervene (message, steer, or recover).
    Degraded(String),
}

/// Pure view of a single member for the wake decision.
#[derive(Debug, Clone)]
pub struct MemberWakeView {
    pub member_id: String,
    pub status: MemberRunStatus,
    /// Whether the member is currently idle (not running a turn).
    pub is_idle: bool,
    /// The member's active in_progress Work, if any.
    pub active_work_id: Option<String>,
    /// Current durable version of the active Work.
    pub active_work_version: Option<u64>,
    /// Last version the member consumed (seen at the start of its last turn).
    /// When this differs from `active_work_version`, the Work has been updated.
    pub last_consumed_work_version: Option<u64>,
    /// Number of queued WorkDeliveries targeting this member.
    pub unconsumed_delivery_count: u32,
    /// Number of response-required messages queued for this member.
    pub unconsumed_message_count: u32,
    /// Number of queued messages for this member that do NOT trigger a round
    /// on their own — ordinary informational mail. Counted separately from
    /// `unconsumed_message_count` because the two have different guarantees:
    /// a response-required message wakes immediately, an informational one is
    /// folded into the next cycle that runs for another reason and, failing
    /// that, gets its own boundary after an idle interval (ADR 0077).
    pub unconsumed_informational_message_count: u32,
    /// How long this member has been continuously idle, from the durable
    /// MemberRun row rather than a process-local counter, so a daemon
    /// generation change does not reset the clock and hide waiting mail.
    /// `None` when the member is not idle or the row carries no usable stamp.
    pub idle_for: Option<Duration>,
    /// Consecutive provider turns with zero tool calls AND no Work transition.
    pub zero_output_streak: u32,
}

/// Pure view of the shared board for the wake decision.
#[derive(Debug, Clone)]
pub struct BoardWakeView {
    /// IDs of ready team_claim Works the member is eligible to claim.
    pub eligible_claim_work_ids: Vec<String>,
}

/// Configurable wake policy.
#[derive(Debug, Clone)]
pub struct WakePolicy {
    /// After this many consecutive zero-output turns, the member is degraded.
    pub zero_output_degradation_threshold: u32,
    /// Starting backoff duration in milliseconds.
    pub backoff_initial_ms: u64,
    /// Maximum backoff duration in milliseconds.
    pub backoff_max_ms: u64,
    /// Backoff multiplier (doubling per consecutive sleep).
    pub backoff_multiplier: f64,
    /// How long a member must have been continuously idle, with nothing else
    /// to run, before queued informational mail earns its own delivery
    /// boundary (ADR 0077).
    pub informational_idle_delivery_ms: u64,
}

impl Default for WakePolicy {
    fn default() -> Self {
        Self {
            zero_output_degradation_threshold: 3,
            backoff_initial_ms: 500,
            backoff_max_ms: 30_000,
            backoff_multiplier: 2.0,
            // 120s. Long enough that a member about to be given Work is not
            // interrupted by mail one cycle early — the ordinary path is still
            // #941's fold into the next cycle, and this only fires when that
            // cycle never comes. Short enough that a Host note to an idle
            // member is not left for the tens of minutes measured in the S1
            // dogfood, where 60 informational Messages never reached a
            // provider at all. See ADR 0077 for the latency arithmetic.
            informational_idle_delivery_ms: 120_000,
        }
    }
}

/// The one wake policy every managed member actually runs under.
///
/// Single source for both consumers of the degradation threshold: the wake
/// loop itself (`run_team_member_with_adapter` in runtime_adapter.rs
/// constructs each member's policy here) and the `team-run recover`
/// blocked-member classifier (`zero_output_degradation_threshold` in
/// drain_lane_resume.rs), which must agree with the gate it defers to. This
/// is deliberately not a configurable policy — do not thread one through the
/// runtime; a per-run override would silently desync the classifier.
pub fn effective_wake_policy() -> WakePolicy {
    WakePolicy::default()
}

// ---------------------------------------------------------------------------
// Pure decision function
// ---------------------------------------------------------------------------

/// Decide what action the supervisor should take for a member.
///
/// Predicates are evaluated in priority order:
///
/// 1. **Already degraded** → keep sleeping (no re-wake).
/// 2. **Zero-output degradation** → member has hit the threshold → `Degraded`.
/// 3. **Delivery/message pending** → `DeliverPending`.
/// 4. **Active Work version changed** → `Continue`.
/// 5. **Zero-output probation + active Work** → one bounded continuation so
///    the threshold can be observed instead of stalling after the first turn.
/// 6. **Idle + an unclaimed eligible team_claim Work** → `ClaimBoardWork`.
/// 7. **Idle ≥ the informational delivery interval + informational mail
///    queued** → `DeliverInformational` (ADR 0077). Below every arm that
///    already runs a cycle, because such a cycle carries the mail anyway;
///    above `Sleep`, because otherwise the mail never moves.
/// 8. **No predicate matches** → `Sleep` with exponential backoff.
///
/// Never wakes for Work in review/blocked/done/cancelled status.
pub fn decide_wake(
    member: &MemberWakeView,
    board: &BoardWakeView,
    policy: &WakePolicy,
    backoff: &WakeBackoff,
) -> WakeDecision {
    // 1. Already degraded → keep sleeping, no further action.
    if member.status == MemberRunStatus::Blocked
        && member.zero_output_streak >= policy.zero_output_degradation_threshold
    {
        return WakeDecision::Sleep(backoff.current_duration(policy));
    }

    // 2. Zero-output streak hit threshold → degrade the member.
    if member.zero_output_streak >= policy.zero_output_degradation_threshold {
        return WakeDecision::Degraded(format!(
            "member {} had {} consecutive zero-output turns (no tool calls, no Work transition)",
            member.member_id, member.zero_output_streak
        ));
    }

    // 3. Unconsumed delivery or response-required messages → deliver.
    if member.unconsumed_delivery_count > 0 || member.unconsumed_message_count > 0 {
        return WakeDecision::DeliverPending;
    }

    // 4. Active Work version changed since the member last consumed it → continue.
    if let (Some(ref active_id), Some(active_version), Some(last_consumed)) = (
        member.active_work_id.as_ref(),
        member.active_work_version,
        member.last_consumed_work_version,
    ) {
        if active_version != last_consumed {
            return WakeDecision::Continue(active_id.to_string());
        }
    }

    // 5. A degradation threshold greater than one requires bounded probation
    // turns. Without this predicate, the first empty provider turn would have
    // no state-change wake and could never reach the configured threshold.
    if member.zero_output_streak > 0 {
        if let Some(active_id) = member.active_work_id.as_ref() {
            return WakeDecision::Continue(active_id.clone());
        }
    }

    // 6. Idle + an unclaimed ready team_claim Work this member may take.
    if member.is_idle && !board.eligible_claim_work_ids.is_empty() {
        return WakeDecision::ClaimBoardWork;
    }

    // 7. Idle long enough, with informational mail queued and no other reason
    // to run → give that mail its own boundary (ADR 0077).
    //
    // Placed HERE, below every arm that already has a reason to run a cycle,
    // because #941 folds queued mail into any such cycle for free: a member
    // with Work, a continuation, an acceptance or a board claim receives its
    // mail without this arm, and firing earlier would only interrupt it one
    // cycle sooner. This arm exists for the case that path cannot reach — an
    // idle member with no Work, where the fold never happens because no cycle
    // ever runs. Placed ABOVE Sleep because otherwise there is no wake at all.
    if member.is_idle
        && member.unconsumed_informational_message_count > 0
        && member.idle_for.is_some_and(|idle| {
            idle >= Duration::from_millis(policy.informational_idle_delivery_ms)
        })
    {
        return WakeDecision::DeliverInformational;
    }

    // 8. No predicate matches → sleep with exponential backoff.
    WakeDecision::Sleep(backoff.current_duration(policy))
}

// ---------------------------------------------------------------------------
// Backoff helper
// ---------------------------------------------------------------------------

/// Trackable exponential backoff state for the wake loop.
///
/// `consecutive_sleeps` is also the length of the current **idle episode**: the
/// unbroken run of polls that found nothing to do. ADR 0078 records an idle
/// episode with two durable rows rather than one per poll, and both are derived
/// from this counter — `cap_recorded` latches the first so the cap row cannot
/// be written twice for the same episode. No clock lives here: the crate stays
/// I/O-free and the episode is measured in polls, not wall time.
#[derive(Debug, Clone)]
pub struct WakeBackoff {
    consecutive_sleeps: u32,
    cap_recorded: bool,
}

impl WakeBackoff {
    pub fn new() -> Self {
        Self {
            consecutive_sleeps: 0,
            cap_recorded: false,
        }
    }

    /// Number of consecutive Sleep decisions without an intervening wake event,
    /// i.e. how many polls the current idle episode has lasted.
    pub fn consecutive_sleeps(&self) -> u32 {
        self.consecutive_sleeps
    }

    /// The backoff has stopped growing: this episode now polls at
    /// `backoff_max_ms` and every further tick is indistinguishable from the
    /// last. One row at this point describes the whole tail (ADR 0078).
    pub fn at_cap(&self, policy: &WakePolicy) -> bool {
        self.consecutive_sleeps > 0
            && self.current_duration(policy) >= Duration::from_millis(policy.backoff_max_ms)
    }

    /// Whether the cap row has already been written for THIS episode.
    pub fn cap_recorded(&self) -> bool {
        self.cap_recorded
    }

    /// Latch the cap row for this episode. Cleared by `reset`, so the next
    /// idle episode records its own cap exactly once.
    pub fn mark_cap_recorded(&mut self) {
        self.cap_recorded = true;
    }

    /// Reset the backoff because a real event occurred (Work, delivery, etc.).
    /// This is also the end of the idle episode, so the cap latch clears with
    /// it.
    pub fn reset(&mut self) {
        self.consecutive_sleeps = 0;
        self.cap_recorded = false;
    }

    /// Record one more sleep cycle.
    pub fn tick(&mut self) {
        self.consecutive_sleeps = self.consecutive_sleeps.saturating_add(1);
    }

    /// Current backoff duration given the policy.
    pub fn current_duration(&self, policy: &WakePolicy) -> Duration {
        backoff_duration(policy, self.consecutive_sleeps)
    }

    /// Sleep for the current backoff duration, then tick the counter.
    pub fn sleep_and_tick(&mut self, policy: &WakePolicy) {
        let d = self.current_duration(policy);
        std::thread::sleep(d);
        self.tick();
    }
}

impl Default for WakeBackoff {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute exponential backoff duration, capped at the policy maximum.
pub fn backoff_duration(policy: &WakePolicy, consecutive_sleeps: u32) -> Duration {
    if consecutive_sleeps == 0 {
        return Duration::from_millis(policy.backoff_initial_ms);
    }
    let exponent = consecutive_sleeps.min(31); // prevent overflow
    let multiplier = policy.backoff_multiplier.powi(exponent as i32);
    let ms =
        (policy.backoff_initial_ms as f64 * multiplier).min(policy.backoff_max_ms as f64) as u64;
    Duration::from_millis(ms)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn member_view(overrides: MemberWakeViewOverrides) -> MemberWakeView {
        let o = overrides;
        MemberWakeView {
            member_id: "member-1".into(),
            status: o.status.unwrap_or(MemberRunStatus::Idle),
            is_idle: o.is_idle.unwrap_or(true),
            active_work_id: o.active_work_id,
            active_work_version: o.active_work_version,
            last_consumed_work_version: o.last_consumed_work_version,
            unconsumed_delivery_count: o.unconsumed_delivery_count.unwrap_or(0),
            unconsumed_message_count: o.unconsumed_message_count.unwrap_or(0),
            unconsumed_informational_message_count: o
                .unconsumed_informational_message_count
                .unwrap_or(0),
            idle_for: o.idle_for,
            zero_output_streak: o.zero_output_streak.unwrap_or(0),
        }
    }

    #[derive(Default)]
    struct MemberWakeViewOverrides {
        status: Option<MemberRunStatus>,
        is_idle: Option<bool>,
        active_work_id: Option<String>,
        active_work_version: Option<u64>,
        last_consumed_work_version: Option<u64>,
        unconsumed_delivery_count: Option<u32>,
        unconsumed_message_count: Option<u32>,
        unconsumed_informational_message_count: Option<u32>,
        idle_for: Option<Duration>,
        zero_output_streak: Option<u32>,
    }

    fn board_view(eligible_ids: &[&str]) -> BoardWakeView {
        BoardWakeView {
            eligible_claim_work_ids: eligible_ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn policy() -> WakePolicy {
        WakePolicy::default()
    }

    fn fresh_backoff() -> WakeBackoff {
        WakeBackoff::new()
    }

    // ── Wake predicate tests ──────────────────────────────────────────────

    #[test]
    fn no_predicates_defaults_to_sleep() {
        let member = member_view(Default::default());
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert!(
            matches!(decision, WakeDecision::Sleep(_)),
            "no predicates should produce Sleep, got {decision:?}"
        );
    }

    #[test]
    fn unconsumed_delivery_triggers_deliver_pending() {
        let member = member_view(MemberWakeViewOverrides {
            unconsumed_delivery_count: Some(1),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::DeliverPending);
    }

    #[test]
    fn unconsumed_message_triggers_deliver_pending() {
        let member = member_view(MemberWakeViewOverrides {
            unconsumed_message_count: Some(2),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::DeliverPending);
    }

    #[test]
    fn active_work_version_changed_triggers_continue() {
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(5),
            last_consumed_work_version: Some(3),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::Continue("work-1".into()));
    }

    #[test]
    fn active_work_version_unchanged_does_not_trigger_continue() {
        // This is the 78%-empty-wake case from the PR description:
        // Work version unchanged, no deliveries → Sleep.
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(3),
            last_consumed_work_version: Some(3),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert!(
            matches!(decision, WakeDecision::Sleep(_)),
            "unchanged Work version with no deliveries should Sleep, got {decision:?}"
        );
    }

    #[test]
    fn idle_member_with_eligible_claim_work_is_woken_for_the_board() {
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(true),
            ..Default::default()
        });
        let board = board_view(&["work-claimable-1", "work-claimable-2"]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::ClaimBoardWork);
    }

    #[test]
    fn non_idle_member_is_not_woken_for_the_board() {
        // A member with active work is not idle even if claimable Works exist.
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Running),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(1),
            last_consumed_work_version: Some(1),
            ..Default::default()
        });
        let board = board_view(&["work-claimable-1"]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert!(matches!(decision, WakeDecision::Sleep(_)));
    }

    #[test]
    fn zero_output_streak_hits_threshold_triggers_degraded() {
        let policy = WakePolicy {
            zero_output_degradation_threshold: 3,
            ..WakePolicy::default()
        };
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(3),
            last_consumed_work_version: Some(3),
            zero_output_streak: Some(3),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy, &backoff);
        assert!(
            matches!(decision, WakeDecision::Degraded(_)),
            "3 zero-output turns should degrade, got {decision:?}"
        );
    }

    #[test]
    fn zero_output_streak_below_threshold_retries_active_work() {
        let policy = WakePolicy {
            zero_output_degradation_threshold: 3,
            ..WakePolicy::default()
        };
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(3),
            last_consumed_work_version: Some(3),
            zero_output_streak: Some(2),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy, &backoff);
        assert_eq!(decision, WakeDecision::Continue("work-1".into()));
    }

    #[test]
    fn already_degraded_stays_sleeping() {
        let policy = WakePolicy {
            zero_output_degradation_threshold: 3,
            ..WakePolicy::default()
        };
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Blocked),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(3),
            last_consumed_work_version: Some(3),
            zero_output_streak: Some(3),
            ..Default::default()
        });
        let board = board_view(&[]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy, &backoff);
        assert!(
            matches!(decision, WakeDecision::Sleep(_)),
            "already degraded (Blocked) member should keep sleeping, got {decision:?}"
        );
    }

    #[test]
    fn delivery_takes_priority_over_board_claim() {
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(true),
            unconsumed_delivery_count: Some(1),
            ..Default::default()
        });
        let board = board_view(&["work-claimable-1"]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::DeliverPending);
    }

    #[test]
    fn continue_takes_priority_over_board_claim() {
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(true),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(5),
            last_consumed_work_version: Some(3),
            ..Default::default()
        });
        let board = board_view(&["work-claimable-1"]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(decision, WakeDecision::Continue("work-1".into()));
    }

    // ── Multi-turn behavioral tests ──────────────────────────────────────

    /// Simulate escalation: streak 1 → Continue (probation), streak 2 →
    /// Continue (probation), streak 3 → Degraded (threshold hit).
    #[test]
    fn zero_output_streak_escalates_to_degraded_across_turns() {
        let policy = WakePolicy {
            zero_output_degradation_threshold: 3,
            ..WakePolicy::default()
        };
        let board = board_view(&[]);

        // Turn 1: streak 1, below threshold, active work exists → Continue.
        {
            let member = member_view(MemberWakeViewOverrides {
                status: Some(MemberRunStatus::Running),
                is_idle: Some(false),
                active_work_id: Some("work-1".into()),
                active_work_version: Some(3),
                last_consumed_work_version: Some(3),
                zero_output_streak: Some(1),
                ..Default::default()
            });
            let decision = decide_wake(&member, &board, &policy, &fresh_backoff());
            assert_eq!(
                decision,
                WakeDecision::Continue("work-1".into()),
                "streak 1 should Continue (probation)"
            );
        }

        // Turn 2: streak 2, still below threshold → Continue.
        {
            let member = member_view(MemberWakeViewOverrides {
                status: Some(MemberRunStatus::Running),
                is_idle: Some(false),
                active_work_id: Some("work-1".into()),
                active_work_version: Some(3),
                last_consumed_work_version: Some(3),
                zero_output_streak: Some(2),
                ..Default::default()
            });
            let decision = decide_wake(&member, &board, &policy, &fresh_backoff());
            assert_eq!(
                decision,
                WakeDecision::Continue("work-1".into()),
                "streak 2 should Continue (probation)"
            );
        }

        // Turn 3: streak 3, hits threshold → Degraded.
        {
            let member = member_view(MemberWakeViewOverrides {
                status: Some(MemberRunStatus::Running),
                is_idle: Some(false),
                active_work_id: Some("work-1".into()),
                active_work_version: Some(3),
                last_consumed_work_version: Some(3),
                zero_output_streak: Some(3),
                ..Default::default()
            });
            let decision = decide_wake(&member, &board, &policy, &fresh_backoff());
            assert!(
                matches!(decision, WakeDecision::Degraded(_)),
                "streak 3 should Degrade, got {decision:?}"
            );
        }
    }

    /// Degraded member must stay sleeping even when new unconsumed deliveries
    /// arrive. Predicate 1 (already degraded) beats predicate 3 (delivery).
    #[test]
    fn degraded_member_ignores_unconsumed_deliveries() {
        let policy = WakePolicy {
            zero_output_degradation_threshold: 3,
            ..WakePolicy::default()
        };
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Blocked),
            is_idle: Some(false),
            active_work_id: Some("work-1".into()),
            active_work_version: Some(3),
            last_consumed_work_version: Some(3),
            zero_output_streak: Some(3),
            unconsumed_delivery_count: Some(2),
            ..Default::default()
        });
        let board = board_view(&[]);
        let decision = decide_wake(&member, &board, &policy, &fresh_backoff());
        assert!(
            matches!(decision, WakeDecision::Sleep(_)),
            "degraded (Blocked + streak>=threshold) with deliveries must Sleep, got {decision:?}"
        );
    }

    /// When the version is already consumed, Continue is suppressed — but
    /// unconsumed deliveries still wake the member for delivery (predicate 3
    /// fires before predicate 4's version check).
    #[test]
    fn consumed_version_suppresses_continue_but_not_delivery() {
        let policy = WakePolicy::default();

        // Case A: consumed version + no deliveries + no streaks → Sleep.
        {
            let member = member_view(MemberWakeViewOverrides {
                status: Some(MemberRunStatus::Running),
                is_idle: Some(false),
                active_work_id: Some("work-1".into()),
                active_work_version: Some(5),
                last_consumed_work_version: Some(5),
                zero_output_streak: Some(0),
                ..Default::default()
            });
            let decision = decide_wake(&member, &board_view(&[]), &policy, &fresh_backoff());
            assert!(
                matches!(decision, WakeDecision::Sleep(_)),
                "consumed version + no deliveries should Sleep (Continue suppressed), got {decision:?}"
            );
        }

        // Case B: consumed version + deliveries → DeliverPending (delivery
        // wakes even though the version is stale).
        {
            let member = member_view(MemberWakeViewOverrides {
                status: Some(MemberRunStatus::Running),
                is_idle: Some(false),
                active_work_id: Some("work-1".into()),
                active_work_version: Some(5),
                last_consumed_work_version: Some(5),
                unconsumed_delivery_count: Some(1),
                ..Default::default()
            });
            let decision = decide_wake(&member, &board_view(&[]), &policy, &fresh_backoff());
            assert_eq!(
                decision,
                WakeDecision::DeliverPending,
                "consumed version + deliveries should DeliverPending"
            );
        }
    }

    // ── Backoff tests ────────────────────────────────────────────────────

    #[test]
    fn backoff_starts_at_initial() {
        let policy = WakePolicy {
            backoff_initial_ms: 500,
            ..WakePolicy::default()
        };
        let d = backoff_duration(&policy, 0);
        assert_eq!(d, Duration::from_millis(500));
    }

    #[test]
    fn backoff_doubles_each_sleep() {
        let policy = WakePolicy {
            backoff_initial_ms: 500,
            backoff_max_ms: 30_000,
            backoff_multiplier: 2.0,
            ..WakePolicy::default()
        };
        assert_eq!(backoff_duration(&policy, 1), Duration::from_millis(1000));
        assert_eq!(backoff_duration(&policy, 2), Duration::from_millis(2000));
        assert_eq!(backoff_duration(&policy, 3), Duration::from_millis(4000));
        assert_eq!(backoff_duration(&policy, 4), Duration::from_millis(8000));
        assert_eq!(backoff_duration(&policy, 5), Duration::from_millis(16000));
    }

    #[test]
    fn backoff_caps_at_max() {
        let policy = WakePolicy {
            backoff_initial_ms: 500,
            backoff_max_ms: 30_000,
            backoff_multiplier: 2.0,
            ..WakePolicy::default()
        };
        assert_eq!(backoff_duration(&policy, 6), Duration::from_millis(30_000));
        assert_eq!(backoff_duration(&policy, 10), Duration::from_millis(30_000));
    }

    #[test]
    fn backoff_resets_on_event() {
        let mut backoff = WakeBackoff::new();
        let policy = WakePolicy::default();

        // Simulate three sleep cycles.
        backoff.tick();
        backoff.tick();
        assert!(backoff.consecutive_sleeps() >= 2);

        // Reset on real wake event.
        backoff.reset();
        assert_eq!(backoff.consecutive_sleeps(), 0);

        // Next sleep starts fresh.
        let d = backoff.current_duration(&policy);
        assert_eq!(d, Duration::from_millis(500));
    }

    // -----------------------------------------------------------------------
    // ADR 0078: an idle episode is two durable rows, not one per poll.
    // -----------------------------------------------------------------------

    #[test]
    fn idle_episode_reaches_its_cap_once_and_rearms_on_the_next_episode() {
        let policy = WakePolicy::default();
        let mut backoff = WakeBackoff::new();

        // A fresh episode is not at its cap, and a poll that has not slept at
        // all is not an episode.
        assert!(!backoff.at_cap(&policy));
        assert!(!backoff.cap_recorded());

        // 500ms doubling to a 30s ceiling reaches the cap on the 6th tick.
        for _ in 0..5 {
            backoff.tick();
            assert!(
                !backoff.at_cap(&policy),
                "still growing at {} sleeps",
                backoff.consecutive_sleeps()
            );
        }
        backoff.tick();
        assert_eq!(backoff.consecutive_sleeps(), 6);
        assert!(backoff.at_cap(&policy));
        assert!(
            !backoff.cap_recorded(),
            "reaching the cap does not itself write the row"
        );

        // The latch makes the row once-per-episode: every further tick is at
        // the cap and none of them may write again.
        backoff.mark_cap_recorded();
        for _ in 0..50 {
            backoff.tick();
            assert!(backoff.at_cap(&policy));
            assert!(backoff.cap_recorded());
        }

        // The wake that ends the episode clears both, so the NEXT idle stretch
        // records its own cap exactly once rather than staying silent forever.
        backoff.reset();
        assert_eq!(backoff.consecutive_sleeps(), 0);
        assert!(!backoff.at_cap(&policy));
        assert!(!backoff.cap_recorded());
    }

    // -----------------------------------------------------------------------
    // ADR 0077: informational mail gets its own boundary once a member has
    // been idle long enough that no other cycle is coming.

    fn informational_view(idle_ms: u64, informational: u32) -> MemberWakeView {
        member_view(MemberWakeViewOverrides {
            unconsumed_informational_message_count: Some(informational),
            idle_for: Some(Duration::from_millis(idle_ms)),
            ..Default::default()
        })
    }

    #[test]
    fn informational_mail_earns_a_boundary_at_the_policy_interval_and_not_before() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        let n = policy.informational_idle_delivery_ms;

        // One millisecond short of the interval: still asleep. The guarantee
        // is an upper bound on latency, not an excuse to interrupt early.
        assert_eq!(
            decide_wake(&informational_view(n - 1, 1), &board, &policy, &backoff),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
        // Exactly at the interval, and beyond it.
        assert_eq!(
            decide_wake(&informational_view(n, 1), &board, &policy, &backoff),
            WakeDecision::DeliverInformational
        );
        assert_eq!(
            decide_wake(&informational_view(n * 5, 3), &board, &policy, &backoff),
            WakeDecision::DeliverInformational
        );
    }

    #[test]
    fn an_idle_member_with_no_informational_mail_still_sleeps() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        assert_eq!(
            decide_wake(
                &informational_view(policy.informational_idle_delivery_ms * 10, 0),
                &board,
                &policy,
                &backoff
            ),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
    }

    #[test]
    fn response_required_mail_still_wakes_immediately_and_outranks_the_idle_arm() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        // Both kinds queued, idle well past the interval: the response-required
        // arm must win, because it wakes at once and carries the rest with it.
        // A member is never made to wait N minutes for mail that already has a
        // wake reason.
        let view = member_view(MemberWakeViewOverrides {
            unconsumed_message_count: Some(1),
            unconsumed_informational_message_count: Some(2),
            idle_for: Some(Duration::from_millis(
                policy.informational_idle_delivery_ms * 10,
            )),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&view, &board, &policy, &backoff),
            WakeDecision::DeliverPending
        );
    }

    #[test]
    fn a_busy_member_never_takes_the_idle_informational_arm() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        // `is_idle` false with no idle clock: the member is mid-turn, and
        // #941 will fold this mail into the boundary it is already heading for.
        let running = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Running),
            is_idle: Some(false),
            unconsumed_informational_message_count: Some(4),
            idle_for: None,
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&running, &board, &policy, &backoff),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
        // Belt and braces: even if an idle clock were somehow present, the
        // predicate still requires `is_idle`.
        let running_with_clock = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Running),
            is_idle: Some(false),
            unconsumed_informational_message_count: Some(4),
            idle_for: Some(Duration::from_millis(
                policy.informational_idle_delivery_ms * 10,
            )),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&running_with_clock, &board, &policy, &backoff),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
    }

    #[test]
    fn every_arm_with_a_reason_to_run_outranks_the_idle_informational_arm() {
        let policy = WakePolicy::default();
        let backoff = WakeBackoff::new();
        let long_idle = Duration::from_millis(policy.informational_idle_delivery_ms * 10);

        // A queued Work delivery.
        let with_work = member_view(MemberWakeViewOverrides {
            unconsumed_delivery_count: Some(1),
            unconsumed_informational_message_count: Some(1),
            idle_for: Some(long_idle),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&with_work, &board_view(&[]), &policy, &backoff),
            WakeDecision::DeliverPending
        );

        // A changed active Work version.
        let with_continuation = member_view(MemberWakeViewOverrides {
            active_work_id: Some("work-1".into()),
            active_work_version: Some(2),
            last_consumed_work_version: Some(1),
            unconsumed_informational_message_count: Some(1),
            idle_for: Some(long_idle),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&with_continuation, &board_view(&[]), &policy, &backoff),
            WakeDecision::Continue("work-1".into())
        );

        // An eligible claimable Work on the board.
        let with_board_work = member_view(MemberWakeViewOverrides {
            unconsumed_informational_message_count: Some(1),
            idle_for: Some(long_idle),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(
                &with_board_work,
                &board_view(&["work-2"]),
                &policy,
                &backoff
            ),
            WakeDecision::ClaimBoardWork
        );

        // Each of those cycles carries the mail for free (#941), which is why
        // the idle arm sits below them.
    }

    #[test]
    fn a_degraded_member_is_not_woken_by_informational_mail() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        // Degradation means the Host must intervene; mail must not restart a
        // member the loop has deliberately stopped driving.
        let degraded = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Blocked),
            zero_output_streak: Some(policy.zero_output_degradation_threshold),
            unconsumed_informational_message_count: Some(3),
            idle_for: Some(Duration::from_millis(
                policy.informational_idle_delivery_ms * 10,
            )),
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&degraded, &board, &policy, &backoff),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
    }

    #[test]
    fn an_unknown_idle_clock_never_fires_the_arm() {
        let policy = WakePolicy::default();
        let board = board_view(&[]);
        let backoff = WakeBackoff::new();
        // A MemberRun row with no usable `last_event_at` yields `None`. Fail
        // closed: no clock, no wake — never treat "unknown" as "long enough".
        let view = member_view(MemberWakeViewOverrides {
            unconsumed_informational_message_count: Some(2),
            idle_for: None,
            ..Default::default()
        });
        assert_eq!(
            decide_wake(&view, &board, &policy, &backoff),
            WakeDecision::Sleep(backoff.current_duration(&policy))
        );
    }
}
