use super::*;

#[test]
fn message_route_requires_verified_immutable_payload_not_identity_only() {
    let message_body_digest = format!("sha256:{}", sha256_hex("hello"));
    let mut request = operation(1, 1);
    request.kind = MESSAGE_REFERENCE_KIND.into();
    request.body_schema = MESSAGE_REFERENCE_SCHEMA.into();
    request.body = json!({
        "message_id": "message:remote-1",
        "body_digest": message_body_digest,
        "canonical_message_envelope": {
            "id": "message:remote-1",
            "body": "hello",
            "body_digest": message_body_digest,
        },
        "message_object_ref": null,
    });
    request.body_digest = json_digest(&request.body).expect("route body digest");
    assert!(matches!(
        request.closed_body().expect("immutable embedded message"),
        ClosedOperationBody::Message(_)
    ));

    let mut identity_only = request.clone();
    identity_only.body["canonical_message_envelope"] = serde_json::Value::Null;
    identity_only.body_digest = json_digest(&identity_only.body).unwrap();
    assert_eq!(
        identity_only
            .closed_body()
            .expect_err("message identity and digest alone are not a payload")
            .code,
        FabricErrorCode::InvalidPayload
    );

    let mut tampered = request;
    tampered.body["canonical_message_envelope"]["body"] = json!("tampered");
    tampered.body_digest = json_digest(&tampered.body).unwrap();
    assert_eq!(
        tampered
            .closed_body()
            .expect_err("target must verify the immutable message body digest")
            .code,
        FabricErrorCode::InvalidPayload
    );
}
