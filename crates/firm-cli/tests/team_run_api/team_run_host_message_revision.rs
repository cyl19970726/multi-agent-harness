use super::*;

const HOST_TOKEN: &str = "host-message-revision-token";

#[test]
fn host_cli_send_revision_matches_host_console_send_message_action() {
    let home = TempHome::new("team-run-host-message-revision");
    let project_id = init_project(&home, "alpha");
    let credentials = serde_json::json!([{
        "token": HOST_TOKEN,
        "actor": {"kind": "agent_member", "id": FIXTURE_HOST_ID},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let host_thread_id = "host-message-revision-thread";
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Keep Host CLI and Host Console on one Team revision",
            "host_surface": "codex-app",
            "host_thread_id": host_thread_id,
            "host_runtime_mode": "external_interactive",
            "members": [
                {"agent_member_id": "worker", "name": "worker", "role": "builder", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id");
    let member_run_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("MemberRun id");

    let (status, bootstrap) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_run_id],
            "kind": "message",
            "body": "bootstrap canonical message fabric"
        }),
    );
    assert_eq!(status, 200, "NodeDaemon bootstrap: {bootstrap}");

    let host_console_route =
        format!("/v1/views/host-console/{FIXTURE_TEAM_ID}?project={project_id}");
    let (status, host_console) =
        serve.get_json_with_headers(&host_console_route, &[("X-AgentFirm-Token", HOST_TOKEN)]);
    assert_eq!(status, 200, "Host Console: {host_console}");
    let host_console_revision = host_console["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "send_message")
        })
        .and_then(|action| action["required_version"].as_u64())
        .expect("Host Console send_message required_version");

    let store = HarnessStore::new(serve.fixture_store_root());
    let cli_team_revision = store
        .teams()
        .expect("Team rows")
        .into_iter()
        .filter(|team| team.id == FIXTURE_TEAM_ID)
        .count() as u64;
    assert_eq!(
        cli_team_revision, host_console_revision,
        "CLI and Host Console must derive the same durable Team revision"
    );
    // The canonical application seam rejects any other expected Team revision,
    // so success below proves the CLI submitted the advertised value.
    let membership = store
        .fabric_team_memberships(&current_space_id(&home))
        .expect("TeamMemberships")
        .into_iter()
        .find(|membership| {
            membership.team_id == FIXTURE_TEAM_ID && membership.agent_member_id == "worker"
        })
        .expect("worker TeamMembership");
    let send = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "message",
            "send",
            "--team-run-id",
            run_id,
            "--to-membership",
            &membership.id,
            "--body",
            "shared Team revision",
            "--surface",
            "codex-app",
            "--thread-id",
            host_thread_id,
            "--idempotency-key",
            "host-message-shared-team-revision",
        ],
    );
    assert!(
        send.status.success(),
        "Host CLI send rejected the Host Console revision: {send:?}"
    );
}
