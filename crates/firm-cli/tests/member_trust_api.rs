//! Exact HTTP acceptance for the Member Execution Trust application service.
//!
//! The server receives an authenticated actor only from transport headers;
//! the typed command body cannot select it. These cases also pin absent-CAS,
//! scoped idempotent replay, payload conflict, and exact route matching.

mod firm_env;

use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};

const TOKEN: &str = "member-trust-http-test-token";

fn headers<'a>(key: &'a str, expected: &'a str) -> [(&'a str, &'a str); 5] {
    [
        ("X-AgentFirm-Token", TOKEN),
        ("X-AgentFirm-Actor-Kind", "human"),
        ("X-AgentFirm-Actor-Id", "host-http-test"),
        ("Idempotency-Key", key),
        ("If-Match", expected),
    ]
}

fn member_command(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "command": "create_agent_member",
        "member": {
            "id": id,
            "name": name,
            "description": "HTTP acceptance identity",
            "role": "worker",
            "capabilities": ["code"],
            "skill_refs": [],
            "provider_profile_ref": "codex-default",
            "model_preference": null,
            "workspace_policy": "managed-worktree",
            "permission_ceiling": "workspace_write",
            "organization_status": "active",
            "version": 1,
            "created_by": {"kind": "external", "id": "body-spoof"},
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
}

#[test]
fn exact_http_contract_is_authenticated_route_bound_and_replay_safe() {
    let home = TempHome::new("member-trust-http");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let serve = ServeHandle::spawn_with_env(
        &home,
        &project_root,
        &[],
        &[("AGENTFIRM_HTTP_MUTATION_TOKEN", TOKEN)],
    );
    let route = format!("/v1/agent-members?project={project_id}");
    let request = member_command("member-http-1", "HTTP Member");

    let (status, body) = serve.post_json(&route, &request);
    assert_eq!(status, 401, "unauthenticated mutation: {body}");
    assert_eq!(body["error"]["code"], "UNAUTHORIZED_ACTOR");

    let request_headers = headers("create-member-http-1", "0");
    let (status, created) = serve.post_json_with_headers(&route, &request, &request_headers);
    assert_eq!(status, 200, "create failed: {created}");
    assert_eq!(created["protocol_version"], "agentfirm-member-trust/1");
    assert_eq!(created["projection"]["created_by"]["kind"], "human");
    assert_eq!(created["projection"]["created_by"]["id"], "host-http-test");
    assert_eq!(created["replayed"], false);

    let (status, replay) = serve.post_json_with_headers(&route, &request, &request_headers);
    assert_eq!(status, 200, "replay failed: {replay}");
    assert_eq!(replay["event_id"], created["event_id"]);
    assert_eq!(replay["store_sequence"], created["store_sequence"]);
    assert_eq!(replay["replayed"], true);

    let drifted = member_command("member-http-1", "Different semantic payload");
    let (status, conflict) = serve.post_json_with_headers(&route, &drifted, &request_headers);
    assert_eq!(status, 409, "payload drift must conflict: {conflict}");
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    let wrong_route = format!("/v1/agent-members/member-http-2/pause?project={project_id}");
    let wrong_headers = headers("wrong-route", "0");
    let (status, mismatch) = serve.post_json_with_headers(&wrong_route, &request, &wrong_headers);
    assert_eq!(status, 400, "body/route mismatch: {mismatch}");
    assert_eq!(mismatch["error"]["code"], "INVALID_STATE_TRANSITION");
}
