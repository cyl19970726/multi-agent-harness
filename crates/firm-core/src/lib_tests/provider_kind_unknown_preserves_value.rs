use super::*;

#[test]
fn provider_kind_unknown_preserves_value() {
    let kind = ProviderKind::from("gemini");
    assert_eq!(kind, ProviderKind::Unknown("gemini".to_string()));
    // Unknown providers round-trip without losing fidelity.
    assert_eq!(kind.to_string(), "gemini");
    assert_eq!(ProviderKind::from("gemini".to_string()), kind);
}
