use super::*;

#[test]
fn durable_rate_limit_rejects_new_work_but_preserves_exact_replay() {
    let root = TestRoot::new("rate-limit");
    let store = FabricStore::open_with_limits(
        root.path(),
        FabricStoreLimits {
            max_operations_per_minute_per_source_actor: 1,
            ..FabricStoreLimits::default()
        },
    )
    .expect("open bounded store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let source_hello = hello("node-a", "gateway-a", "cert-a", &fingerprint("node-a"));
    let source = connect_node(
        &control,
        lease.control_plane_generation,
        &source_hello,
        &signing_key("node-a"),
        30,
    )
    .expect("source connect");
    let target_hello = hello("node-b", "gateway-b", "cert-b", &fingerprint("node-b"));
    connect_node(
        &control,
        lease.control_plane_generation,
        &target_hello,
        &signing_key("node-b"),
        30,
    )
    .expect("target connect");
    let first = operation(source.gateway_generation, lease.control_plane_generation);
    accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        first.clone(),
        100,
    )
    .expect("first operation is within limit");
    assert!(
        accept_fabric_operation(
            &control,
            lease.control_plane_generation,
            source.gateway_generation,
            first.clone(),
            101,
        )
        .expect("exact replay bypasses new-work rate accounting")
        .3
    );
    let before = store.snapshot().expect("snapshot");
    let mut second = first;
    second.id = "operation-rate-limited".into();
    second.idempotency_key = "idempotency-rate-limited".into();
    let limited = accept_fabric_operation(
        &control,
        lease.control_plane_generation,
        source.gateway_generation,
        second,
        102,
    )
    .expect_err("new operation exceeds durable rate limit");
    assert_eq!(limited.code, FabricErrorCode::RateLimited);
    assert!(limited.retryable);
    assert_eq!(limited.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}
