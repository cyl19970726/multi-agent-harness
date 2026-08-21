use super::*;

#[test]
fn ordinary_message_response_intent_defaults_from_sender() {
    for kind in [
        ProviderDispatchIntent::Message,
        ProviderDispatchIntent::Message,
        ProviderDispatchIntent::Message,
        ProviderDispatchIntent::Message,
        ProviderDispatchIntent::Message,
    ] {
        // `message` is the only legal carrier for Host questions,
        // revisions, and acceptance decisions: Host mail must stay waking.
        assert!(
            bare_team_message(kind).requires_response(),
            "host {kind:?} must default to response_required"
        );
        // Peer-to-peer confirmations converge without a new round.
        assert!(
            !peer_team_message(kind).requires_response(),
            "peer {kind:?} must default to informational"
        );
    }
}
