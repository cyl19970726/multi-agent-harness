use super::*;

#[test]
fn canonical_team_message_routes_member_to_host_identity_without_special_inbox_authority() {
    let home = TempHome::new("host-inbox-http");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise native Host inbox",
            "host_surface": "codex-app",
            "host_thread_id": "codex-thread-http-a",
            "members": [
                {"name": "member-a", "role": "builder", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id");
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": member_id,
            "recipient_runtime_ids": ["host"],
            "kind": "message",
            "body": "QUESTION: choose A or B",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let store = HarnessStore::new(serve.fixture_store_root());
    let canonical_id = sent["result"]["id"].as_str().expect("canonical Message id");
    let delivery = store
        .fabric_message_deliveries(&current_space_id(&home))
        .expect("canonical deliveries")
        .into_iter()
        .find(|delivery| delivery.message_id == canonical_id)
        .expect("Host identity delivery");
    assert_ne!(delivery.recipient_agent_member_id.as_deref(), Some("host"));
    assert_eq!(delivery.status, CanonicalMessageDeliveryStatus::Queued);

    let (status, exact) =
        serve.get_json("/v1/team-runs/host-inbox?surface=codex-app&thread_id=codex-thread-http-a");
    assert_eq!(status, 200, "body: {exact}");
    assert_eq!(exact["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        exact["runs"][0]["messages"][0]["id"].as_str(),
        Some(canonical_id),
        "native Host inbox is a projection of canonical MessageDelivery, not a special host ledger: {exact}"
    );

    let (status, other) =
        serve.get_json("/v1/team-runs/host-inbox?surface=codex-app&thread_id=another-thread");
    assert_eq!(status, 200, "body: {other}");
    assert_eq!(other["runs"].as_array().map(Vec::len), Some(0));
}
