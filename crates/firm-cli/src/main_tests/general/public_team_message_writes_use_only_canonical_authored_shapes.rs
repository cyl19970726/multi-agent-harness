use super::*;

#[test]
fn public_team_message_writes_use_only_canonical_authored_shapes() {
    assert!(parse_team_message_kind("assignment").is_err());
    assert!(matches!(
        parse_team_message_kind("message"),
        Ok(ProviderDispatchIntent::Message)
    ));
    assert!(parse_team_message_kind("handoff").is_err());
    assert!(matches!(
        parse_team_message_kind("control"),
        Ok(ProviderDispatchIntent::Control)
    ));

    for historical in [
        "question",
        "answer",
        "progress",
        "blocker",
        "review_request",
        "review_result",
        "plan_request",
        "plan_proposal",
        "plan_feedback",
        "plan_approval",
        "broadcast",
    ] {
        parse_team_message_kind(historical)
            .expect_err("historical message kinds must not be authored");
    }
}
