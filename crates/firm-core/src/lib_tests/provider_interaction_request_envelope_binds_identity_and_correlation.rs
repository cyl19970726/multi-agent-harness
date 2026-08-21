use super::*;

#[test]
fn provider_interaction_request_envelope_binds_identity_and_correlation() {
    let body = provider_interaction_request_body();
    let mut message = bare_team_message(ProviderDispatchIntent::ProviderInteractionRequest);
    message.sender_runtime_id = body.member.clone();
    message.sender = Some(TeamActorRef {
        kind: TeamActorKind::ProviderRuntimeProjection,
        id: body.member.clone(),
        display_name: None,
        authn_source: Some("provider_reverse_rpc".to_string()),
    });
    message.recipients = vec![TeamRecipientRef {
        kind: TeamRecipientKind::Host,
        id: "host".to_string(),
    }];
    message.recipient_runtime_ids = vec!["host".to_string()];
    message.deliveries = vec![ProviderDispatchAttempt {
        member_id: "host".to_string(),
        policy: TeamDeliveryPolicy::ManualAck,
        status: TeamDeliveryStatus::Delivered,
        attempt: 1,
        claim_id: None,
        claimed_by_supervisor_id: None,
        claimed_generation: None,
        claimed_unix_ms: None,
        claim_expires_unix_ms: None,
        provider_receipt_id: Some("host-receipt".to_string()),
        failure_reason: None,
        updated_at: "now".to_string(),
    }];
    message.correlation_id = body.correlation_id();
    message.body = body.to_canonical_json().expect("body");
    message
        .validate_provider_interaction_contract()
        .expect("valid request envelope");

    message.correlation_id = "caller-chosen".to_string();
    assert!(message
        .validate_provider_interaction_contract()
        .expect_err("unstable correlation")
        .contains("correlation_id"));

    message.correlation_id = body.correlation_id();
    message.response_intent = Some(ProviderResponseIntent::Informational);
    assert!(message
        .validate_provider_interaction_contract()
        .expect_err("request cannot suppress response")
        .contains("must require"));

    message.response_intent = Some(ProviderResponseIntent::ResponseRequired);
    message.sender.as_mut().expect("sender").id = "forged-member".to_string();
    assert!(message
        .validate_provider_interaction_contract()
        .expect_err("request sender is member-bound")
        .contains("sender"));
}
