use super::*;

#[test]
fn unknown_provider_runtime_start_fails_fast() {
    let root = std::env::temp_dir().join(format!(
        "harness-cli-test-{}",
        generated_id("unknown-start")
    ));
    let store = HarnessStore::new(&root);
    let mut member = make_member("gemini-agent");
    member.provider = "gemini".into();

    let error = start_compatibility_delivery_runtime(&store, &member)
        .expect_err("unknown provider must fail fast rather than assume codex");
    let message = error.to_string();
    // Assert the EXACT message: the supported list is now derived from the
    // provider registry, so this guards against ordering/spacing/list drift
    // (which a substring check would silently miss). Pi is the fourth
    // registered provider.
    assert_eq!(
        message,
        "unknown provider \"gemini\" for runtime start; supported providers: codex, claude, kimi"
    );

    let _ = std::fs::remove_dir_all(root);
}
