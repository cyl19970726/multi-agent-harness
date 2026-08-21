use super::*;

#[test]
fn parse_worker_model_reads_claude_init_and_ignores_codex() {
    let claude = vec![
        serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-4-8"}),
        serde_json::json!({"type": "result", "usage": {"input_tokens": 1, "output_tokens": 1}}),
    ];
    assert_eq!(
        parse_worker_model(&claude).as_deref(),
        Some("claude-opus-4-8")
    );
    // codex exec --json carries no system/model frame.
    let codex = vec![
        serde_json::json!({"type": "thread.started"}),
        serde_json::json!({"type": "turn.completed", "usage": {"input_tokens": 1}}),
    ];
    assert_eq!(parse_worker_model(&codex), None);
    assert_eq!(parse_worker_model(&[]), None);
}
