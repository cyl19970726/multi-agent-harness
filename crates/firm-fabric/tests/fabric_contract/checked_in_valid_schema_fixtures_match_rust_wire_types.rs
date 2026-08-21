use super::*;

#[test]
fn checked_in_valid_schema_fixtures_match_rust_wire_types() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/remote-fabric/fixtures/valid");
    let node: CompanyNode = serde_json::from_slice(
        &std::fs::read(root.join("company-node.json")).expect("read CompanyNode fixture"),
    )
    .expect("CompanyNode fixture matches Rust");
    assert_eq!(node.schema_version, FABRIC_SCHEMA_VERSION);
    let enrollment: NodeEnrollment = serde_json::from_slice(
        &std::fs::read(root.join("node-enrollment.json")).expect("read enrollment fixture"),
    )
    .expect("NodeEnrollment fixture matches Rust");
    assert_eq!(enrollment.status, EnrollmentStatus::Pending);
    let gateway: NodeGatewayLease = serde_json::from_slice(
        &std::fs::read(root.join("node-gateway-lease.json")).expect("read gateway fixture"),
    )
    .expect("NodeGatewayLease fixture matches Rust");
    assert_eq!(gateway.gateway_generation, 1);
    let hello: NodeHello = serde_json::from_slice(
        &std::fs::read(root.join("node-hello.json")).expect("read NodeHello fixture"),
    )
    .expect("NodeHello fixture matches Rust");
    assert_eq!(hello.protocol_min, FABRIC_PROTOCOL_VERSION);
    let welcome: NodeWelcome = serde_json::from_slice(
        &std::fs::read(root.join("node-welcome.json")).expect("read NodeWelcome fixture"),
    )
    .expect("NodeWelcome fixture matches Rust");
    assert_eq!(welcome.gateway_generation, 1);
    let frame: FabricFrame = serde_json::from_slice(
        &std::fs::read(root.join("fabric-frame.json")).expect("read FabricFrame fixture"),
    )
    .expect("FabricFrame fixture matches Rust");
    frame
        .validate()
        .expect("FabricFrame fixture digest matches");
    let operation: RoutedOperation = serde_json::from_slice(
        &std::fs::read(root.join("routed-operation.json")).expect("read operation fixture"),
    )
    .expect("RoutedOperation fixture matches Rust");
    assert_eq!(operation.protocol_version, FABRIC_PROTOCOL_VERSION);
    operation
        .validate_digest()
        .expect("fixture body digest matches");
    let receipt: RouteReceipt = serde_json::from_slice(
        &std::fs::read(root.join("route-receipt.json")).expect("read receipt fixture"),
    )
    .expect("RouteReceipt fixture matches Rust");
    assert_eq!(receipt.control_plane_generation, 2);
    let artifact: RemoteArtifactManifest = serde_json::from_slice(
        &std::fs::read(root.join("artifact-manifest.json")).expect("read artifact fixture"),
    )
    .expect("artifact fixture matches Rust");
    assert_eq!(artifact.size_bytes, 128);
}
