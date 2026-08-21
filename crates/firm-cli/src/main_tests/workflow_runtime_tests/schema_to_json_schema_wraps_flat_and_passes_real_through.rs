use super::*;

#[test]
fn schema_to_json_schema_wraps_flat_and_passes_real_through() {
    // Flat { key: hint } -> a string-property object schema with required keys.
    let flat = serde_json::json!({ "verdict": "the call", "score": "0-100" });
    let js = schema_to_json_schema(&flat);
    assert_eq!(js["type"], serde_json::json!("object"));
    assert_eq!(
        js["properties"]["verdict"]["type"],
        serde_json::json!("string")
    );
    assert_eq!(
        js["properties"]["verdict"]["description"],
        serde_json::json!("the call")
    );
    assert_eq!(js["additionalProperties"], serde_json::json!(false));
    let req = js["required"].as_array().expect("required array");
    assert!(req.contains(&serde_json::json!("verdict")));
    assert!(req.contains(&serde_json::json!("score")));

    // An already-valid JSON Schema (has `type`/`properties`) is unchanged.
    let real = serde_json::json!({
        "type": "object",
        "properties": { "score": { "type": "integer" } },
        "required": ["score"],
    });
    assert_eq!(schema_to_json_schema(&real), real);
}
