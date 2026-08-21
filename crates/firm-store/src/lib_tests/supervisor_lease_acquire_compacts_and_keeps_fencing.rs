use super::*;

/// Compaction on acquire bounds the file at one row per run and must keep
/// generation fencing intact.
#[test]
fn supervisor_lease_acquire_compacts_and_keeps_fencing() {
    let root = team_test_root("lease-compact");
    let store = HarnessStore::new(&root);
    seed_lease_run(&store, "run-a");
    store
        .acquire_test_supervisor_lease("run-a", "sup-1", 1, "a", 1_000, 10)
        .expect("acquire gen 1");
    for tick in 0..500u64 {
        store
            .renew_team_supervisor_lease("run-a", "sup-1", 1, 1_001 + tick, 10)
            .expect("renew");
    }
    let before = store
        .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
        .expect("read")
        .len();
    assert!(before > 500, "history should be long before compaction");

    // The lease has expired, so a different Supervisor takes generation 2.
    let gen2 = store
        .acquire_test_supervisor_lease("run-a", "sup-2", 3, "b", 900_000, 15_000)
        .expect("acquire gen 2");
    assert_eq!(gen2.generation, 2);

    // Compaction runs before the new row is appended, so one run yields the
    // collapsed prior row plus the freshly acquired lease. The invariant is
    // that the file is bounded by run count rather than by heartbeat count.
    let after = store
        .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
        .expect("read")
        .len();
    assert_eq!(
        after, 2,
        "compaction must bound the file at ~one row per run, got {after} (was {before})"
    );

    // The fenced-out generation must still be rejected after compaction.
    assert!(
        store
            .renew_team_supervisor_lease("run-a", "sup-1", 1, 900_001, 15_000)
            .is_err(),
        "stale generation must not renew"
    );
    let live = store
        .latest_lease_for_run_unlocked("run-a")
        .expect("tail")
        .expect("present");
    assert_eq!(live.supervisor_id, "sup-2");
    assert_eq!(live.generation, 2);
}
