use super::*;

#[test]
fn operation_registry_requires_closed_kind_schema_and_body_scope() {
    let mut request = operation(3, 2);
    assert!(matches!(
        request.closed_body().expect("closed probe"),
        ClosedOperationBody::Probe(_)
    ));

    request.kind = RUNTIME_COMMAND_REFERENCE_KIND.into();
    request.body_schema = RUNTIME_COMMAND_REFERENCE_SCHEMA.into();
    request.body = json!({
        "runtime_command_id": "runtime-command:remote-1",
        "intent_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "target_execution_space_id": "space-b",
        "canonical_command_intent": {
            "id": "runtime-command:remote-1",
            "target_execution_space_id": "space-b",
            "command": "resume_session",
            "idempotency_key": "remote-runtime-1",
            "expected_version": 4,
            "expires_unix_ms": 90000,
            "payload": {"session_id": "session-b", "session_generation": 3},
            "payload_fingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "issued_at": "unix-ms:100"
        }
    });
    request.body_digest = json_digest(&request.body).expect("body digest");
    assert!(matches!(
        request.closed_body().expect("closed runtime reference"),
        ClosedOperationBody::RuntimeCommand(_)
    ));

    let mut injected_actor_field = request.clone();
    injected_actor_field.body["actor_id"] = json!("client-selected-host");
    injected_actor_field.body_digest =
        json_digest(&injected_actor_field.body).expect("hostile body digest");
    assert_eq!(
        injected_actor_field
            .closed_body()
            .expect_err("unknown authority field must fail closed")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut wrong_scope = request.clone();
    wrong_scope.body["target_execution_space_id"] = json!("space-c");
    wrong_scope.body_digest = json_digest(&wrong_scope.body).expect("wrong-scope digest");
    assert_eq!(
        wrong_scope
            .closed_body()
            .expect_err("body scope cannot override route scope")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut collaboration = operation(3, 2);
    let collaboration_payload = json!({"delegation_id": "delegation-1"});
    let collaboration_payload_digest = format!(
        "sha256:{}",
        json_digest(&collaboration_payload).expect("collaboration payload digest")
    );
    collaboration.kind = COLLABORATION_BUSINESS_OPERATION_KIND.into();
    collaboration.body_schema = COLLABORATION_BUSINESS_OPERATION_SCHEMA.into();
    collaboration.expected_target_revision = Some(7);
    collaboration.authorization_context = BTreeMap::from([
        ("target_team_id".into(), "team-b".into()),
        ("target_team_revision".into(), "9".into()),
        ("placement_generation".into(), "1".into()),
        (
            "required_capability".into(),
            "collaboration.delegation_decide".into(),
        ),
        ("business_actor_kind".into(), "agent_member".into()),
        ("business_actor_id".into(), "host-b".into()),
    ]);
    collaboration.body = json!({
        "business_kind": "delegation_decide",
        "required_capability": "collaboration.delegation_decide",
        "business_actor_kind": "agent_member",
        "business_actor_id": "host-b",
        "target_team_id": "team-b",
        "target_team_revision": 9,
        "placement_generation": 1,
        "expected_revision": 7,
        "payload_digest": collaboration_payload_digest,
        "payload": collaboration_payload,
    });
    collaboration.body_digest = json_digest(&collaboration.body).expect("body digest");
    assert!(matches!(
        collaboration
            .closed_body()
            .expect("closed collaboration operation"),
        ClosedOperationBody::CollaborationBusiness(_)
    ));
    let mut widened = collaboration;
    widened.body["required_capability"] = json!("collaboration.artifact_grant");
    widened.body_digest = json_digest(&widened.body).expect("widened body digest");
    assert_eq!(
        widened
            .closed_body()
            .expect_err("business kind cannot widen its capability")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut mismatched_schema = request;
    mismatched_schema.body_schema = MESSAGE_REFERENCE_SCHEMA.into();
    assert_eq!(
        mismatched_schema
            .closed_body()
            .expect_err("kind and schema are one frozen pair")
            .code,
        FabricErrorCode::SchemaIncompatible
    );
}
