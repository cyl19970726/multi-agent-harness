#[cfg(test)]
mod claude_stream_tests {
    use std::fs;

    // Mock ClaudeStreamEvent for testing
    #[derive(Debug, Clone)]
    struct ClaudeStreamEvent {
        event_type: String,
        payload: serde_json::Value,
    }

    impl ClaudeStreamEvent {
        fn parse_line(line: &str) -> Option<Self> {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(payload) => {
                    let event_type = payload
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Some(ClaudeStreamEvent {
                        event_type,
                        payload,
                    })
                }
                Err(_) => None,
            }
        }

        fn session_id(&self) -> Option<String> {
            if self.event_type == "system" {
                self.payload
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
    }

    #[test]
    fn test_parse_claude_stream_json_sample() {
        // Load the sample NDJSON file without requiring a live claude binary
        let sample_path = "tests/data/claude_stream_sample.ndjson";
        let content = fs::read_to_string(sample_path).expect("Failed to load sample NDJSON");

        // Parse events
        let mut events = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Some(event) = ClaudeStreamEvent::parse_line(trimmed) {
                    events.push(event);
                }
            }
        }

        // Verify events were parsed
        assert!(
            !events.is_empty(),
            "Events should be parsed from sample NDJSON"
        );
        assert_eq!(events.len(), 7, "Sample NDJSON has 7 events");

        // Verify event types
        assert_eq!(events[0].event_type, "system");
        assert_eq!(events[1].event_type, "stream_event");
        assert_eq!(events[6].event_type, "result");

        // Verify session_id extraction
        let session_id = events.iter().find_map(|e| e.session_id());
        assert_eq!(session_id, Some("sess_test_12345".to_string()));
    }

    #[test]
    fn test_claude_stream_resilience() {
        // Test resilience: skip invalid lines, handle empty lines
        let input = r#"
{"type": "system", "session_id": "sess_123"}
invalid json line
{"type": "stream_event", "event": "text_delta"}

{"type": "result"}
"#;

        let mut events = Vec::new();
        for line in input.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Some(event) = ClaudeStreamEvent::parse_line(trimmed) {
                    events.push(event);
                }
            }
        }

        // Should have parsed 3 valid events, skipping invalid JSON
        assert_eq!(events.len(), 3, "Should skip invalid JSON lines gracefully");
        assert_eq!(events[0].event_type, "system");
        assert_eq!(events[1].event_type, "stream_event");
        assert_eq!(events[2].event_type, "result");
    }
}
