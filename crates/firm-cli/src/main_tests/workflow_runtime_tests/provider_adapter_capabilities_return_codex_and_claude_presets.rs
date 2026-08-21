use super::*;

#[test]
fn provider_adapter_capabilities_return_codex_and_claude_presets() {
    // goal-provider-neutral S1: codex/claude adapters expose their real
    // capability presets through the trait, and every registered adapter
    // resolves a non-panicking capability set (default impl covers new
    // providers).
    assert_eq!(
        CodexAdapter.capabilities(),
        ProviderCapabilities::codex_exec(),
        "codex adapter must report the codex_exec preset"
    );
    assert_eq!(
        ClaudeAdapter.capabilities(),
        ProviderCapabilities::claude_exec(),
        "claude adapter must report the claude_exec preset"
    );
    // Resolved through the registry by id, too.
    let codex = provider_adapter("codex").expect("codex registered");
    assert_eq!(codex.capabilities(), ProviderCapabilities::codex_exec());
    let claude = provider_adapter("claude").expect("claude registered");
    assert_eq!(claude.capabilities(), ProviderCapabilities::claude_exec());
    // Kimi (goal-provider-neutral S4): registered as a third provider with an
    // explicit, honestly-degraded capability preset (only streaming claimed).
    assert_eq!(
        KimiAdapter.capabilities(),
        ProviderCapabilities::kimi_exec(),
        "kimi adapter must report the kimi_exec preset"
    );
    let kimi = provider_adapter("kimi").expect("kimi registered");
    assert_eq!(kimi.capabilities(), ProviderCapabilities::kimi_exec());
    assert_eq!(kimi.name(), "kimi");
    assert_eq!(kimi.live_ndjson_file_name(), "kimi.stream-json.ndjson");
    // Kimi marks its unverified axes false (degraded-until-proven), unlike
    // claude which has confirmed schema/cost.
    assert!(!kimi.capabilities().schema, "kimi schema is S3-spike TBD");
    assert!(!kimi.capabilities().cost, "kimi cost is S3-spike TBD");
    assert!(!kimi.capabilities().resume, "kimi resume is S3-spike TBD");
    // Read-only enforcement: codex (--sandbox read-only) and claude (read-only
    // tool allowlist) PHYSICALLY enforce read-only; kimi -p has no read-only
    // mode (rejects every permission flag). This is capability metadata only;
    // read-only workflow leaves still run in the selected project root.
    assert!(
        codex.capabilities().enforces_read_only,
        "codex enforces read-only via --sandbox read-only"
    );
    assert!(
        claude.capabilities().enforces_read_only,
        "claude enforces read-only via a read-only tool allowlist"
    );
    assert!(
        !kimi.capabilities().enforces_read_only,
        "kimi -p has no read-only mode"
    );
    // supported_provider_names() is the single source of truth and now lists kimi.
    assert!(
        supported_provider_names().contains(&"kimi"),
        "registry-derived provider list must include kimi"
    );
    // Every registered adapter answers capabilities() without panicking.
    for adapter in provider_registry() {
        let caps = adapter.capabilities();
        assert!(
            caps.streaming,
            "{} should support streaming exec",
            adapter.name()
        );
    }
}
