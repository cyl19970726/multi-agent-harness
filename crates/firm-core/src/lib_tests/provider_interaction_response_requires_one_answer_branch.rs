use super::*;

#[test]
fn provider_interaction_response_requires_one_answer_branch() {
    let response = ProviderInteractionResponseBody {
        interaction_type: ProviderInteractionType::Question,
        choice: Some("yes".to_string()),
        text: None,
        session: "thread-9".to_string(),
        member: "member-run-2".to_string(),
        generation: 3,
    };
    let canonical = response.to_canonical_json().expect("choice response");
    assert_eq!(
        ProviderInteractionResponseBody::parse_canonical_json(&canonical).expect("parse"),
        response
    );

    let mut both = response.clone();
    both.text = Some("also text".to_string());
    assert!(both
        .validate()
        .expect_err("mutually exclusive")
        .contains("mutually"));

    let mut approval_text = response;
    approval_text.interaction_type = ProviderInteractionType::ToolApproval;
    approval_text.choice = None;
    approval_text.text = Some("yes".to_string());
    assert!(approval_text
        .validate()
        .expect_err("approval must choose")
        .contains("free text"));
    assert_eq!(
        provider_interaction_response_id("request-7").expect("stable id"),
        "provider-interaction-response:9:request-7"
    );
    assert!(provider_interaction_response_id("  ").is_err());
}
