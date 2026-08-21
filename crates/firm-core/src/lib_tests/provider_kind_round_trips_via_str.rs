use super::*;

#[test]
fn provider_kind_round_trips_via_str() {
    for (input, expected) in [
        ("codex", ProviderKind::Codex),
        ("claude", ProviderKind::Claude),
    ] {
        let kind = ProviderKind::from(input);
        assert_eq!(kind, expected);
        // Display must reproduce the original provider string verbatim.
        assert_eq!(kind.to_string(), input);
        assert_eq!(kind.as_str(), input);
    }
}
