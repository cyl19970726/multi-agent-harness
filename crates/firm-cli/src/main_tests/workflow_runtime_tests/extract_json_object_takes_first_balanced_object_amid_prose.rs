use super::*;

    #[test]
    fn extract_json_object_takes_first_balanced_object_amid_prose() {
        // Prose around the object, plus braces inside a string literal.
        let reply = "Here is the result:\n{\"msg\": \"a } b\", \"ok\": true}\nThanks!";
        let value = extract_json_object(reply).expect("parsed");
        assert_eq!(value["msg"], serde_json::json!("a } b"));
        assert_eq!(value["ok"], serde_json::json!(true));
    }

