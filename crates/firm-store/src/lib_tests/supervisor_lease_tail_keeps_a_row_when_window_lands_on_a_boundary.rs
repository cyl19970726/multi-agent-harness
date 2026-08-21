use super::*;

/// The tail window may land exactly on a row boundary. Discarding the first
/// line unconditionally would drop a COMPLETE row; reviewer-reported.
#[test]
fn supervisor_lease_tail_keeps_a_row_when_window_lands_on_a_boundary() {
    let root = team_test_root("lease-boundary");
    let store = HarnessStore::new(&root);
    seed_lease_run(&store, "run-a");
    store
        .acquire_test_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
        .expect("acquire");
    for tick in 0..20u64 {
        store
            .renew_team_supervisor_lease("run-a", "sup-a", 1, 1_001 + tick, 15_000)
            .expect("renew");
    }
    let path = root.join("team_supervisor_leases.jsonl");
    let bytes = std::fs::read(&path).expect("read lease file");
    let total = bytes.len() as u64;
    // Start the window exactly at the first byte of the LAST row, i.e. one
    // past the second-to-last newline. The file ends with a newline, so the
    // last newline is the row terminator, not the row start.
    let last_terminator = bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .expect("trailing newline");
    let row_start = bytes[..last_terminator]
        .iter()
        .rposition(|&b| b == b'\n')
        .expect("a previous row") as u64
        + 1;
    let window = total - row_start;
    let rows = store
        .read_jsonl_tail::<TeamSupervisorLease>("team_supervisor_leases.jsonl", window)
        .expect("tail read");
    assert_eq!(
        rows.len(),
        1,
        "a window landing on a row boundary must keep that row, got {}",
        rows.len()
    );
}
