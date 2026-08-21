use super::*;

    #[test]
    fn schema_instruction_lists_keys_and_inlines_the_shape() {
        let schema = serde_json::json!({ "ok": "" });
        let instruction = schema_instruction(&schema);
        assert!(instruction.contains("ONLY a single JSON object"));
        assert!(instruction.contains("ok"));
        // The compact schema is inlined as a shape hint.
        assert!(instruction.contains("{\"ok\":\"\"}"));
    }

