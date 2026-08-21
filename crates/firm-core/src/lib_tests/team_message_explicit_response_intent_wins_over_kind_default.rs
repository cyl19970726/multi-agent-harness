use super::*;

#[test]
fn team_message_explicit_response_intent_wins_over_kind_default() {
    // Override upward: an ack-only peer note that genuinely needs action.
    let mut ack_only = peer_team_message(ProviderDispatchIntent::Message);
    assert_eq!(
        ack_only.effective_response_intent(),
        ProviderResponseIntent::Informational
    );
    ack_only.response_intent = Some(ProviderResponseIntent::ResponseRequired);
    assert!(ack_only.requires_response());
    // Override downward: Host mail that is deliberately FYI-only.
    let mut host_fyi = bare_team_message(ProviderDispatchIntent::Message);
    assert!(host_fyi.requires_response());
    host_fyi.response_intent = Some(ProviderResponseIntent::Informational);
    assert!(!host_fyi.requires_response());
    // Override downward on a work-carrying kind too.
    let mut control = bare_team_message(ProviderDispatchIntent::Control);
    control.response_intent = Some(ProviderResponseIntent::Informational);
    assert!(!control.requires_response());
    // The explicit field round-trips through serde; an absent field keeps
    // historical rows on their kind+sender-derived default.
    let json = serde_json::to_string(&ack_only).expect("serialize");
    assert!(json.contains("\"response_intent\":\"response_required\""));
    let without = peer_team_message(ProviderDispatchIntent::Message);
    let json = serde_json::to_string(&without).expect("serialize");
    assert!(!json.contains("response_intent"));
    let historical: TeamMessageProjection =
        serde_json::from_str(&json).expect("deserialize without the field");
    assert_eq!(historical.response_intent, None);
    assert!(!historical.requires_response());
}
