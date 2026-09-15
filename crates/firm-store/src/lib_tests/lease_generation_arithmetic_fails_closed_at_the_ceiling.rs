use super::*;

/// A lease generation is a fence, so the one thing it must never do is repeat.
/// `saturating_add(1)` at `u64::MAX` mints a successor whose generation equals
/// the predecessor's, which every downstream generation check would then accept
/// as current. Both lease acquirers therefore refuse instead of saturating.
/// The ceiling row is seeded directly because no writer can reach it in a test.
#[test]
fn lease_generation_arithmetic_fails_closed_at_the_ceiling() {
    let root = team_test_root("lease-generation-ceiling");
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

    let first = store
        .acquire_node_daemon_lease(node_id, "daemon-1", "instance-1", 1_000, 5_000)
        .expect("acquire generation 1");
    assert_eq!(first.generation, 1);
    store
        .release_node_daemon_lease(node_id, "daemon-1", first.generation, "instance-1", 2_000)
        .expect("release generation 1");

    append_sparse_row(
        &root,
        "node_daemon_leases.jsonl",
        &serde_json::to_string(&NodeDaemonLease {
            node_id: node_id.into(),
            daemon_id: "daemon-1".into(),
            generation: u64::MAX,
            instance_id: "instance-1".into(),
            status: NodeDaemonLeaseStatus::Released,
            acquired_unix_ms: 2_000,
            renewed_unix_ms: 2_000,
            expires_unix_ms: 7_000,
            released_unix_ms: Some(2_500),
        })
        .expect("encode ceiling lease"),
    );

    let refusal = store
        .acquire_node_daemon_lease(node_id, "daemon-2", "instance-2", 3_000, 5_000)
        .expect_err("a successor generation must not repeat the predecessor's");
    assert!(
        refusal
            .to_string()
            .contains("NODE_DAEMON_LEASE_GENERATION_EXHAUSTED"),
        "unexpected refusal: {refusal}"
    );
    let latest = store
        .latest_node_daemon_lease(node_id)
        .expect("read latest")
        .expect("ceiling row is still the latest");
    assert_eq!(
        latest.generation,
        u64::MAX,
        "the refusal must not have written a successor row"
    );

    let supervisor_root = team_test_root("supervisor-generation-ceiling");
    let supervisor_store = HarnessStore::new(&supervisor_root);
    seed_lease_run(&supervisor_store, "run-ceiling");
    let supervisor = supervisor_store
        .acquire_test_supervisor_lease("run-ceiling", "sup-1", 1, "a", 1_000, 15_000)
        .expect("acquire supervisor generation 1");
    assert_eq!(supervisor.generation, 1);

    append_sparse_row(
        &supervisor_root,
        "team_supervisor_leases.jsonl",
        &serde_json::to_string(&TeamSupervisorLease {
            generation: u64::MAX,
            expires_unix_ms: 16_000,
            ..supervisor
        })
        .expect("encode ceiling supervisor lease"),
    );

    let refusal = supervisor_store
        .acquire_test_supervisor_lease("run-ceiling", "sup-2", 2, "b", 900_000, 15_000)
        .expect_err("an expired ceiling lease must not mint a repeated generation");
    assert!(
        refusal
            .to_string()
            .contains("TEAM_SUPERVISOR_LEASE_GENERATION_EXHAUSTED"),
        "unexpected refusal: {refusal}"
    );
}
