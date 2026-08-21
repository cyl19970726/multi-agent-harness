use super::*;

#[test]
fn offline_queue_capacity_rejects_at_count_and_over_bytes_with_zero_delta() {
    fn bounded_control(label: &str, limits: FabricStoreLimits) -> (TestRoot, FabricStore) {
        let root = TestRoot::new(label);
        let store = FabricStore::open_with_limits(root.path(), limits).expect("bounded store");
        (root, store)
    }

    for (label, limits) in [
        (
            "queue-count-boundary",
            FabricStoreLimits {
                max_queued_operations_per_node: 1,
                ..FabricStoreLimits::default()
            },
        ),
        (
            "queue-byte-boundary",
            FabricStoreLimits {
                max_queued_bytes_per_node: 1,
                ..FabricStoreLimits::default()
            },
        ),
    ] {
        let (_root, store) = bounded_control(label, limits);
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
        if label == "queue-count-boundary" {
            accept_fabric_operation(
                &control,
                lease.control_plane_generation,
                source.gateway_generation,
                first.clone(),
                100,
            )
            .expect("the one allowed queue slot is accepted");
        }
        let before = store
            .snapshot()
            .expect("snapshot before capacity rejection");
        let mut rejected_operation = first;
        rejected_operation.id = format!("operation-{label}");
        rejected_operation.idempotency_key = format!("idempotency-{label}");
        let rejected = accept_fabric_operation(
            &control,
            lease.control_plane_generation,
            source.gateway_generation,
            rejected_operation,
            101,
        )
        .expect_err("capacity boundary rejects without accepting partial route state");
        assert_eq!(rejected.code, FabricErrorCode::QueueCapacity);
        assert_eq!(rejected.effect, EffectCertainty::None);
        assert_eq!(store.snapshot().expect("snapshot after rejection"), before);
    }
}
