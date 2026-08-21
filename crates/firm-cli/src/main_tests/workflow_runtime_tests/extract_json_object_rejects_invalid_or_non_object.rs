use super::*;

    #[test]
    fn extract_json_object_rejects_invalid_or_non_object() {
        assert!(extract_json_object("not json at all").is_none());
        // A JSON array is not an object.
        assert!(extract_json_object("[1, 2, 3]").is_none());
        // An unbalanced object does not parse.
        assert!(extract_json_object("{\"ok\": true").is_none());
    }

