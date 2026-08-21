use super::*;

    #[test]
    fn schema_required_keys_reads_top_level_object_keys() {
        let schema = serde_json::json!({ "ok": "", "summary": "", "score": 0 });
        let mut keys = schema_required_keys(&schema);
        keys.sort();
        assert_eq!(keys, vec!["ok", "score", "summary"]);
        // A non-object schema declares no required keys.
        assert!(schema_required_keys(&serde_json::json!("nope")).is_empty());
    }

