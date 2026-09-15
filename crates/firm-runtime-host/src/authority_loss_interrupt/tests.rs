//! The registry is process-global, so every case here holds one serializing
//! lock: a `Process`-scoped request would otherwise reach a turn registered by
//! a test running in parallel in the same binary.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn serialized() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn team_scope(team_run_id: &str) -> AuthorityLossScope {
    AuthorityLossScope::TeamRun(team_run_id.to_string())
}

#[test]
fn a_live_turn_observes_exactly_one_cooperative_interrupt() {
    let _serialized = serialized();
    let turn = register_live_provider_turn("kimi", "run-one-interrupt", "member-1");

    let report = request_authority_loss_interrupt(
        &team_scope("run-one-interrupt"),
        "TEAM_SUPERVISOR_LEASE_LOST",
        Duration::ZERO,
    );

    assert_eq!(report.turns_live, 1);
    assert_eq!(
        turn.take_authority_loss_interrupt().as_deref(),
        Some("TEAM_SUPERVISOR_LEASE_LOST")
    );
    assert_eq!(
        turn.take_authority_loss_interrupt(),
        None,
        "a turn must never be interrupted twice by the same latch"
    );
}

#[test]
fn a_second_latch_never_republishes_an_interrupt_the_turn_already_took() {
    let _serialized = serialized();
    let turn = register_live_provider_turn("codex", "run-second-latch", "member-1");

    request_authority_loss_interrupt(&team_scope("run-second-latch"), "first", Duration::ZERO);
    assert_eq!(
        turn.take_authority_loss_interrupt().as_deref(),
        Some("first")
    );
    // The machine latch can follow a Supervisor latch on the same turn.
    let second =
        request_authority_loss_interrupt(&team_scope("run-second-latch"), "second", Duration::ZERO);

    assert_eq!(second.turns_live, 1);
    assert_eq!(second.turns_interrupted, 1, "already dispatched");
    assert_eq!(
        turn.take_authority_loss_interrupt(),
        None,
        "a second latch must not issue a second provider interrupt"
    );
}

