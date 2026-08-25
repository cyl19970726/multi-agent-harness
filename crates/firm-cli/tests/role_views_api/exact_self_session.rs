use super::*;

/// DEV-21 end-to-end: a fresh Team start materializes the MemberRun and
/// AgentSession without a provider-native Session id; the provider settle then
/// persists it through `save_member_run`, which must sync the trust MemberRun
/// (selector layer) and the canonical AgentSession (exact-binding layer) so the
/// exact-self RoleView projection resolves. A Team sibling that never settled
/// keeps the honest unavailable shape instead of fabricating a projection.
#[test]
fn exact_self_session_projection_follows_fresh_start_settle_sync() {
    let home = TempHome::new("role-view-settle-sync");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let space_id = current_space_id(&home);
    let settled_native_session_id = "thread_fake_codex_app_server";
    let rollout_dir = home.home().join(".codex/sessions/2026/08/13");
    std::fs::create_dir_all(&rollout_dir).expect("Codex rollout fixture root");
    std::fs::write(
        rollout_dir.join(format!(
            "rollout-2026-08-13T00-00-00-{settled_native_session_id}.jsonl"
        )),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{settled_native_session_id}\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_reasoning\",\"turn_id\":\"turn-settle-1\",\"text\":\"raw-chain-of-thought-must-not-appear\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"turn_id\":\"turn-settle-1\",\"message\":\"display-safe settled result\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn-settle-1\"}}}}\n"
        ),
    )
    .expect("Codex rollout fixture");
    let run = |args: &[&str]| {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend_from_slice(args);
        let output = run_firm(&home, &root, &full);
        assert!(output.status.success(), "fixture {args:?}: {output:?}");
        output
    };
    let node: serde_json::Value =
        serde_json::from_slice(&run(&["node", "init"]).stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--project-binding-id",
        &project_id,
    ]);
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    let mission_id = "mission-settle-sync".to_string();
    firm_env::seed_historical_mission(
        &home,
        &project_id,
        &mission_id,
        "Fresh-start Session binding",
    );
    let host_id = "agent-settle-sync-host";
    let host = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        host_id,
        "Settle Sync Host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host: {host:?}");
    let worker_id = "agent-settle-sync-worker";
    let worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        worker_id,
        "Settle Sync Worker",
        "builder",
        "codex",
        &[],
    );
    assert!(worker.status.success(), "worker: {worker:?}");
    let sibling_worker_id = "agent-settle-sync-sibling";
    let sibling_worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        sibling_worker_id,
        "Settle Sync Sibling",
        "builder",
        "codex",
        &[],
    );
    assert!(
        sibling_worker.status.success(),
        "sibling worker: {sibling_worker:?}"
    );
    run(&[
        "team",
        "create",
        "--name",
        "Settle sync team",
        "--description",
        "Fresh-start settle sync integration",
        "--mission-id",
        &mission_id,
        "--host-agent-id",
        host_id,
        "--node-id",
        node_id,
        "--member",
        host_id,
        "--member",
        worker_id,
        "--member",
        sibling_worker_id,
    ]);
    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    let team = store
        .latest_teams()
        .expect("teams")
        .into_values()
        .next()
        .expect("settle sync team");
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind":"agent_member","id":team.host_agent_id},
        "authority_actors": []
    },{
        "token": MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":worker_id},
        "authority_actors": []
    },{
        "token": SIBLING_MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":sibling_worker_id},
        "authority_actors": []
    }])
    .to_string();
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("settle-codex-bin"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &["--space", &space_id],
        &[
            ("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str()),
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        ],
    );
    // Fresh start: no resume_native_session_id, so the MemberRun and the
    // materialized AgentSession both start with no native Session binding.
    let (status, created_run) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id": team.id,
            "objective": "Fresh-start settle writes the trust Session binding",
            "members": [
                {"agent_member_id":worker_id,"name":"worker","role":"builder","provider":"codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_run_id = created_run["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member run id")
        .to_string();
    let live_work = run_firm(
        &home,
        &root,
        &[
            "--space",
            &space_id,
            "--project",
            &project_id,
            "team-run",
            "work",
            "create",
            "--team-run-id",
            &run_id,
            "--work-id",
            "work-live-provider-sse",
            "--title",
            "Exercise provider live SSE",
            "--completion-criteria",
            "Provider emits display-safe live activity and terminal clear",
            "--owner-member-run-id",
            &member_run_id,
        ],
    );
    assert!(live_work.status.success(), "live Work: {live_work:?}");
    let mut member_sse = serve.open_sse_with_token(
        &format!("?space={space_id}&project={project_id}"),
        Some(MEMBER_TOKEN),
    );
    member_sse
        .get_mut()
        .set_read_timeout(Some(std::time::Duration::from_millis(250)))
        .expect("private SSE timeout");
    // Serve and NodeDaemon are independent children. Wait for the volatile
    // loopback endpoint handshake before the provider turn starts.
    let mut sink_registered = false;
    for _ in 0..150 {
        let status = run_firm(&home, &root, &["daemon", "status"]);
        if status.status.success()
            && serde_json::from_slice::<serde_json::Value>(&status.stdout)
                .is_ok_and(|value| value["live_provider_activity_sink_registered"] == true)
        {
            sink_registered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        sink_registered,
        "serve never registered its volatile live provider activity sink"
    );
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start: {started}");

    let live_frames = collect_named_sse_data(
        &mut member_sse,
        std::time::Duration::from_secs(10),
        "live_provider_activity",
    );
    let private_live = live_frames
        .iter()
        .filter(|frame| frame["schema_version"] == "agentfirm.live_provider_activity_event.v1")
        .collect::<Vec<_>>();
    assert!(private_live.iter().any(|frame| {
        frame["reason"] == "updated"
            && frame["scope"]["member_run_id"] == member_run_id
            && frame["activity"]["items"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(item["kind"].as_str(), Some("tool_started" | "response_streaming"))
                })
            })
    }), "independent NodeDaemon provider activity never reached authenticated serve SSE: {private_live:?}");
    assert!(
        private_live.iter().any(|frame| {
            frame["reason"] == "terminal"
                && frame["scope"]["member_run_id"] == member_run_id
                && frame["activity"].is_null()
        }),
        "provider terminal did not clear the volatile overlay: {private_live:?}"
    );
    let serialized_live = serde_json::to_string(&private_live).expect("private SSE JSON");
    assert!(!serialized_live.contains("cargo check"));
    assert!(!serialized_live.contains("hidden"));

    let mut settled = false;
    for _ in 0..500 {
        settled = store
            .member_runs()
            .expect("ledger MemberRuns")
            .into_iter()
            .rev()
            .any(|member| {
                member.id == member_run_id
                    && member.native_session.as_ref().is_some_and(|session| {
                        session.native_session_id == settled_native_session_id
                    })
            });
        if settled {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        settled,
        "provider settle never wrote the native Session into the ledger projection"
    );

    // The settle must sync both trust layers, not just the ledger projection.
    // The write-back runs immediately after the ledger append inside
    // save_member_run; poll the trust rows so the assertions never race it.
    let mut trust_run = None;
    for _ in 0..500 {
        trust_run = store
            .trust_member_runs(&space_id)
            .expect("trust MemberRuns")
            .into_iter()
            .find(|run| run.id == member_run_id);
        if trust_run
            .as_ref()
            .and_then(|run| run.native_session.as_ref())
            .is_some_and(|session| session.native_session_id == settled_native_session_id)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let trust_run = trust_run.expect("canonical trust MemberRun");
    assert_eq!(
        trust_run
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some(settled_native_session_id),
        "fresh-start settle must sync the native Session onto the trust MemberRun: {trust_run:?}"
    );
    let mut sessions = Vec::new();
    for _ in 0..500 {
        sessions = store
            .fabric_agent_sessions(&space_id)
            .expect("canonical AgentSessions")
            .into_iter()
            .filter(|session| session.agent_member_id == worker_id)
            .collect::<Vec<_>>();
        if sessions.len() == 1
            && sessions[0]
                .native_session_ref
                .as_ref()
                .is_some_and(|session| session.native_session_id == settled_native_session_id)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(sessions.len(), 1, "one current AgentSession: {sessions:?}");
    assert_eq!(
        sessions[0]
            .native_session_ref
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some(settled_native_session_id),
        "fresh-start settle must sync the native Session onto the canonical AgentSession: {:?}",
        sessions[0]
    );

    // Layer 1 + layer 2 now resolve: the exact-self owner reads the real
    // provider-native observation instead of an unavailable placeholder.
    let member_agent_workspace_route =
        format!("/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={worker_id}");
    let (status, member_self_workspace) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "exact-self Member AgentWorkspace: {member_self_workspace}"
    );
    let owner_projection = &member_self_workspace["data"]["session_event_projection"];
    assert_eq!(owner_projection["disabled_reason"], serde_json::Value::Null);
    assert!(owner_projection["agent_session_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("agent-session:")));
    assert!(owner_projection["source_snapshot_fingerprint"]
        .as_str()
        .is_some_and(|fingerprint| fingerprint.starts_with("sha256:")));
    assert_eq!(
        owner_projection["episodes"][0]["provider_turn_id"],
        "turn-settle-1"
    );
    assert_eq!(owner_projection["episodes"][0]["terminal"], true);
    let serialized_owner_projection =
        serde_json::to_string(owner_projection).expect("projection JSON");
    assert!(serialized_owner_projection.contains("display-safe settled result"));
    assert!(!serialized_owner_projection.contains("raw-chain-of-thought-must-not-appear"));

    // The Host-selected surface stays public: the private projection is
    // structurally absent even though the binding now exists.
    let (status, host_selected_member) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", TOKEN)],
    );
    assert_eq!(
        status, 200,
        "Host-selected Member AgentWorkspace: {host_selected_member}"
    );
    assert_eq!(
        host_selected_member["data"]["projection_scope"],
        "host_member_public"
    );
    assert!(
        host_selected_member["data"]
            .get("session_event_projection")
            .is_none(),
        "Host-selected Member must structurally omit the private Session projection"
    );

    // The same active MemberRun is readable from the loopback Dashboard
    // without borrowing either the Host or Member mutation authority.
    let (status, local_operator_member) = serve.get_json(&member_agent_workspace_route);
    assert_eq!(
        status, 200,
        "loopback Operator AgentWorkspace: {local_operator_member}"
    );
    assert_eq!(
        local_operator_member["data"]["projection_scope"],
        "host_member_public"
    );
    assert_eq!(
        local_operator_member["allowed_actions"],
        serde_json::json!([]),
        "loopback Operator must not borrow AgentMember actions"
    );

    // A Team sibling that never settled a native Session keeps the honest
    // unavailable shape: no fabricated session id, fingerprint, or episodes.
    let sibling_self_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={sibling_worker_id}"
    );
    let (status, sibling_self_unavailable) = serve.get_json_with_headers(
        &sibling_self_route,
        &[("X-AgentFirm-Token", SIBLING_MEMBER_TOKEN)],
    );
    assert_eq!(status, 200, "unavailable exact-self projection");
    let unavailable = &sibling_self_unavailable["data"]["session_event_projection"];
    for field in [
        "agent_session_id",
        "agent_session_generation",
        "source_snapshot_fingerprint",
    ] {
        assert!(
            unavailable[field].is_null(),
            "unavailable provider projection must not fabricate {field}"
        );
    }
    assert_eq!(unavailable["episodes"], serde_json::json!([]));
    assert_eq!(unavailable["truncated"], false);
    assert!(unavailable["disabled_reason"].as_str().is_some());

    // Stop is an execution fence, not read authority. Close the exact Member
    // through its Supervisor, then stop the NodeDaemon cleanly before proving
    // that owner history still reconstructs from the provider-native source.
    let closed = run_firm(
        &home,
        &root,
        &[
            "--space",
            &space_id,
            "--project",
            &project_id,
            "team-run",
            "close-member",
            "--id",
            &run_id,
            "--member-run-id",
            &member_run_id,
            "--reason",
            "historical projection after clean Stop",
        ],
    );
    assert!(closed.status.success(), "close Member: {closed:?}");
    let mut member_stopped = false;
    for _ in 0..500 {
        member_stopped = store
            .member_runs()
            .expect("MemberRuns after Close")
            .into_iter()
            .rev()
            .find(|member| member.id == member_run_id)
            .is_some_and(|member| {
                member.status == harness_core::MemberRunStatus::Stopped
                    && member.coordination_status == harness_core::MemberCoordinationStatus::Closed
            });
        if member_stopped {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        member_stopped,
        "Member did not reach a clean stopped boundary"
    );
    let daemon_stopped = run_firm(&home, &root, &["daemon", "stop"]);
    assert!(
        daemon_stopped.status.success(),
        "clean NodeDaemon stop: {daemon_stopped:?}"
    );
    let (status, stopped_owner_workspace) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "stopped owner workspace: {stopped_owner_workspace}"
    );
    assert_eq!(
        stopped_owner_workspace["data"]["session_event_projection"]["availability"],
        "available"
    );
    assert!(
        stopped_owner_workspace["data"]["session_event_projection"]["episodes"]
            .as_array()
            .is_some_and(|episodes| !episodes.is_empty())
    );
}
