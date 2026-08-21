use super::*;

/// The tail-window fast path must not change which lease a reader sees,
/// even when the target row sits far in front of the window.
#[test]
fn supervisor_lease_tail_read_agrees_with_full_scan() {
    let root = team_test_root("lease-tail");
    let store = HarnessStore::new(&root);
    seed_lease_run(&store, "run-a");
    seed_lease_run(&store, "run-b");
    store
        .acquire_test_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
        .expect("acquire a");
    store
        .acquire_test_supervisor_lease("run-b", "sup-b", 2, "b", 1_000, 15_000)
        .expect("acquire b");
    // Push run-a's latest row well outside the 256 KiB tail window.
    for tick in 0..4_000u64 {
        store
            .renew_team_supervisor_lease("run-b", "sup-b", 1, 2_000 + tick, 15_000)
            .expect("renew b");
    }

    let tail = store
        .latest_lease_for_run_unlocked("run-a")
        .expect("tail read")
        .expect("run-a lease present");
    let full = latest_by_id(
        store
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
            .expect("full scan"),
        |lease| lease.team_run_id.clone(),
    )
    .remove("run-a")
    .expect("run-a in full scan");
    assert_eq!(tail.supervisor_id, full.supervisor_id);
    assert_eq!(tail.generation, full.generation);
    assert_eq!(tail.expires_unix_ms, full.expires_unix_ms);
}
