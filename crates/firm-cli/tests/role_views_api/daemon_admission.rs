use super::*;

#[test]
fn operator_eligible_daemon_and_server_probed_admission_are_real_and_fail_closed() {
    let home = TempHome::new("role-operator-actions");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
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
    let node_id = node["id"].as_str().expect("node id");
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--execution-space-id",
        &space_id,
        "--project-binding-id",
        &project_id,
    ]);

    let fake_bin = home.base().join("operator-probe-bin");
    std::fs::create_dir_all(&fake_bin).expect("fake bin");
    let shim = fake_bin.join("codex");
    std::fs::write(&shim, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 9.9.9'; exit 0; fi\nexit 2\n").expect("probe shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).unwrap();
    }
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let credentials = serde_json::json!([{"token":OPERATOR_TOKEN,"actor":{"kind":"service","id":node_id},"authority_actors":[]}]).to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &["--space", &space_id],
        &[
            ("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str()),
            ("PATH", path.as_str()),
        ],
    );
    let operator_route = format!("/v1/views/operator/{node_id}?project={project_id}");
    let (status, operator) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator view: {operator}");
    let actions = operator["allowed_actions"].as_array().expect("actions");
    assert!(
        actions
            .iter()
            .any(|action| action["kind"] == "stop_daemon" && action["disabled_reason"].is_null()),
        "eligible stop action: {operator}"
    );
    let eligible_admission_action = actions
        .iter()
        .find(|action| {
            action["kind"] == "admit_provider"
                && action["intent_binding"]["provider"] == "codex"
                && action["intent_binding"]["execution_mode"] == "codex_app_server"
        })
        .expect("registered Codex admission action");
    assert!(eligible_admission_action["disabled_reason"].is_null());
    assert_eq!(
        eligible_admission_action["intent_binding"]["eligibility"],
        "eligible"
    );
    let admission_fingerprint = eligible_admission_action["intent_binding"]
        ["eligibility_fingerprint"]
        .as_str()
        .expect("server-built admission fingerprint")
        .to_string();
    assert!(
        actions.iter().any(|action| {
            action["kind"] == "admit_provider"
                && action["intent_binding"]["provider"] == "claude"
                && action["intent_binding"]["execution_mode"] == "claude_agent_sdk"
                && action["intent_binding"]["eligibility"] == "disabled"
                && action["disabled_reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
        }),
        "missing registered provider binary must remain an explicit disabled tuple: {operator}"
    );
    let node_revision = operator["data"]["node"]["node_revision"]
        .as_u64()
        .expect("node revision")
        .to_string();
    let initial_daemon_generation = operator["data"]["node"]["daemon_generation"]
        .as_u64()
        .expect("live daemon generation");
    assert!(actions.iter().any(|action| {
        action["kind"] == "stop_daemon"
            && action["authority_generation"] == initial_daemon_generation
    }));
    let initial_stop_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-initial-stop"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-stop"),
    ];
    let stop_route = format!("/v1/agentfirm/nodes/{node_id}/daemon-stop?project={project_id}");
    let (status, initial_stopped) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop","daemon_generation":initial_daemon_generation}),
        &initial_stop_headers,
    );
    assert_eq!(status, 200, "initial daemon stop: {initial_stopped}");
    let store = HarnessStore::new(home.spaces_dir().join(&space_id))
        .with_provider_compatibility_scope(&project_id, format!("execution-space:{space_id}"));
    let release_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if store
            .latest_node_daemon_lease(node_id)
            .expect("stopped daemon lease")
            .is_some_and(|lease| lease.status == harness_core::NodeDaemonLeaseStatus::Released)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < release_deadline,
            "NodeDaemon did not explicitly release after Stop"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let dead_instance_id = format!("2147483647:{}:dead-daemon", unix_ms());
    let dead_lease = store
        .acquire_node_daemon_lease(node_id, "dead-daemon", &dead_instance_id, unix_ms(), 1)
        .expect("expired predecessor fixture lease");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let (status, recovery_view) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator recovery view: {recovery_view}");
    let recovery_action = recovery_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "recover_daemon_predecessor")
        })
        .expect("expired predecessor recovery action");
    assert_eq!(
        recovery_action["recovery_binding"],
        serde_json::json!({
            "daemon_id":"dead-daemon",
            "instance_id":dead_instance_id,
            "daemon_generation":dead_lease.generation,
        })
    );
    assert!(recovery_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| !actions
            .iter()
            .any(|action| action["kind"] == "start_daemon")));
    let recovery_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-predecessor-recover"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-recover-predecessor"),
    ];
    let recovery_route =
        format!("/v1/agentfirm/nodes/{node_id}/daemon-recover-predecessor?project={project_id}");
    let (status, recovered) = serve.post_json_with_headers(
        &recovery_route,
        &serde_json::json!({
            "action":"recover_daemon_predecessor",
            "daemon_id":"dead-daemon",
            "instance_id":dead_instance_id,
            "daemon_generation":dead_lease.generation,
            "provider_process_groups_terminated_confirmed":true,
            "evidence_ref":"test:dead-process-and-provider-groups-absent",
        }),
        &recovery_headers,
    );
    assert_eq!(status, 200, "predecessor recovery: {recovered}");
    assert_eq!(
        store
            .latest_node_daemon_lease(node_id)
            .expect("recovered lease")
            .expect("lease row")
            .status,
        harness_core::NodeDaemonLeaseStatus::Released
    );
    let (status, after_stop) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator after stop: {after_stop}");
    let stopped_generation = after_stop["data"]["node"]["daemon_generation"]
        .as_u64()
        .expect("released daemon generation");
    assert!(after_stop["allowed_actions"]
        .as_array()
        .is_some_and(|actions| {
            actions.iter().any(|action| {
                action["kind"] == "start_daemon"
                    && action["authority_generation"] == stopped_generation
            })
        }));
    assert!(after_stop["allowed_actions"]
        .as_array()
        .is_some_and(|actions| {
            actions.iter().any(|action| {
                action["kind"] == "start_daemon" && action["disabled_reason"].is_null()
            })
        }));
    let start_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-start"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-start"),
    ];
    let start_route = format!("/v1/agentfirm/nodes/{node_id}/daemon-start?project={project_id}");
    let (status, started) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"daemon_start","max_concurrency":1,"daemon_generation":stopped_generation}),
        &start_headers,
    );
    assert_eq!(status, 200, "daemon start: {started}");
    let (status, start_replay) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"daemon_start","max_concurrency":1,"daemon_generation":stopped_generation}),
        &start_headers,
    );
    assert_eq!(status, 200, "daemon start replay: {start_replay}");
    assert_eq!(start_replay["replayed"], true);
    assert_eq!(start_replay["event_id"], started["event_id"]);
    let (status, changed_start) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"daemon_start","max_concurrency":2,"daemon_generation":stopped_generation}),
        &start_headers,
    );
    assert_eq!(
        status, 409,
        "changed daemon start replay must fail closed: {changed_start}"
    );
    let (status, after_start) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator after start: {after_start}");
    let successor_generation = after_start["data"]["node"]["daemon_generation"]
        .as_u64()
        .expect("successor daemon generation");
    assert!(successor_generation > stopped_generation);
    let stale_stop_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-stale-stop"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-stop"),
    ];
    let (status, stale_stopped) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop","daemon_generation":stopped_generation}),
        &stale_stop_headers,
    );
    assert_eq!(status, 409, "stale generation stop: {stale_stopped}");
    let (status, after_stale_stop) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator after stale stop: {after_stale_stop}");
    assert_eq!(
        after_stale_stop["data"]["node"]["daemon_generation"], successor_generation,
        "stale stop cannot replace or terminate the successor generation"
    );
    assert!(after_stale_stop["allowed_actions"]
        .as_array()
        .is_some_and(|actions| {
            actions.iter().any(|action| {
                action["kind"] == "stop_daemon" && action["disabled_reason"].is_null()
            })
        }));

    let before = store
        .latest_provider_compatibility_admissions()
        .expect("admissions before")
        .len();
    let admission_headers = action_headers(
        OPERATOR_TOKEN,
        "operator-provider-admit",
        node_revision.as_str(),
    );
    let admission_route =
        format!("/v1/agentfirm/nodes/{node_id}/provider-admission?project={project_id}");
    let intent = serde_json::json!({"action":"admit_provider","provider":"codex","execution_mode":"codex_app_server","eligibility_fingerprint":admission_fingerprint});
    let (status, admitted) =
        serve.post_json_with_headers(&admission_route, &intent, &admission_headers);
    assert_eq!(status, 200, "provider admission: {admitted}");
    assert_eq!(admitted["projection"]["provider_version"], "9.9.9");
    assert!(admitted["projection"]["evidence_refs"]
        .as_array()
        .is_some_and(|refs| refs.iter().all(|item| item
            .as_str()
            .is_some_and(|value| value.starts_with("server-")))));
    let (status, replay) =
        serve.post_json_with_headers(&admission_route, &intent, &admission_headers);
    assert_eq!(status, 200, "provider admission replay: {replay}");
    assert_eq!(replay["replayed"], true);

    // A Project Binding is selected independently from its shared Execution
    // Space. The server must rebuild admission eligibility for B and must not
    // replay A's completed external-effect receipt under the same key.
    let project_b_root = home.base().join("project-b");
    std::fs::create_dir_all(&project_b_root).expect("project B root");
    let project_b = run_firm(
        &home,
        &project_b_root,
        &[
            "project",
            "add",
            project_b_root.to_str().expect("project B UTF-8 path"),
        ],
    );
    assert!(project_b.status.success(), "project B add: {project_b:?}");
    let project_b_json: serde_json::Value =
        serde_json::from_slice(&project_b.stdout).expect("project B JSON");
    let project_b_id = project_b_json["id"]
        .as_str()
        .expect("project B id")
        .to_string();
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--execution-space-id",
        &space_id,
        "--project-binding-id",
        &project_b_id,
    ]);
    let operator_b_route = format!("/v1/views/operator/{node_id}?project={project_b_id}");
    let (status, operator_b) =
        serve.get_json_with_headers(&operator_b_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Project B Operator view: {operator_b}");
    let binding_b = operator_b["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action["kind"] == "admit_provider"
                    && action["intent_binding"]["provider"] == "codex"
                    && action["intent_binding"]["execution_mode"] == "codex_app_server"
            })
        })
        .and_then(|action| action["intent_binding"].as_object())
        .expect("Project B server-built admission binding");
    assert_eq!(binding_b["project_binding_id"], project_b_id);
    assert!(binding_b["registration_identity"]
        .as_str()
        .is_some_and(|identity| identity.ends_with(&project_b_id)));
    assert_eq!(binding_b["registration_revision"], 1);
    let b_intent = serde_json::json!({
        "action":"admit_provider",
        "provider":"codex",
        "execution_mode":"codex_app_server",
        "eligibility_fingerprint":binding_b["eligibility_fingerprint"],
    });
    let admission_b_route =
        format!("/v1/agentfirm/nodes/{node_id}/provider-admission?project={project_b_id}");
    let store_before_project_switch = ledger_digest(serve.fixture_store_root());
    let receipts_root = home
        .firm_home()
        .join("runtime")
        .join("operator-action-receipts");
    let receipts_before_project_switch = file_tree_digest(&receipts_root);
    let daemon_generation_before_project_switch =
        operator_b["data"]["node"]["daemon_generation"].clone();
    let admissions_before_project_switch = store
        .latest_provider_compatibility_admissions()
        .expect("admissions before Project switch replay")
        .len();
    let (status, project_switch_rejected) =
        serve.post_json_with_headers(&admission_b_route, &b_intent, &admission_headers);
    assert_eq!(
        status, 409,
        "same key cannot cross Project Binding scope: {project_switch_rejected}"
    );
    assert_eq!(
        project_switch_rejected["error"]["code"],
        "IDEMPOTENCY_CONFLICT"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        store_before_project_switch,
        "cross-Project replay has zero Store side effects"
    );
    assert_eq!(
        file_tree_digest(&receipts_root),
        receipts_before_project_switch,
        "cross-Project replay cannot rewrite the completed receipt"
    );
    assert_eq!(
        store
            .latest_provider_compatibility_admissions()
            .expect("admissions after Project switch replay")
            .len(),
        admissions_before_project_switch,
        "cross-Project replay cannot perform provider admission"
    );
    let (status, operator_b_after) =
        serve.get_json_with_headers(&operator_b_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Project B after rejection: {operator_b_after}");
    assert_eq!(
        operator_b_after["data"]["node"]["daemon_generation"],
        daemon_generation_before_project_switch,
        "cross-Project replay cannot affect the NodeDaemon"
    );
    let hostile = serde_json::json!({"action":"admit_provider","provider":"codex","execution_mode":"codex_app_server","eligibility_fingerprint":admission_fingerprint,"provider_version":"browser-spoof","evidence_refs":["browser-proof"]});
    let hostile_headers = action_headers(
        OPERATOR_TOKEN,
        "operator-provider-hostile",
        node_revision.as_str(),
    );
    let (status, rejected) =
        serve.post_json_with_headers(&admission_route, &hostile, &hostile_headers);
    assert_eq!(status, 409, "hostile admission facts: {rejected}");
    assert_eq!(
        store
            .latest_provider_compatibility_admissions()
            .expect("admissions after")
            .len(),
        before + 1,
        "hostile browser facts have zero durable side effects"
    );

    std::fs::write(
        &shim,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 0.148.0-alpha.9'; exit 0; fi\nexit 2\n",
    )
    .expect("replace probe shim with an adapter-current version");
    let (status, current_provider_view) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(
        status, 200,
        "current provider Operator view: {current_provider_view}"
    );
    let current_admission_action = current_provider_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action["kind"] == "admit_provider"
                    && action["intent_binding"]["provider"] == "codex"
                    && action["intent_binding"]["execution_mode"] == "codex_app_server"
            })
        })
        .expect("admission action remains visible with an explicit disabled reason");
    assert!(current_admission_action["disabled_reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("observed provider tuple is current")));
    let current_headers = action_headers(
        OPERATOR_TOKEN,
        "operator-provider-current-version",
        node_revision.as_str(),
    );
    let (status, current_rejected) =
        serve.post_json_with_headers(&admission_route, &intent, &current_headers);
    assert_eq!(
        status, 409,
        "current tuple cannot be admitted: {current_rejected}"
    );
    assert_eq!(
        store
            .latest_provider_compatibility_admissions()
            .expect("admissions after current tuple rejection")
            .len(),
        before + 1,
        "ineligible current tuple has zero durable side effects"
    );

    let stop_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-stop"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-stop"),
    ];
    let (status, stopped) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop","daemon_generation":successor_generation}),
        &stop_headers,
    );
    assert_eq!(status, 200, "daemon stop: {stopped}");
    let (status, stop_replay) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop","daemon_generation":successor_generation}),
        &stop_headers,
    );
    assert_eq!(status, 200, "daemon stop replay: {stop_replay}");
    assert_eq!(stop_replay["replayed"], true);
    assert_eq!(stop_replay["event_id"], stopped["event_id"]);

    run(&["node", "drain", "--id", node_id]);
    let (status, replay_after_revision_advance) =
        serve.post_json_with_headers(&admission_route, &intent, &admission_headers);
    assert_eq!(
        status, 200,
        "provider replay must precede advanced ExecutionNode revision checks: {replay_after_revision_advance}"
    );
    assert_eq!(replay_after_revision_advance["replayed"], true);
    let conflicting_intent = serde_json::json!({"action":"admit_provider","provider":"claude","execution_mode":"claude_agent_sdk","eligibility_fingerprint":admission_fingerprint});
    let (status, conflicting_replay) =
        serve.post_json_with_headers(&admission_route, &conflicting_intent, &admission_headers);
    assert_eq!(
        status, 409,
        "provider replay fingerprint conflict: {conflicting_replay}"
    );
}
