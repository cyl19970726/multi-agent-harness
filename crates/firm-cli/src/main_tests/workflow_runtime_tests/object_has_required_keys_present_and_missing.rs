use super::*;

    #[test]
    fn object_has_required_keys_present_and_missing() {
        let obj = serde_json::json!({ "ok": true, "summary": "x" });
        let required: Vec<String> = vec!["ok".into(), "summary".into()];
        assert!(object_has_required_keys(&obj, &required));
        // A missing key fails validation.
        let missing: Vec<String> = vec!["ok".into(), "score".into()];
        assert!(!object_has_required_keys(&obj, &missing));
        // An empty required set is vacuously satisfied.
        assert!(object_has_required_keys(&obj, &[]));
    }

