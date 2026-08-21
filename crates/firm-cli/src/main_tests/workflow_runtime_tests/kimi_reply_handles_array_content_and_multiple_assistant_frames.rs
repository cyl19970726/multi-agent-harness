use super::*;

    #[test]
    fn kimi_reply_handles_array_content_and_multiple_assistant_frames() {
        let frames: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "alpha"}]}),
            serde_json::json!({"role": "user", "content": "ignored"}),
            serde_json::json!({"role": "assistant", "content": "beta"}),
        ];
        assert_eq!(
            extract_kimi_reply_text(&frames).as_deref(),
            Some("alpha\nbeta")
        );
    }

