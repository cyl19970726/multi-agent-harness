use super::*;

#[test]
fn unauthenticated_team_member_inbox_http_route_is_retired() {
    let home = TempHome::new("team-inbox-http");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise member inbox",
            "members": [
                {"name": "member-a", "role": "builder", "provider": "codex"},
                {"name": "member-b", "role": "reviewer", "provider": "codex"}
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
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "Please review the shared Work board"
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let (status, inbox) =
        serve.get_json(&format!("/v1/team-runs/{run_id}/members/{member_id}/inbox"));
    assert_eq!(status, 410, "body: {inbox}");
    assert_eq!(
        inbox["error"]["code"].as_str(),
        Some("RETIRED_RUNTIME_READER")
    );
    let (status, all) = serve.get_json(&format!(
        "/v1/team-runs/{run_id}/members/{member_id}/inbox?all=true"
    ));
    assert_eq!(status, 410, "body: {all}");
    assert_eq!(
        all["error"]["code"].as_str(),
        Some("RETIRED_RUNTIME_READER")
    );
}
