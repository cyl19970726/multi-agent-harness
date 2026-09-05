use super::*;

use crate::completed_run_members::{
    is_unclosed_managed_member, unclosed_managed_member_count, CompletedRunServingIdler,
    ServingObservation, SERVED_LEDGER_DECODES,
};
use std::sync::atomic::Ordering;

fn decodes() -> u64 {
    SERVED_LEDGER_DECODES.load(Ordering::Relaxed)
}

fn complete_team_run(store: &HarnessStore, run_id: &str) {
    let current = crate::latest_team_run(store, run_id).expect("read TeamRun");
    let mut completed = current.clone();
    completed.status = harness_core::TeamRunStatus::Completed;
    completed.updated_at = crate::now_string();
    completed.completed_at = Some(crate::now_string());
    store
        .compare_and_append_team_run_lifecycle(&current, &completed)
        .expect("complete the TeamRun");
}

/// A Completed TeamRun keeps its Supervisor only so `close-member` still has a
/// live provider-loop authority (#812). The loop that serves it used to
/// re-decode `member_runs.jsonl` and `team_runs.jsonl` in full every second;
/// in the dogfood Execution Space that is ~14 MB of JSONL per second per
/// served run, and it is what starved the daemon's own Supervisor heartbeat
/// off the store write lock until the NodeDaemon lost machine authority and
/// self-stopped (#836).
///
/// This drives the exact gate that loop drives: no decode at all while the
/// ledgers are byte-identical, a full interval of quiet, and a wake well
/// inside one interval as soon as the CAS write a Close performs on the
/// member row lands.
#[test]
fn idle_completed_run_serving_decodes_no_ledger_until_a_close_lands() {
    const IDLE_INTERVAL: Duration = Duration::from_millis(600);
    const IDLE_TICKS: usize = 5;

    let (store, root) = temp_store("completed-run-serving-idle");
    let created = create_two_member_team_run(&store);
    let run_id = created.team_run.id.clone();
    complete_team_run(&store, &run_id);

    let mut idler = CompletedRunServingIdler::new(IDLE_INTERVAL);

    // The pass that discovers the Completed run still decodes; that
    // observation is what the idle ticks are then allowed to reuse.
    let before = decodes();
    let ServingObservation::Rescanned {
        members,
        run_status,
    } = idler
        .observe(&store, &run_id, false)
        .expect("first serving pass")
    else {
        panic!("a pass that is not idle completed-serving must decode the ledgers");
    };
    assert_eq!(run_status, harness_core::TeamRunStatus::Completed);
    let unclosed = unclosed_managed_member_count(&members, &run_id);
    assert!(
        unclosed > 0,
        "the completed run must still have unclosed managed members to serve"
    );
    assert_eq!(decodes() - before, 1, "the first pass decodes exactly once");

    // N idle ticks with nothing written at all.
    for tick in 0..IDLE_TICKS {
        assert!(
            matches!(
                idler
                    .observe(&store, &run_id, true)
                    .expect("idle serving tick"),
                ServingObservation::Unchanged
            ),
            "idle tick {tick} must not re-decode an unchanged ledger"
        );
    }
    assert_eq!(
        decodes() - before,
        1,
        "an idle completed-run serving loop performs zero further ledger decodes"
    );

    // A quiet store is left alone for the whole interval.
    let quiet = Instant::now();
    idler.wait_for_ledger_change(&store);
    assert!(
        quiet.elapsed() >= IDLE_INTERVAL,
        "a quiet store must be left alone for the whole interval, waited {:?}",
        quiet.elapsed()
    );

    // A Close is an ordinary CLI CAS write on the member row. It must still be
    // served within one interval, otherwise the last Close could not release
    // the Supervisor lease promptly.
    let member = members
        .iter()
        .find(|member| is_unclosed_managed_member(member, &run_id))
        .expect("unclosed managed member")
        .clone();
    let closer = {
        let store = store.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut closed = member.clone();
            closed.coordination_status = harness_core::MemberCoordinationStatus::Closed;
            closed.status = harness_core::MemberRunStatus::Stopped;
            store
                .compare_and_append_member_run(&member, &closed)
                .expect("record the member Close");
        })
    };
    let waiting = Instant::now();
    idler.wait_for_ledger_change(&store);
    let waited = waiting.elapsed();
    closer.join().expect("Close writer thread");
    assert!(
        waited < IDLE_INTERVAL,
        "a Close must be served within one interval, waited {waited:?}"
    );

    let ServingObservation::Rescanned {
        members: after_close,
        ..
    } = idler
        .observe(&store, &run_id, true)
        .expect("post-Close serving pass")
    else {
        panic!("a changed ledger must be re-decoded even in the idle branch");
    };
    assert_eq!(
        unclosed_managed_member_count(&after_close, &run_id),
        unclosed - 1,
        "the Close is observed by the next idle tick"
    );

    std::fs::remove_dir_all(root).expect("remove test store");
}
