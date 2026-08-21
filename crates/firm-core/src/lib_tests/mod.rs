use super::*;

fn bare_team_message(kind: ProviderDispatchIntent) -> TeamMessageProjection {
    TeamMessageProjection {
        id: "tmsg-1".to_string(),
        team_run_id: "run-1".to_string(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".to_string(),
        recipients: Vec::new(),
        recipient_runtime_ids: Vec::new(),
        kind,
        body: "body".to_string(),
        correlation_id: "corr-1".to_string(),
        causation_id: None,
        response_intent: None,
        evidence_refs: Vec::new(),
        deliveries: Vec::new(),
        created_at: "now".to_string(),
    }
}

fn peer_team_message(kind: ProviderDispatchIntent) -> TeamMessageProjection {
    let mut message = bare_team_message(kind);
    message.sender_runtime_id = "member-run-2".to_string();
    message.sender = Some(TeamActorRef {
        kind: TeamActorKind::ProviderRuntimeProjection,
        id: "member-run-2".to_string(),
        display_name: None,
        authn_source: None,
    });
    message
}

fn provider_interaction_request_body() -> ProviderInteractionRequestBody {
    ProviderInteractionRequestBody {
        interaction_type: ProviderInteractionType::Question,
        prompt: "Choose a path".to_string(),
        options: vec![ProviderInteractionMessageOption {
            id: "yes".to_string(),
            label: "Continue".to_string(),
            intent: Some("approve".to_string()),
        }],
        provider: "codex".to_string(),
        provider_request_id: "request-7".to_string(),
        method: "item/tool/requestUserInput".to_string(),
        session: "thread-9".to_string(),
        member: "member-run-2".to_string(),
        generation: 3,
    }
}

mod historical_rows_without_sender_fall_back_to_from_member_id;
mod ordinary_message_intent_treats_operator_and_service_as_coordination_plane;
mod ordinary_message_response_intent_defaults_from_sender;
mod provider_interaction_body_is_strict_canonical_json;
mod provider_interaction_request_envelope_binds_identity_and_correlation;
mod provider_interaction_response_requires_one_answer_branch;
mod provider_kind_round_trips_via_str;
mod provider_kind_unknown_preserves_value;
mod provider_price_per_mtok_preserves_provider_rates;
mod team_message_explicit_response_intent_wins_over_kind_default;
mod team_message_response_intent_defaults_from_kind;
