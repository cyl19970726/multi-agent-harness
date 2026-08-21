use super::*;

    #[test]
    fn step_result_json_merges_details_without_overriding_base() {
        // The base keys (provider/ok/...) always win; details adds new keys.
        let result = workflow::StepResult {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            isolation: None,
            ok: true,
            output_summary: "summary".into(),
            step_id: None,
            started_at: None,
            details: Some(serde_json::json!({
                "model": "gpt-5-codex",
                "duration_ms": 99,
                // A colliding key must NOT override the base value.
                "ok": false,
            })),
            structured: None,
            ordinal: None,
        };
        let json = workflow::step_result_json(&result);
        assert_eq!(json["provider"], serde_json::json!("codex"));
        assert_eq!(json["ok"], serde_json::json!(true)); // base wins
        assert_eq!(json["model"], serde_json::json!("gpt-5-codex"));
        assert_eq!(json["duration_ms"], serde_json::json!(99));
    }

