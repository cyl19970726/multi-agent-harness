use super::*;

#[test]
fn host_rest_enrollment_uses_csr_possession_and_one_atomic_consumption() {
    let root = std::env::temp_dir().join(format!(
        "agentfirm-fabric-http-enroll-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove prior test root");
    }
    std::fs::create_dir(&root).expect("create test root");
    let store = harness_fabric::FabricStore::open(&root).expect("FabricStore");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert("company-test", [4; 32]);
    let control = ControlPlane::new("company-test", "control-test", &store, &keys, [5; 32]);
    let now = now_unix_ms().expect("clock");
    let lease = control
        .acquire_lease("lease-test", 0, now)
        .expect("Control Plane lease");
    let ca = harness_fabric::pki::generate_ca("company-test").expect("Company CA");
    let actor = AuthenticatedActor {
        company_id: "company-test".into(),
        actor_id: "company-host:http".into(),
        actor_kind: harness_fabric::ActorKind::Human,
        role_bindings: std::collections::BTreeSet::from(["company_host".into()]),
        session_id: "host-test".into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
    };
    let created = route_host_http(
        "POST",
        "/v1/fabric/enrollments",
        "/v1/fabric/enrollments",
        &serde_json::json!({
            "enrollment_id":"enrollment-node-a",
            "requested_name":"Node A",
            "allowed_capabilities":["durable-routing","artifact-transfer"],
            "authorized_node_daemon_id":"node-daemon:node-a",
            "authorized_node_daemon_generation":1
        }),
        Some(&actor),
        &control,
        lease.control_plane_generation,
        &ca,
        now + 1,
        "host-token-00000000000000000000000000000000",
    )
    .expect("create enrollment");
    let raw_token = created["raw_token"].as_str().expect("one-time token");
    let csr = harness_fabric::pki::generate_node_csr("company-test", "node-a").expect("Node CSR");
    let schema_bundle_digest = remote_fabric_schema_bundle_digest();
    let enrolled = route_host_http(
        "POST",
        "/v1/fabric/nodes/enroll",
        "/v1/fabric/nodes/enroll",
        &serde_json::json!({
            "raw_token":raw_token,
            "node_id":"node-a",
            "display_name":"Node A",
            "csr_pem":csr.csr_pem,
            "schema_bundle_digest":schema_bundle_digest
        }),
        None,
        &control,
        lease.control_plane_generation,
        &ca,
        now + 2,
        "host-token-00000000000000000000000000000000",
    )
    .expect("consume CSR enrollment");
    assert_eq!(enrolled["node"]["id"], "node-a");
    assert_eq!(
        enrolled["node"]["public_key_fingerprint"],
        csr.public_key_fingerprint
    );
    let listed = route_host_http(
        "GET",
        "/v1/fabric/nodes",
        "/v1/fabric/nodes?limit=1",
        &serde_json::Value::Null,
        Some(&actor),
        &control,
        lease.control_plane_generation,
        &ca,
        now + 2,
        "host-token-00000000000000000000000000000000",
    )
    .expect("list Nodes with opaque snapshot cursor");
    let cursor = listed["next_cursor"].as_str().expect("next cursor");
    assert_ne!(cursor, "node-a");
    assert_eq!(cursor.len(), 64);
    assert_eq!(
        route_host_http(
            "GET",
            "/v1/fabric/nodes",
            "/v1/fabric/nodes?cursor=browser-selected-node-a",
            &serde_json::Value::Null,
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 2,
            "host-token-00000000000000000000000000000000",
        )
        .expect_err("raw or forged Node cursor fails closed")
        .code,
        FabricErrorCode::ExpectedRevisionConflict
    );
    let before = store.snapshot().expect("before replay");
    let replay = route_host_http(
        "POST",
        "/v1/fabric/nodes/enroll",
        "/v1/fabric/nodes/enroll",
        &serde_json::json!({
            "raw_token":raw_token,
            "node_id":"node-a",
            "display_name":"Node A",
            "csr_pem":csr.csr_pem,
            "schema_bundle_digest":schema_bundle_digest
        }),
        None,
        &control,
        lease.control_plane_generation,
        &ca,
        now + 3,
        "host-token-00000000000000000000000000000000",
    )
    .expect_err("one-use token cannot replay");
    assert_eq!(replay.code, FabricErrorCode::EnrollmentConsumed);
    assert_eq!(store.snapshot().expect("after replay"), before);

    let recovery = route_host_http(
        "POST",
        "/v1/fabric/enrollments",
        "/v1/fabric/enrollments",
        &serde_json::json!({
            "enrollment_id":"enrollment-node-a-recovery",
            "requested_name":"Node A recovered",
            "allowed_capabilities":["durable-routing","artifact-transfer"],
            "authorized_node_daemon_id":"node-daemon:node-a",
            "authorized_node_daemon_generation":2
        }),
        Some(&actor),
        &control,
        lease.control_plane_generation,
        &ca,
        now + 4,
        "host-token-00000000000000000000000000000000",
    )
    .expect("Host grants exact successor recovery");
    let recovery_csr =
        harness_fabric::pki::generate_node_csr("company-test", "node-a").expect("successor CSR");
    let recovered = route_host_http(
        "POST",
        "/v1/fabric/nodes/enroll",
        "/v1/fabric/nodes/enroll",
        &serde_json::json!({
            "raw_token":recovery["raw_token"],
            "node_id":"node-a",
            "display_name":"Node A recovered",
            "csr_pem":recovery_csr.csr_pem,
            "schema_bundle_digest":schema_bundle_digest
        }),
        None,
        &control,
        lease.control_plane_generation,
        &ca,
        now + 5,
        "host-token-00000000000000000000000000000000",
    )
    .expect("existing Node recovers under Host-frozen successor daemon generation");
    assert_eq!(recovered["node"]["node_revision"], 2);
    assert_eq!(recovered["certificate"]["node_daemon_generation"], 2);
    let recovery_state = store.snapshot().expect("recovery state");
    assert!(recovery_state.revoked_certificate_serials.contains(
        enrolled["certificate"]["serial"]
            .as_str()
            .expect("old serial")
    ));
    std::fs::remove_dir_all(root).expect("remove test root");
}
