use super::*;

/// Compaction on renewal bounds node_daemon_leases.jsonl at ~one row per node
/// while latest_node_daemon_lease keeps returning the newest
/// generation/instance — and generation fencing stays intact.
#[test]
fn node_daemon_lease_renewal_compacts_and_keeps_latest() {
    let root = team_test_root("node-lease-renewal-compact");
    let store = HarnessStore::new(&root);
    let node_id = "00000000-0000-4000-8000-000000000001";
    store
        .insert_execution_node(&ExecutionNode {
            id: node_id.into(),
            display_name: "test-node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("seed Node");
    let lease = store
        .acquire_node_daemon_lease(node_id, "daemon-1", "instance-1", 1_000, 1_000_000)
        .expect("acquire generation 1");
    for tick in 0..5_000u64 {
        store
            .renew_node_daemon_lease(
                node_id,
                "daemon-1",
                lease.generation,
                "instance-1",
                1_001 + tick,
                1_000_000,
            )
            .expect("renew");
    }

    // Retention rule: one row per node (latest wins), compaction before the
    // append, so one node yields the collapsed prior row plus the in-flight
    // renewal — bounded by node count, never by heartbeat count.
    let rows = store
        .read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")
        .expect("read")
        .len();
    assert!(
        rows <= 2,
        "5,000 renewals must keep the ledger bounded at ~one row per node plus the in-flight renewal, got {rows}"
    );

    let latest = store
        .latest_node_daemon_lease(node_id)
        .expect("latest")
        .expect("present");
    assert_eq!(latest.daemon_id, "daemon-1");
    assert_eq!(latest.instance_id, "instance-1");
    assert_eq!(latest.generation, lease.generation);
    assert_eq!(latest.expires_unix_ms, 1_001 + 4_999 + 1_000_000);

    // The fenced-out generation must still be rejected after compaction.
    assert!(
        store
            .renew_node_daemon_lease(node_id, "daemon-1", 99, "instance-1", 9_000_000, 1_000_000)
            .is_err(),
        "a foreign generation must not renew after compaction"
    );
}
