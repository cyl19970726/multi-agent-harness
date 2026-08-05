//! Pure wake-predicate decision function for the supervisor member loop.
//!
//! ADR 0050: the pull half of the ownership model — a member is only woken
//! when a state-change predicate holds. Idle members may be offered a
//! board-discovery hint for eligible team_claim Works, but ownership starts
//! only at the explicit atomic claim.
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
    /// Unconsumed WorkDelivery or response-required messages are waiting.
    DeliverPending,
    /// Idle member + eligible ready team_claim Work exists → board-discovery
    /// hint. The member must perform the atomic claim itself.
    ClaimHint(Vec<String>),
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
}

impl Default for WakePolicy {
    fn default() -> Self {
        Self {
            zero_output_degradation_threshold: 3,
            backoff_initial_ms: 500,
            backoff_max_ms: 30_000,
            backoff_multiplier: 2.0,
        }
    }
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
/// 5. **Idle + eligible team_claim Works** → `ClaimHint`.
/// 6. **No predicate matches** → `Sleep` with exponential backoff.
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

    // 5. Idle + eligible ready team_claim Works → board-discovery hint.
    if member.is_idle && !board.eligible_claim_work_ids.is_empty() {
        return WakeDecision::ClaimHint(board.eligible_claim_work_ids.clone());
    }

    // 6. No predicate matches → sleep with exponential backoff.
    WakeDecision::Sleep(backoff.current_duration(policy))
}

// ---------------------------------------------------------------------------
// Backoff helper
// ---------------------------------------------------------------------------

/// Trackable exponential backoff state for the wake loop.
#[derive(Debug, Clone)]
pub struct WakeBackoff {
    consecutive_sleeps: u32,
}

impl WakeBackoff {
    pub fn new() -> Self {
        Self {
            consecutive_sleeps: 0,
        }
    }

    /// Number of consecutive Sleep decisions without an intervening wake event.
    #[cfg(test)]
    pub fn consecutive_sleeps(&self) -> u32 {
        self.consecutive_sleeps
    }

    /// Reset the backoff because a real event occurred (Work, delivery, etc.).
    pub fn reset(&mut self) {
        self.consecutive_sleeps = 0;
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
    fn idle_member_with_eligible_claim_work_gets_claim_hint() {
        let member = member_view(MemberWakeViewOverrides {
            status: Some(MemberRunStatus::Idle),
            is_idle: Some(true),
            ..Default::default()
        });
        let board = board_view(&["work-claimable-1", "work-claimable-2"]);
        let backoff = fresh_backoff();
        let decision = decide_wake(&member, &board, &policy(), &backoff);
        assert_eq!(
            decision,
            WakeDecision::ClaimHint(vec!["work-claimable-1".into(), "work-claimable-2".into()])
        );
    }

    #[test]
    fn non_idle_member_does_not_get_claim_hint() {
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
    fn zero_output_streak_below_threshold_does_not_degrade() {
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
        assert!(
            matches!(decision, WakeDecision::Sleep(_)),
            "2 zero-output turns below threshold of 3 should Sleep, got {decision:?}"
        );
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
    fn delivery_takes_priority_over_claim_hint() {
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
    fn continue_takes_priority_over_claim_hint() {
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
}
