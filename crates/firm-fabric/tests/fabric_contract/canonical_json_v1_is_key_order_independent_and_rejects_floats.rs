use super::*;

#[test]
fn canonical_json_v1_is_key_order_independent_and_rejects_floats() {
    let left = json!({"z": [3, {"b": 2, "a": 1}], "a": "value"});
    let right = json!({"a": "value", "z": [3, {"a": 1, "b": 2}]});
    assert_eq!(
        canonical_json_bytes(&left).expect("canonical left"),
        br#"{"a":"value","z":[3,{"a":1,"b":2}]}"#
    );
    assert_eq!(json_digest(&left).unwrap(), json_digest(&right).unwrap());
    assert_eq!(
        json_digest(&json!({"unsafe": 1.5}))
            .expect_err("wire canonicalization must reject floats")
            .code,
        FabricErrorCode::InvalidPayload
    );
}
