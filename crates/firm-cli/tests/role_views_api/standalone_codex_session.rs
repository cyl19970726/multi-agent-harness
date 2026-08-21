use super::*;

#[test]
fn standalone_codex_session_runs_through_node_daemon_and_replays_without_team_membership() {
    let home = TempHome::new("standalone-codex-node-session");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    assert!(run_firm(&home, &root, &["init"]).status.success());
    let project_id = current_project_id(&home);
    let space_id = current_space_id(&home);
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
    let identity_id = "agent-standalone-codex";
    let created = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        identity_id,
        "Standalone Codex",
        "builder",
        "codex",
        &[],
    );
    assert!(
        created.status.success(),
        "AgentIdentity fixture: {created:?}"
    );
    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    assert!(
        store
            .fabric_team_memberships(&space_id)
            .expect("memberships")
            .iter()
            .all(|membership| membership.agent_member_id != identity_id),
        "standalone StartSession precondition must contain no TeamMembership"
    );

    let real_codex =
        std::env::var("AGENTFIRM_REAL_CODEX_NODE_SESSION").is_ok_and(|value| value == "1");
    let fake_bin = (!real_codex)
        .then(|| fake_provider::install_codex_team_shim(&home.base().join("node-codex-bin")));
    let thread_marker = home.base().join("node-codex-thread.jsonl");
    let path = fake_bin.as_ref().map_or_else(
        || std::env::var("PATH").unwrap_or_default(),
        |fake_bin| {
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").unwrap_or_default()
            )
        },
    );
    let credentials = serde_json::json!([{
        "token": MEMBER_TOKEN,
        "actor": {"kind":"agent_member","id":identity_id},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &["--space", &space_id],
        &[
            ("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str()),
            ("PATH", path.as_str()),
            (
                "FAKE_CODEX_THREAD_MARKER",
                thread_marker.to_str().expect("marker path"),
            ),
        ],
    );
    let headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "standalone-codex-session-start"),
        ("If-Match", "0"),
    ];
    let intent = serde_json::json!({
        "command":"start_session",
        "expires_unix_ms":unix_ms()+30_000,
        "payload":{"agent_member_id":identity_id}
    });
    let before_commands = store.runtime_commands(&space_id).expect("commands before");
    let (status, started) =
        serve.post_json_with_headers("/v1/agentfirm/runtime-commands", &intent, &headers);
    assert_eq!(status, 200, "standalone StartSession: {started}");
    assert_eq!(started["ok"], true, "NodeDaemon response: {started}");
    assert_eq!(started["result"]["session"]["agent_member_id"], identity_id);
    assert_eq!(
        started["result"]["native_session"]["execution_mode"],
        "node_daemon_app_server"
    );
    assert_eq!(
        started["result"]["permission"]["effective"],
        "workspace_write"
    );
    if !real_codex {
        assert!(
            std::fs::read_to_string(&thread_marker)
                .expect("adapter thread/start marker")
                .contains("thread/start"),
            "NodeDaemon must open the provider-native Codex session"
        );
    }
    let commands_after_start = store.runtime_commands(&space_id).expect("commands after");
    assert_eq!(commands_after_start.len(), before_commands.len() + 1);
    let sessions_after_start = store
        .fabric_agent_sessions(&space_id)
        .expect("sessions after");
    assert_eq!(sessions_after_start.len(), 1);
    let native_session = sessions_after_start[0]
        .native_session_ref
        .as_ref()
        .expect("native ref");
    assert!(!native_session.native_session_id.is_empty());
    if !real_codex {
        assert_eq!(
            native_session.native_session_id,
            "thread_fake_codex_app_server"
        );
    }

    let marker_before_replay =
        (!real_codex).then(|| std::fs::read(&thread_marker).expect("marker before replay"));
    let durable_before_replay = (
        store
            .canonical_operations()
            .expect("operations before replay"),
        store
            .runtime_commands(&space_id)
            .expect("commands before replay"),
        store
            .fabric_agent_sessions(&space_id)
            .expect("sessions before replay"),
    );
    let (status, replayed) =
        serve.post_json_with_headers("/v1/agentfirm/runtime-commands", &intent, &headers);
    assert_eq!(status, 200, "StartSession replay: {replayed}");
    assert_eq!(replayed["replayed"], true, "replay response: {replayed}");
    if let Some(marker_before_replay) = marker_before_replay {
        assert_eq!(
            std::fs::read(&thread_marker).expect("marker after replay"),
            marker_before_replay,
            "replay must not open a second provider-native thread"
        );
    }
    assert_eq!(
        (
            store
                .canonical_operations()
                .expect("operations after replay"),
            store
                .runtime_commands(&space_id)
                .expect("commands after replay"),
            store
                .fabric_agent_sessions(&space_id)
                .expect("sessions after replay"),
        ),
        durable_before_replay,
        "replay must not append a second Session or RuntimeCommand"
    );

    let resume_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "standalone-codex-session-resume"),
        ("If-Match", "0"),
    ];
    let resume_intent = serde_json::json!({
        "command":"resume_session",
        "expires_unix_ms":unix_ms()+30_000,
        "payload":{
            "session_id":sessions_after_start[0].id,
            "session_generation":sessions_after_start[0].runtime_generation
        }
    });
    let marker_before_resume =
        (!real_codex).then(|| std::fs::read(&thread_marker).expect("marker before resume"));
    let (status, resumed) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &resume_intent,
        &resume_headers,
    );
    assert_eq!(status, 200, "standalone ResumeSession: {resumed}");
    assert_eq!(
        resumed["result"]["lifecycle"], "cold",
        "resuming a live provider thread must not fabricate an active turn"
    );
    if let Some(marker_before_resume) = marker_before_resume {
        assert_eq!(
            std::fs::read(&thread_marker).expect("marker after resume"),
            marker_before_resume,
            "ResumeSession must reuse the exact live NodeDaemon provider handle"
        );
    }
    let commands_after_resume = store
        .runtime_commands(&space_id)
        .expect("commands after resume");
    let (status, resume_replay) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &resume_intent,
        &resume_headers,
    );
    assert_eq!(status, 200, "ResumeSession replay: {resume_replay}");
    assert_eq!(resume_replay["replayed"], true);
    assert_eq!(
        store
            .runtime_commands(&space_id)
            .expect("commands after resume replay"),
        commands_after_resume,
        "resume replay never re-enters the provider adapter"
    );

    let stop_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "standalone-codex-session-stop"),
        ("If-Match", "0"),
    ];
    let stop_intent = serde_json::json!({
        "command":"stop_session",
        "expires_unix_ms":unix_ms()+30_000,
        "payload":{
            "session_id":sessions_after_start[0].id,
            "session_generation":sessions_after_start[0].runtime_generation
        }
    });
    let (status, stopped) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &stop_intent,
        &stop_headers,
    );
    assert_eq!(status, 200, "standalone StopSession: {stopped}");
    assert_eq!(stopped["result"]["lifecycle"], "closed");
    assert_eq!(
        store
            .fabric_agent_sessions(&space_id)
            .expect("closed session")[0]
            .lifecycle,
        harness_core::agentfirm_api::AgentSessionStatus::Closed
    );
    let commands_after_stop = store
        .runtime_commands(&space_id)
        .expect("commands after stop");
    let operations_after_stop = store.canonical_operations().expect("operations after stop");
    let (status, stop_replay) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &stop_intent,
        &stop_headers,
    );
    assert_eq!(status, 200, "StopSession replay: {stop_replay}");
    assert_eq!(stop_replay["replayed"], true);
    assert_eq!(
        store
            .runtime_commands(&space_id)
            .expect("commands after stop replay"),
        commands_after_stop
    );
    assert_eq!(
        store
            .canonical_operations()
            .expect("operations after stop replay"),
        operations_after_stop
    );

    let mut hostile_reuse = stop_intent.clone();
    hostile_reuse["expires_unix_ms"] = serde_json::json!(unix_ms() + 40_000);
    let (status, hostile) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &hostile_reuse,
        &stop_headers,
    );
    assert_eq!(
        status, 409,
        "changed replay semantics must conflict: {hostile}"
    );
    assert!(
        hostile.to_string().contains("IDEMPOTENCY_KEY_REUSED"),
        "hostile replay must name the immutable-key conflict: {hostile}"
    );
    assert_eq!(
        store
            .runtime_commands(&space_id)
            .expect("commands after hostile replay"),
        commands_after_stop,
        "hostile replay has zero RuntimeCommand side effects"
    );
    assert_eq!(
        store
            .canonical_operations()
            .expect("operations after hostile replay"),
        operations_after_stop,
        "hostile replay has zero canonical operation side effects"
    );
}
