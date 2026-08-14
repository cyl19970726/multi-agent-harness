//! Exact HTTP acceptance for the Member Execution Trust application service.
//!
//! The server resolves an authenticated actor and authority set from its
//! credential registry; headers and typed command bodies cannot select them.
//! These cases also pin absent-CAS,
//! scoped idempotent replay, payload conflict, and exact route matching.

mod firm_env;

use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};
use harness_store::HarnessStore;

const TOKEN: &str = "member-trust-http-test-token";

fn headers<'a>(key: &'a str, expected: &'a str) -> [(&'a str, &'a str); 3] {
    [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", key),
        ("If-Match", expected),
    ]
}

fn spoof_headers<'a>(key: &'a str, expected: &'a str) -> [(&'a str, &'a str); 7] {
    [
        ("X-AgentFirm-Token", TOKEN),
        ("X-AgentFirm-Actor-Kind", "service"),
        ("X-AgentFirm-Actor-Id", "impersonated-service"),
        ("X-AgentFirm-Authority-Kind", "human"),
        ("X-AgentFirm-Authority-Id", "impersonated-authority"),
        ("Idempotency-Key", key),
        ("If-Match", expected),
    ]
}

fn member_command(id: &str, name: &str, creator_id: &str) -> serde_json::Value {
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
            "created_by": {"kind": "human", "id": creator_id},
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
}

fn standalone_member_run_command(team_run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "command": "create_member_run",
        "run": {
            "id": "retired-standalone-member-run",
            "agent_member_id": "retired-standalone-member",
            "team_run_id": team_run_id,
            "role_snapshot": "implementer",
            "provider_profile_snapshot": "codex/codex_app_server",
            "requested_controls": {},
            "effective_controls": {},
            "coordination_status": "active",
            "runtime_status": "idle",
            "runtime_generation": 1,
            "workspace_binding_id": null,
            "native_session": null,
            "version": 1,
            "started_at": "unix-ms:1",
            "last_event_at": null,
            "finished_at": null
        }
    })
}

fn member_run_authority_counts(store: &HarnessStore) -> (usize, usize, usize) {
    (
        store.team_runs().expect("TeamRun projections").len(),
        store
            .member_runs()
            .expect("legacy runtime projections")
            .len(),
        store
            .canonical_operations()
            .expect("canonical trust operations")
            .len(),
    )
}

#[test]
fn exact_http_contract_is_authenticated_route_bound_and_replay_safe() {
    let home = TempHome::new("member-trust-http");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind": "human", "id": "host-http-test"},
        "authority_actors": [{"kind": "human", "id": "waiver-board"}]
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &project_root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let route = format!("/v1/agent-members?project={project_id}");
    let request = member_command("member-http-1", "HTTP Member", "host-http-test");

    let (status, body) = serve.post_json(&route, &request);
    assert_eq!(status, 401, "unauthenticated mutation: {body}");
    assert_eq!(body["error"]["code"], "UNAUTHORIZED_ACTOR");

    let hostile_headers = spoof_headers("header-spoof", "0");
    let (status, header_spoof) = serve.post_json_with_headers(&route, &request, &hostile_headers);
    assert_eq!(status, 401, "header identity spoof: {header_spoof}");
    assert_eq!(header_spoof["error"]["code"], "UNAUTHORIZED_ACTOR");

    let body_spoof = member_command("member-http-1", "HTTP Member", "body-spoof");
    let body_spoof_headers = headers("body-spoof", "0");
    let (status, body_spoof_response) =
        serve.post_json_with_headers(&route, &body_spoof, &body_spoof_headers);
    assert_eq!(status, 409, "body identity spoof: {body_spoof_response}");
    assert_eq!(body_spoof_response["error"]["code"], "UNAUTHORIZED_ACTOR");

    let request_headers = headers("create-member-http-1", "0");
    let (status, created) = serve.post_json_with_headers(&route, &request, &request_headers);
    assert_eq!(status, 200, "create failed: {created}");
    assert_eq!(created["protocol_version"], "agentfirm-member-trust/1");
    assert_eq!(created["projection"]["created_by"]["kind"], "human");
    assert_eq!(created["projection"]["created_by"]["id"], "host-http-test");
    assert_ne!(
        created["projection"]["created_by"]["id"],
        "impersonated-service"
    );
    assert_eq!(created["replayed"], false);

    let (status, replay) = serve.post_json_with_headers(&route, &request, &request_headers);
    assert_eq!(status, 200, "replay failed: {replay}");
    assert_eq!(replay["event_id"], created["event_id"]);
    assert_eq!(replay["store_sequence"], created["store_sequence"]);
    assert_eq!(replay["replayed"], true);

    let drifted = member_command(
        "member-http-1",
        "Different semantic payload",
        "host-http-test",
    );
    let (status, conflict) = serve.post_json_with_headers(&route, &drifted, &request_headers);
    assert_eq!(status, 409, "payload drift must conflict: {conflict}");
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    let wrong_route = format!("/v1/agent-members/member-http-2/pause?project={project_id}");
    let wrong_headers = headers("wrong-route", "0");
    let (status, mismatch) = serve.post_json_with_headers(&wrong_route, &request, &wrong_headers);
    assert_eq!(status, 400, "body/route mismatch: {mismatch}");
    assert_eq!(mismatch["error"]["code"], "INVALID_STATE_TRANSITION");
}

#[test]
fn retired_http_standalone_member_run_create_is_unknown_with_zero_authority_delta() {
    let home = TempHome::new("member-trust-retired-member-run-create");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind": "human", "id": "host-http-test"},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &project_root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let before = member_run_authority_counts(&store);
    let team_run_id = "retired-standalone-team-run";
    let route = format!("/v1/team-runs/{team_run_id}/member-runs?project={project_id}");
    let request_headers = headers("retired-member-run-create", "0");

    let (status, body) = serve.post_json_with_headers(
        &route,
        &standalone_member_run_command(team_run_id),
        &request_headers,
    );
    assert_eq!(status, 400, "retired endpoint must be unknown: {body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("unknown action path")),
        "retired endpoint must fail as an unknown action: {body}"
    );
    assert_eq!(
        member_run_authority_counts(&store),
        before,
        "retired HTTP endpoint must not create a TeamRun, legacy runtime projection, or canonical operation"
    );
}
