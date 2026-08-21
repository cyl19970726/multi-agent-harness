use super::*;

    #[test]
    fn schema_to_json_schema_coerces_known_type_hints() {
        // Well-known type words become real JSON-Schema scalar types (issue #139
        // item 5) — no `description`, so the provider returns a real bool/int/
        // number, not the string "true"/"7". Descriptive hints stay `string`.
        let flat = serde_json::json!({
            "ok": "bool",
            "count": "int",
            "ratio": "number",
            "note": "a short reason",
        });
        let js = schema_to_json_schema(&flat);
        assert_eq!(js["properties"]["ok"]["type"], serde_json::json!("boolean"));
        assert!(js["properties"]["ok"].get("description").is_none());
        assert_eq!(
            js["properties"]["count"]["type"],
            serde_json::json!("integer")
        );
        assert_eq!(
            js["properties"]["ratio"]["type"],
            serde_json::json!("number")
        );
        // A non-type-word hint is still a string field with the hint as description.
        assert_eq!(
            js["properties"]["note"]["type"],
            serde_json::json!("string")
        );
        assert_eq!(
            js["properties"]["note"]["description"],
            serde_json::json!("a short reason")
        );
    }

