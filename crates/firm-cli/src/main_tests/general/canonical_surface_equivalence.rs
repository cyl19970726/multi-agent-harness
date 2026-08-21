
#[test]
fn canonical_surface_equivalence() {
    use harness_store::canonical_surface;

    // kimi family
    assert_eq!(canonical_surface("kimi"), "kimi");
    assert_eq!(canonical_surface("kimi-cli"), "kimi");
    assert_eq!(canonical_surface("kimi-code"), "kimi");

    // codex family
    assert_eq!(canonical_surface("codex"), "codex");
    assert_eq!(canonical_surface("codex-app"), "codex");
    assert_eq!(canonical_surface("codex-app-server"), "codex");

    // claude family
    assert_eq!(canonical_surface("claude"), "claude");
    assert_eq!(canonical_surface("claude-code"), "claude");

    // Unknown surfaces pass through
    assert_eq!(canonical_surface("cli"), "cli");
    assert_eq!(canonical_surface("http"), "http");
    assert_eq!(canonical_surface(""), "");
    assert_eq!(canonical_surface("custom-agent"), "custom-agent");
}
