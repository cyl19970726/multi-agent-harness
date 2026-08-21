use super::*;

#[test]
fn ordinary_message_intent_treats_operator_and_service_as_coordination_plane() {
    // ADR 0012: the Dashboard is the control plane, so an Operator reply
    // must wake an idle member exactly like a Host reply. Routed Company OS
    // inbox mail arrives as a Service sender and must execute, not idle.
    for actor_kind in [
        TeamActorKind::Host,
        TeamActorKind::Operator,
        TeamActorKind::Service,
    ] {
        let mut message = bare_team_message(ProviderDispatchIntent::Message);
        message.sender_runtime_id = format!("{actor_kind:?}-sender");
        message.sender = Some(TeamActorRef {
            kind: actor_kind,
            id: "sender-1".to_string(),
            display_name: None,
            authn_source: None,
        });
        assert!(message.requires_response(), "{actor_kind:?}");
    }
}
