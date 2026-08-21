use super::*;

#[test]
fn kimi_parsers_match_the_real_v018_stream_shape() {
    // Verified LIVE against `kimi -p --output-format stream-json` (v0.18): a flat
    // assistant frame + a session.resume_hint meta frame — NOT claude-shaped.
    let frames: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "assistant", "content": "pong"}),
        serde_json::json!({
            "role": "meta",
            "type": "session.resume_hint",
            "session_id": "session_abc-123",
            "command": "kimi -r session_abc-123",
            "content": "To resume this session: kimi -r session_abc-123"
        }),
    ];
    assert_eq!(extract_kimi_reply_text(&frames).as_deref(), Some("pong"));
    assert_eq!(
        extract_kimi_session_id(&frames).as_deref(),
        Some("session_abc-123")
    );
    assert_eq!(
        infer_kimi_status(&frames, true),
        ProviderExecutionStatus::Succeeded
    );
    // REGRESSION GUARD: the claude reply extractor must FAIL on this shape —
    // proving why the kimi-native parser is required (no `type:"result"` /
    // `message.content[]`). The pre-fix adapter reused this and lost the reply.
    let claude_events: Vec<ClaudeStreamEvent> = frames
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .filter_map(|l| ClaudeStreamEvent::parse_line(&l))
        .collect();
    assert_eq!(
        extract_claude_reply_text(&claude_events),
        None,
        "claude parser must not extract a reply from real kimi frames"
    );
}
