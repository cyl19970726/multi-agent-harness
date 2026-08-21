use super::*;

#[test]
fn provider_price_per_mtok_preserves_provider_rates() {
    assert_eq!(provider_price_per_mtok("claude"), (3.0, 15.0));
    assert_eq!(provider_price_per_mtok("codex"), (1.25, 10.0));
    assert_eq!(provider_price_per_mtok("gemini"), (1.25, 10.0));
    // Kimi has its own placeholder row (NOT priced as gpt-5-class), so spend
    // estimates don't wildly over-bound a cheaper provider
    // (goal-provider-neutral S4). Confirm it diverges from the default.
    assert_eq!(provider_price_per_mtok("kimi"), (0.60, 2.50));
    assert_ne!(
        provider_price_per_mtok("kimi"),
        provider_price_per_mtok("codex")
    );
}
