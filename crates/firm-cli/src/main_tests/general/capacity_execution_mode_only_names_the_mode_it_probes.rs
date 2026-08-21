use super::*;

#[test]
fn capacity_execution_mode_only_names_the_mode_it_probes() {
    for (provider, mode) in [
        ("codex", "codex_app_server"),
        ("claude", "claude_agent_sdk"),
        ("kimi", "kimi_acp"),
    ] {
        assert_eq!(capacity_execution_mode(provider, None).unwrap(), mode);
        assert_eq!(capacity_execution_mode(provider, Some(mode)).unwrap(), mode);
    }
    // A capacity claim is never carried across execution modes: asking for
    // a mode this preflight does not probe is refused rather than answered
    // with another mode's observation under that label.
    let error = capacity_execution_mode("codex", Some("codex_exec"))
        .expect_err("codex_exec is not the probed mode");
    assert!(error.to_string().contains("codex_app_server"), "{error}");
    assert!(capacity_execution_mode("codex", Some("totally-bogus")).is_err());
}
