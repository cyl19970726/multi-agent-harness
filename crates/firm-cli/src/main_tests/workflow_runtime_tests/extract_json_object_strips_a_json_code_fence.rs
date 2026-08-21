use super::*;

    #[test]
    fn extract_json_object_strips_a_json_code_fence() {
        let reply = "```json\n{\"ok\": true, \"summary\": \"done\"}\n```";
        let value = extract_json_object(reply).expect("parsed");
        assert_eq!(value["summary"], serde_json::json!("done"));
        // A bare (langless) fence works too.
        let reply2 = "```\n{\"ok\": false}\n```";
        let value2 = extract_json_object(reply2).expect("parsed");
        assert_eq!(value2["ok"], serde_json::json!(false));
    }

