use super::*;

#[test]
fn route_queue_replay_uses_semantic_intent_not_regenerated_timestamps() {
    let mut operation: harness_fabric::RoutedOperation = serde_json::from_str(include_str!(
        "../../../../schemas/remote-fabric/fixtures/valid/routed-operation.json"
    ))
    .expect("valid routed operation fixture");
    operation
        .authorization_context
        .insert("capability".into(), "remote-probe".into());
    operation.created_at_unix_ms = 11;
    operation.expires_at_unix_ms = 2_000;
    operation.actor.issued_at_unix_ms = 11;
    operation.actor.expires_at_unix_ms = 2_000;
    let body = serde_json::json!({"probe":"reachable"});

    assert!(route_queue_intent_matches(
        &operation,
        "company-a",
        "operation-1",
        harness_fabric::PROBE_OPERATION_KIND,
        "node-a",
        Some("space-a"),
        "node-b",
        "space-b",
        "idem-1",
        "probe:a:b",
        None,
        harness_fabric::PROBE_BODY_SCHEMA,
        &body,
        "remote-probe",
    )
    .expect("semantic replay comparison"));

    let changed = serde_json::json!({"probe":"changed"});
    assert!(!route_queue_intent_matches(
        &operation,
        "company-a",
        "operation-1",
        harness_fabric::PROBE_OPERATION_KIND,
        "node-a",
        Some("space-a"),
        "node-b",
        "space-b",
        "idem-1",
        "probe:a:b",
        None,
        harness_fabric::PROBE_BODY_SCHEMA,
        &changed,
        "remote-probe",
    )
    .expect("changed replay comparison"));
}
