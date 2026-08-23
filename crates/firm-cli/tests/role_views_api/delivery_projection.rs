use super::*;

/// DEV-24/25 end-to-end: the TeamWorkspace timeline must render the parent
/// Message's delivery from the same canonical per-recipient truth as the
/// delivery Activity row, carry the TeamMessage→Work correlation, and resolve
/// Host/Member actor labels server-side; the Host Agent Workspace must label
/// an external interactive Host session as external instead of implying a
/// managed provider runtime.
#[test]
fn delivery_projection_is_consistent_correlated_and_host_mode_is_labeled() {
    let home = TempHome::new("role-view-delivery-host-mode");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    assert!(run_firm(&home, &root, &["init"]).status.success());
    let project_id = current_project_id(&home);
    let space_id = current_space_id(&home);
    let host_thread_id = "acceptance-host-thread";
    let rollout_dir = home.home().join(".codex/sessions/2026/08/13");
    std::fs::create_dir_all(&rollout_dir).expect("Codex rollout fixture root");
    std::fs::write(
        rollout_dir.join(format!("rollout-2026-08-13T00-00-00-{host_thread_id}.jsonl")),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{host_thread_id}\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_reasoning\",\"turn_id\":\"turn-host-1\",\"text\":\"raw-chain-of-thought-must-not-appear\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"turn_id\":\"turn-host-1\",\"message\":\"display-safe host observation\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn-host-1\"}}}}\n"
        ),
    )
    .expect("Host rollout fixture");
    let run = |args: &[&str]| {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend_from_slice(args);
        let output = run_firm(&home, &root, &full);
        assert!(output.status.success(), "fixture {args:?}: {output:?}");
        output
    };
    let node: serde_json::Value =
        serde_json::from_slice(&run(&["node", "init"]).stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        &node_id,
        "--project-binding-id",
        &project_id,
    ]);
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    let mission_id = "mission-delivery-projection".to_string();
    firm_env::seed_historical_mission(
        &home,
        &project_id,
        &mission_id,
        "Delivery projection contract",
    );
    let host_id = "agent-acceptance-host";
    let member_id = "agent-acceptance-member";
    for (id, name, role) in [
        (host_id, "Acceptance Host", "host"),
        (member_id, "Acceptance Member", "builder"),
    ] {
        let created =
            create_canonical_agent_member(&home, &root, &project_id, id, name, role, "codex", &[]);
        assert!(created.status.success(), "AgentMember {id}: {created:?}");
    }
    run(&[
        "team",
        "create",
        "--name",
        "Acceptance Team",
        "--description",
        "Delivery projection acceptance",
        "--mission-id",
        &mission_id,
        "--host-agent-id",
        host_id,
        "--node-id",
        &node_id,
        "--member",
        host_id,
        "--member",
        member_id,
    ]);
    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    let team = store
        .latest_teams()
        .expect("teams")
        .into_values()
        .next()
        .expect("Team");
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind":"agent_member","id":host_id},
        "authority_actors": []
    },{
        "token": MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":member_id},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &["--space", &space_id],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let (status, created_run) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id":team.id,
            "objective":"Delivery projection acceptance",
            "host_surface":"codex",
            "host_thread_id":host_thread_id,
            "host_runtime_mode":"external_interactive",
            "members":[
                {"agent_member_id":host_id,"name":"host","role":"host","provider":"codex"},
                {"agent_member_id":member_id,"name":"member","role":"builder","provider":"codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_run_id = created_run["result"]["member_runs"][1]["id"]
        .as_str()
        .expect("MemberRun id")
        .to_string();
    // Materialize the runtime fabric (identities, sessions, memberships)
    // through the real NodeDaemon path, as in the canonical journey test.
    let (status, bootstrap) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id":"host",
            "recipient_runtime_ids":[member_run_id],
            "kind":"message",
            "body":"bootstrap canonical memberships and sessions"
        }),
    );
    assert_eq!(status, 200, "NodeDaemon bootstrap: {bootstrap}");

    // One Work-linked and one deliberately unlinked Host-authored message.
    let work_route = format!("/v1/agentfirm/team-runs/{run_id}/works?project={project_id}");
    let (status, created_work) = serve.post_json_with_headers(
        &work_route,
        &serde_json::json!({
            "action":"create_work",
            "work_id":"work-linked-1",
            "title":"Correlated delivery Work",
            "completion_criteria_markdown":"Delivery rows keep the Message's Work link",
            "claim_mode":"team_claim"
        }),
        &action_headers(TOKEN, "create-work-linked-1", "0"),
    );
    assert_eq!(status, 200, "create Work: {created_work}");
    let message_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/send?project={project_id}");
    let host_console_route = format!("/v1/views/host-console/{}?project={project_id}", team.id);
    let team_revision = |serve: &ServeHandle| {
        let (status, view) =
            serve.get_json_with_headers(&host_console_route, &[("X-AgentFirm-Token", TOKEN)]);
        assert_eq!(status, 200, "Host console for send revision: {view}");
        view["allowed_actions"]
            .as_array()
            .and_then(|actions| {
                actions
                    .iter()
                    .find(|action| action["kind"] == "send_message")
            })
            .and_then(|action| action["required_version"].as_u64())
            .expect("send_message required version")
            .to_string()
    };
    let send = |key: &str, body: &str, work_id: Option<&str>| {
        let version = team_revision(&serve);
        let mut intent = serde_json::json!({
            "action":"send_message",
            "recipient_ids":[member_id],
            "body":body,
        });
        if let Some(work_id) = work_id {
            intent["work_id"] = serde_json::json!(work_id);
        }
        let (status, response) = serve.post_json_with_headers(
            &message_route,
            &intent,
            &action_headers(TOKEN, key, &version),
        );
        assert_eq!(status, 200, "send_message {key}: {response}");
        response["projection"]["id"]
            .as_str()
            .expect("message id")
            .to_string()
    };
    let linked_message_id = send(
        "send-linked-1",
        "Work-linked Host note",
        Some("work-linked-1"),
    );
    let unlinked_message_id = send("send-unlinked-1", "Deliberately unlinked Host note", None);

    // Drive the Work-linked delivery to its authoritative acknowledged state.
    let lease = store
        .latest_node_daemon_lease(&node_id)
        .expect("daemon lease")
        .expect("current daemon lease");
    let linked_delivery = store
        .fabric_message_deliveries(&space_id)
        .expect("canonical deliveries")
        .into_iter()
        .find(|delivery| {
            delivery.message_id == linked_message_id
                && delivery.recipient_agent_member_id.as_deref() == Some(member_id)
        })
        .expect("Work-linked delivery");
    assert_eq!(
        linked_delivery.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
    );
    let daemon_context = |command: &str, key: &str| MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: lease.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version: 0,
        request_fingerprint: None,
    };
    store
        .claim_message_for_provider(
            &daemon_context("test.node_daemon.message_claim", "claim-linked"),
            &linked_delivery.id,
            &node_id,
            &lease.daemon_id,
            lease.generation,
            "claim-linked",
            RuntimeDispatchMode::QueueOnly,
            "unix-ms:100",
        )
        .expect("claim linked delivery");
    store
        .record_message_provider_receipt(
            &daemon_context("test.node_daemon.message_receipt", "receipt-linked"),
            &linked_delivery.id,
            &node_id,
            &lease.daemon_id,
            lease.generation,
            "claim-linked",
            "provider-receipt-linked",
            "unix-ms:101",
        )
        .expect("provider receipt");
    store
        .acknowledge_message_delivery(
            &MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: member_id.into(),
                },
                authority_actor: None,
                command_name: "test.agent_session.message_ack".into(),
                idempotency_key: "ack-linked".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &linked_delivery.id,
            "unix-ms:102",
        )
        .expect("recipient ACK");

    // Issue 2: the parent Message and the delivery Activity row present one
    // authoritative status, the Work correlation survives, and actor labels
    // resolve server-side.
    let team_workspace_route = format!("/v1/views/team-workspace/{}?project={project_id}", team.id);
    let (status, workspace) =
        serve.get_json_with_headers(&team_workspace_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "TeamWorkspace: {workspace}");
    let messages = workspace["data"]["messages"].as_array().expect("messages");
    let linked = messages
        .iter()
        .find(|message| message["message_id"] == linked_message_id)
        .expect("linked message summary");
    assert_eq!(linked["work_id"], "work-linked-1");
    assert_eq!(linked["delivery_state"], "acknowledged");
    assert_eq!(
        linked["sender"]["display_name"], "Acceptance Host",
        "Host actor label resolves to the durable name, not the raw id"
    );
    let linked_delivery_summary = &linked["deliveries"][0];
    assert_eq!(linked_delivery_summary["status"], "acknowledged");
    assert_eq!(linked_delivery_summary["recipient_identity_id"], member_id);
    assert_eq!(
        linked_delivery_summary["recipient_display_name"],
        "Acceptance Member"
    );
    let unlinked = messages
        .iter()
        .find(|message| message["message_id"] == unlinked_message_id)
        .expect("unlinked message summary");
    assert_eq!(unlinked["work_id"], serde_json::Value::Null);
    assert_eq!(
        unlinked["delivery_state"], "queued",
        "queued and acknowledged remain distinguishable on the parent Message"
    );

    let activity = workspace["data"]["activity"].as_array().expect("activity");
    let linked_delivery_row = activity
        .iter()
        .find(|row| row["source"] == "message_delivery" && row["id"] == linked_delivery.id)
        .expect("linked delivery activity row");
    assert_eq!(
        linked_delivery_row["status"], "acknowledged",
        "Activity row must match the parent Message's authoritative status"
    );
    assert_eq!(linked_delivery_row["message_id"], linked_message_id);
    assert_eq!(
        linked_delivery_row["work_id"], "work-linked-1",
        "canonical delivery fact inherits the authored Message's Work link"
    );
    assert_eq!(linked_delivery_row["actor_ref"]["id"], member_id);
    assert_eq!(
        linked_delivery_row["actor_ref"]["display_name"],
        "Acceptance Member"
    );
    let unlinked_delivery_row = activity
        .iter()
        .find(|row| row["source"] == "message_delivery" && row["message_id"] == unlinked_message_id)
        .expect("unlinked delivery activity row");
    assert_eq!(
        unlinked_delivery_row["work_id"],
        serde_json::Value::Null,
        "an unlinked Message never acquires an invented Work link"
    );
    let host_message_row = activity
        .iter()
        .find(|row| row["source"] == "message" && row["id"] == linked_message_id)
        .expect("Host-authored activity row");
    assert_eq!(host_message_row["actor_ref"]["id"], host_id);
    assert_eq!(
        host_message_row["actor_ref"]["display_name"], "Acceptance Host",
        "Host-authored Activity resolves the durable Host label"
    );

    // Issue 1: the exact Host self view carries the external-interactive mode
    // label, the owner-private Session projection, the transient live slot,
    // and the authorized conversation surface.
    let host_workspace_route =
        format!("/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={host_id}");
    let (status, host_workspace) =
        serve.get_json_with_headers(&host_workspace_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Host AgentWorkspace: {host_workspace}");
    let host_selected = &host_workspace["data"]["selected_agent"];
    assert_eq!(host_selected["is_host"], true);
    assert_eq!(
        host_selected["host_session_mode"], "external_interactive",
        "an external Codex session as Host must be labeled external, never a managed runtime"
    );
    let host_roster = host_workspace["data"]["roster"]
        .as_array()
        .expect("roster")
        .iter()
        .find(|member| member["agent_member_ref"]["id"] == host_id)
        .expect("Host roster entry");
    assert_eq!(host_roster["host_session_mode"], "external_interactive");
    let host_projection = &host_workspace["data"]["session_event_projection"];
    assert_eq!(
        host_projection["disabled_reason"],
        "No provider-native Session is bound to this selected Agent run."
    );
    assert_eq!(host_projection["agent_session_id"], serde_json::Value::Null);
    assert_eq!(host_projection["episodes"], serde_json::json!([]));
    assert!(
        host_workspace["data"]
            .get("live_provider_activity")
            .is_some(),
        "Host self view carries the nullable transient live slot"
    );
    assert!(host_workspace["allowed_actions"]
        .as_array()
        .expect("Host actions")
        .iter()
        .any(|action| action["kind"] == "send_message" && action["disabled_reason"].is_null()));

    // External-interactive ownership comes from the exact Host MemberRun, not
    // from an optional provider thread locator. Missing thread remains
    // pull-only and never fabricates an AgentSession.
    let (status, unbound_run) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id":team.id,
            "objective":"Unbound Host attempt",
            "members":[{"agent_member_id":member_id,"name":"member","role":"builder","provider":"codex"}]
        }),
    );
    assert_eq!(status, 200, "unbound TeamRun: {unbound_run}");
    let unbound_run_id = unbound_run["result"]["team_run"]["id"]
        .as_str()
        .expect("unbound run id");
    let (status, unbound_workspace) = serve.get_json_with_headers(
        &format!(
            "/v1/views/agent-workspace/{unbound_run_id}?project={project_id}&agent_id={host_id}"
        ),
        &[("X-AgentFirm-Token", TOKEN)],
    );
    assert_eq!(status, 200, "unbound Host view: {unbound_workspace}");
    assert_eq!(
        unbound_workspace["data"]["selected_agent"]["host_session_mode"],
        "external_interactive"
    );
    assert_eq!(
        unbound_workspace["data"]["session_event_projection"]["disabled_reason"],
        "No provider-native Session is bound to this selected Agent run."
    );
}
