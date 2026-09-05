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

    // Retention rule: per (node, daemon, generation) keep the group's first
    // row (the acquire) and the last row of each status, with compaction
    // running before the append — so one renewed generation holds the
    // acquire row, the previous latest renewal, and the in-flight renewal:
    // bounded by generation count, never by heartbeat count.
    let rows = store
        .read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")
        .expect("read")
        .len();
    assert!(
        rows <= 3,
        "5,000 renewals must keep the ledger bounded at acquire + last renewal + in-flight renewal, got {rows}"
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

    // The reviewer scenario (fabric_identity_sessions.rs
    // `predecessor_was_released`): generation 1 drains and releases,
    // generation 2 acquires and renews — the renewal-time compaction must NOT
    // drop generation 1's Released row, or session adoption on a daemon
    // restart breaks.
    store
        .drain_node_daemon_lease(node_id, "daemon-1", 1, "instance-1", 6_001, 10)
        .expect("drain generation 1");
    store
        .release_node_daemon_lease(node_id, "daemon-1", 1, "instance-1", 6_100)
        .expect("release generation 1");
    let gen2 = store
        .acquire_node_daemon_lease(node_id, "daemon-1", "instance-2", 6_200, 1_000_000)
        .expect("acquire generation 2");
    assert_eq!(gen2.generation, 2);
    for tick in 0..8u64 {
        store
            .renew_node_daemon_lease(
                node_id,
                "daemon-1",
                2,
                "instance-2",
                6_201 + tick,
                1_000_000,
            )
            .expect("renew generation 2");
    }

    let rows = store
        .read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")
        .expect("read final ledger");
    // Bound: per (node, daemon, generation) the acquire row plus the last
    // row of each status plus the in-flight renewal — generation 1 keeps
    // acquire/last-renewal/draining/released (4), generation 2 keeps
    // acquire/last-renewal/in-flight (3) — never the 5,010 appended rows.
    assert!(
        rows.len() <= 7,
        "renewal compaction must bound the ledger by generation count, got {}",
        rows.len()
    );
    let predecessor = rows
        .iter()
        .rfind(|lease| {
            lease.node_id == node_id && lease.daemon_id == "daemon-1" && lease.generation == 1
        })
        .expect("the released predecessor row survives successor renewals");
    assert_eq!(
        predecessor.status,
        NodeDaemonLeaseStatus::Released,
        "the predecessor's last row must stay the explicit Released row"
    );
    // The acquire row of each generation is the first row of its group.
    assert_eq!(
        rows.iter()
            .find(|lease| lease.generation == 1)
            .expect("generation 1 acquire row")
            .instance_id,
        "instance-1"
    );
    let latest = store
        .latest_node_daemon_lease(node_id)
        .expect("latest after successor renewals")
        .expect("present");
    assert_eq!(latest.generation, 2);
    assert_eq!(latest.instance_id, "instance-2");
    assert_eq!(latest.status, NodeDaemonLeaseStatus::Active);
}
