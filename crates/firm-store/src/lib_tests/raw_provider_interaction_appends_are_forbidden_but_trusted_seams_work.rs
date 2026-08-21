use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn raw_provider_interaction_appends_are_forbidden_but_trusted_seams_work() {
    let root = team_test_root("provider-interaction-raw-append");
    let store = HarnessStore::new(&root);
    let (request_body, request) =
        seed_provider_interaction_bridge(&store, "run-interaction-raw-append");

    let mut raw_request = request.clone();
    raw_request.id = "raw-provider-request".into();
    raw_request.body = r#"{"type":"question","unknown":true}"#.into();
    assert!(store
        .append_team_message(&raw_request)
        .expect_err("raw provider request must be forbidden")
        .to_string()
        .contains("RAW_APPEND_FORBIDDEN"));

    let queued_response = provider_interaction_response(&request_body, &request, "continue");
    assert!(store
        .append_team_message(&queued_response)
        .expect_err("even valid queued response requires atomic record seam")
        .to_string()
        .contains("RAW_APPEND_FORBIDDEN"));

    let mut delivered_response = queued_response.clone();
    delivered_response.deliveries[0].status = TeamDeliveryStatus::Delivered;
    delivered_response.deliveries[0].provider_receipt_id = Some("forged-receipt".into());
    assert!(store
        .append_team_message(&delivered_response)
        .expect_err("raw Delivered response must be forbidden")
        .to_string()
        .contains("RAW_APPEND_FORBIDDEN"));

    let recorded = store
        .record_provider_interaction_response(&queued_response, "unix-ms:4")
        .expect("trusted response seam remains legal");
    assert_eq!(recorded.id, queued_response.id);
    assert_eq!(recorded.deliveries[0].status, TeamDeliveryStatus::Queued);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
