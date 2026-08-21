use super::*;

#[test]
fn team_message_response_intent_defaults_from_kind() {
    // Coordination-plane messages and runtime-control kinds require a
    // response; peer informational messages do not create ping-pong.
    assert!(bare_team_message(ProviderDispatchIntent::Message).requires_response());
    assert!(!peer_team_message(ProviderDispatchIntent::Message).requires_response());
    for kind in [
        ProviderDispatchIntent::Control,
        ProviderDispatchIntent::ProviderInteractionRequest,
    ] {
        assert!(bare_team_message(kind).requires_response(), "{kind:?}");
        assert!(peer_team_message(kind).requires_response(), "{kind:?}");
    }
    assert!(
        !bare_team_message(ProviderDispatchIntent::ProviderInteractionResponse).requires_response()
    );
    assert!(
        !peer_team_message(ProviderDispatchIntent::ProviderInteractionResponse).requires_response()
    );
}
