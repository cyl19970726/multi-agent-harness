use super::*;

    #[test]
    fn parse_claude_result_extras_reads_structured_and_cost() {
        let events = vec![
            serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-4-8"}),
            serde_json::json!({
                "type": "result",
                "structured_output": { "verdict": "pass", "score": 100 },
                "total_cost_usd": 0.1866,
                "usage": { "input_tokens": 5, "output_tokens": 2 }
            }),
        ];
        let (structured, cost) = parse_claude_result_extras(&events);
        assert_eq!(
            structured,
            Some(serde_json::json!({ "verdict": "pass", "score": 100 }))
        );
        assert_eq!(cost, Some(0.1866));

        // No `result` frame -> both None.
        let (s2, c2) = parse_claude_result_extras(&[serde_json::json!({"type": "system"})]);
        assert!(s2.is_none() && c2.is_none());
    }

