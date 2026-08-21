use super::*;

#[test]
fn claude_member_runtime_start_dispatches_to_claude_stub() {
    let root =
        std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("claude-start")));
    let store = HarnessStore::new(&root);
    let mut member = make_member("claude-agent");
    member.provider = "claude".into();

    let runtime = start_compatibility_delivery_runtime(&store, &member)
        .expect("claude runtime start dispatches to claude implementation");
    assert_eq!(
        runtime.provider, "claude",
        "runtime must have claude provider"
    );
    assert_eq!(runtime.command, "claude", "runtime must use claude command");
    assert!(
        runtime
            .control_endpoint
            .as_deref()
            .map(|ep| ep.starts_with("claude-runtime://"))
            .unwrap_or(false),
        "claude runtime must use claude-runtime:// endpoint"
    );
    assert!(
        runtime.pid.is_none(),
        "claude on-demand runtime should not have persistent PID"
    );

    let _ = std::fs::remove_dir_all(root);
}
