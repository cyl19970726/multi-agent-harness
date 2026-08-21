use super::*;

#[test]
fn value_to_json_round_trips_with_json_to_value() {
    // A nested JSON value -> Starlark value -> JSON value must be identical.
    let original = serde_json::json!({
        "ok": true,
        "count": 3,
        "name": "audit",
        "tags": ["a", "b"],
        "nested": { "k": 1, "flag": false },
        "missing": serde_json::Value::Null,
    });
    Module::with_temp_heap(|module| {
        let value = json_to_value(module.heap(), &original);
        let back = value_to_json(value);
        assert_eq!(back, original);
        Ok::<(), starlark::Error>(())
    })
    .expect("round trip");
}
