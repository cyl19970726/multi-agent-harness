use super::*;

#[test]
fn extract_json_object_handles_bare_object() {
    let value = extract_json_object(r#"{"ok": true, "n": 3}"#).expect("parsed");
    assert_eq!(value["ok"], serde_json::json!(true));
    assert_eq!(value["n"], serde_json::json!(3));
}
