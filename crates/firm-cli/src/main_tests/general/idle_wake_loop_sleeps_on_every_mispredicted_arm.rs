use super::*;

/// `decide_wake` predicts from a pure view built before the claim. When the
/// matching claim has already disappeared, the `DeliverPending`, `Continue`
/// and `ClaimHint` arms used to re-enter the loop with no sleep at all, so an
/// idle member re-ran whole-Store scans at 100% CPU (#584).
///
/// The loop body now returns `IdleWakeStep`, and `IdleWakeStep::Retry` is the
/// only way back into the loop. `idle_wake_retry_gate` is the only thing that
/// acts on it, so this test pins the property every arm now inherits: one
/// bounded backoff per re-entry, never a free spin.
#[test]
fn idle_wake_loop_sleeps_on_every_mispredicted_arm() {
    const BACKOFF_MS: u64 = 60;
    let policy = supervisor_wake::WakePolicy {
        zero_output_degradation_threshold: 3,
        backoff_initial_ms: BACKOFF_MS,
        backoff_max_ms: BACKOFF_MS,
        backoff_multiplier: 1.0,
    };
    let mut backoff = supervisor_wake::WakeBackoff::new();
    assert_eq!(backoff.consecutive_sleeps(), 0);

    const RETRIES: u32 = 3;
    let started = Instant::now();
    for expected_sleeps in 1..=RETRIES {
        // `idle_since` is refreshed on every pass so the bounded test idle
        // grace can never retire this member early; the gate must sleep.
        let retired = idle_wake_retry_gate(Instant::now(), &policy, &mut backoff);
        assert!(
            retired.is_none(),
            "a member well inside its idle grace keeps waiting"
        );
        assert_eq!(
            backoff.consecutive_sleeps(),
            expected_sleeps,
            "each mispredicted arm re-enters through exactly one bounded backoff"
        );
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(BACKOFF_MS * u64::from(RETRIES)),
        "the gate slept its backoff on every re-entry rather than spinning: {elapsed:?}"
    );
}
