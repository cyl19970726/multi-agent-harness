use super::*;

#[test]
fn historical_rows_without_sender_fall_back_to_from_member_id() {
    let mut historical = bare_team_message(ProviderDispatchIntent::Message);
    historical.sender = None;
    assert!(historical.requires_response(), "sender_runtime_id == host");
    historical.sender_runtime_id = "member-run-9".to_string();
    assert!(
        !historical.requires_response(),
        "historical peer mail stays informational"
    );
}