#[test]
fn a_turn_driven_by_another_thread_is_reported_dispatched() {
    let _serialized = serialized();
    let polling = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    let thread_polling = Arc::clone(&polling);
    let thread_observed = Arc::clone(&observed);
    let driver = std::thread::spawn(move || {
        let turn = register_live_provider_turn("claude", "run-other-thread", "member-1");
        thread_polling.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if turn.take_authority_loss_interrupt().is_some() {
                thread_observed.store(true, Ordering::Release);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    while !polling.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }

    let report = request_authority_loss_interrupt(
        &team_scope("run-other-thread"),
        "NODE_DAEMON_MACHINE_AUTHORITY_LOST",
        Duration::from_secs(5),
    );
    driver.join().expect("driver thread");

    assert!(observed.load(Ordering::Acquire));
    assert_eq!(report.turns_live, 1);
    assert_eq!(report.turns_interrupted, 1);
    assert_eq!(report.turns[0].provider, "claude");
    assert_eq!(report.turns[0].member_run_id, "member-1");
    assert_eq!(
        report.turns[0].outcome,
        AuthorityLossInterruptOutcome::Dispatched
    );
}

#[test]
fn a_turn_that_never_polls_is_reported_without_blocking_the_drain() {
    let _serialized = serialized();
    let _turn = {
        let registered = Arc::new(Mutex::new(None));
        let thread_registered = Arc::clone(&registered);
        // Registered by another thread so the fan-out actually waits on it.
        std::thread::spawn(move || {
            let turn = register_live_provider_turn("deepseek", "run-hung", "member-1");
            *thread_registered
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(turn);
            std::thread::sleep(Duration::from_secs(2));
        });
        loop {
            if registered
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_some()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        registered
    };

    let started = Instant::now();
    let report = request_authority_loss_interrupt(
        &team_scope("run-hung"),
        "NODE_DAEMON_MACHINE_AUTHORITY_LOST",
        Duration::from_millis(80),
    );
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(900),
        "a hung provider must never hold the drain: {elapsed:?}"
    );
    assert_eq!(report.turns_live, 1);
    assert_eq!(report.turns_interrupted, 0);
    assert_eq!(
        report.turns[0].outcome,
        AuthorityLossInterruptOutcome::NotObserved
    );
}

#[test]
fn a_team_run_latch_never_reaches_another_runs_live_turn() {
    let _serialized = serialized();
    let mine = register_live_provider_turn("kimi", "run-mine", "member-1");
    let theirs = register_live_provider_turn("kimi", "run-theirs", "member-2");

    let report =
        request_authority_loss_interrupt(&team_scope("run-mine"), "lease lost", Duration::ZERO);

    assert_eq!(report.turns_live, 1);
    assert!(mine.take_authority_loss_interrupt().is_some());
    assert_eq!(
        theirs.take_authority_loss_interrupt(),
        None,
        "one run's lease loss must not interrupt another run's turn"
    );
}

#[test]
fn a_process_latch_reaches_every_live_turn() {
    let _serialized = serialized();
    let first = register_live_provider_turn("codex", "run-process-a", "member-1");
    let second = register_live_provider_turn("pi", "run-process-b", "member-2");

    let report = request_authority_loss_interrupt(
        &AuthorityLossScope::Process,
        "NODE_DAEMON_MACHINE_AUTHORITY_LOST",
        Duration::ZERO,
    );

    assert_eq!(report.turns_live, 2);
    assert!(first.take_authority_loss_interrupt().is_some());
    assert!(second.take_authority_loss_interrupt().is_some());
    assert_eq!(report.scope, "process");
}

#[test]
fn a_finished_turn_is_no_longer_interruptible() {
    let _serialized = serialized();
    drop(register_live_provider_turn(
        "codex",
        "run-finished",
        "member-1",
    ));

    let report =
        request_authority_loss_interrupt(&team_scope("run-finished"), "lease lost", Duration::ZERO);

    assert_eq!(report.turns_live, 0);
    assert_eq!(report.turns_interrupted, 0);
}

#[test]
fn a_latch_reached_from_the_turns_own_thread_never_waits_on_itself() {
    let _serialized = serialized();
    let turn = register_live_provider_turn("kimi", "run-self-wait", "member-1");

    let started = Instant::now();
    let report = request_authority_loss_interrupt(
        &team_scope("run-self-wait"),
        "lease lost",
        Duration::from_secs(30),
    );
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "the latch may run inside the turn's own supervisor thread: {elapsed:?}"
    );
    assert_eq!(report.turns_live, 1);
    assert_eq!(report.turns_interrupted, 0, "not polled yet");
    assert!(
        turn.take_authority_loss_interrupt().is_some(),
        "the request stays latched for the turn's next control poll"
    );
}

#[test]
fn the_report_renders_the_journalled_interrupt_evidence() {
    let _serialized = serialized();
    let turn = register_live_provider_turn("kimi", "run-report", "member-1");
    request_authority_loss_interrupt(&team_scope("run-report"), "lease lost", Duration::ZERO);
    assert!(turn.take_authority_loss_interrupt().is_some());
    let report =
        request_authority_loss_interrupt(&team_scope("run-report"), "lease lost", Duration::ZERO);

    let json = report.to_json();

    assert_eq!(json["scope"], "run-report");
    assert_eq!(json["reason"], "lease lost");
    assert_eq!(json["turns_live"], 1);
    assert_eq!(json["turns_interrupted"], 1);
    assert_eq!(json["turns"][0]["provider"], "kimi");
    assert_eq!(json["turns"][0]["member_run_id"], "member-1");
    assert_eq!(json["turns"][0]["outcome"], "dispatched");
}
