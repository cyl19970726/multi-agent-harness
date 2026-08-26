use super::*;

#[test]
fn canonical_team_message_journey_uses_node_daemon_sessions_deliveries_and_cursor() {
    let home = TempHome::new("canonical-role-message-journey");
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
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    let mission_id = "mission-message-journey".to_string();
    firm_env::seed_historical_mission(&home, &project_id, &mission_id, "Canonical message journey");
    let host_id = "agent-message-host";
    let member_id = "agent-message-member";
    for (id, name, role) in [
        (host_id, "Message Host", "host"),
        (member_id, "Message Member", "builder"),
    ] {
        let created =
            create_canonical_agent_member(&home, &root, &project_id, id, name, role, "codex", &[]);
        assert!(created.status.success(), "AgentMember {id}: {created:?}");
    }
    run(&[
        "team",
        "create",
        "--name",
        "Canonical Message Team",
        "--description",
        "Wave4C real message journey",
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
    },{
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
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
            "objective":"Canonical Team Message journey",
            "host_surface":"codex-app",
            "host_thread_id":"canonical-message-host-thread",
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
        .expect("TeamRun id");
    let member_run_id = created_run["result"]["member_runs"][1]["id"]
        .as_str()
        .expect("MemberRun id");

    // The helper uses the real authenticated HTTP RuntimeCommand and daemon
    // socket. It only replaces the retired test route inside ServeHandle.
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
    let lease = store
        .latest_node_daemon_lease(&node_id)
        .expect("daemon lease")
        .expect("current daemon lease");
    let sessions = store
        .fabric_agent_sessions(&space_id)
        .expect("AgentSessions");
    assert!(
        sessions
            .iter()
            .all(|session| session.agent_member_id != host_id),
        "external Host must not materialize an AgentSession"
    );
    let member_session = sessions
        .iter()
        .find(|session| {
            session.agent_member_id == member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .expect("Member AgentSession");
    assert_eq!(member_session.node_daemon_id, lease.daemon_id);
    assert_eq!(member_session.node_daemon_generation, lease.generation);
    let space_store_root = home.spaces_dir().join(&space_id);
    let before_hostile_runtime = ledger_digest(&space_store_root);
    let hostile_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "hostile-sibling-runtime-control"),
        ("If-Match", "0"),
    ];
    let (status, hostile_runtime) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &serde_json::json!({
            "command":"stop_session",
            "expires_unix_ms":unix_ms()+30_000,
            "payload":{
                "session_id":member_session.id,
                "session_generation":member_session.runtime_generation
            }
        }),
        &hostile_headers,
    );
    assert_eq!(status, 403, "sibling runtime control: {hostile_runtime}");
    assert_eq!(hostile_runtime["error"]["code"], "UNAUTHORIZED_ACTOR");
    assert_eq!(
        ledger_digest(&space_store_root),
        before_hostile_runtime,
        "hostile runtime control must have byte-zero durable side effects"
    );

    let spoof_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "hostile-runtime-capability-spoof"),
        ("If-Match", "0"),
    ];
    let (status, capability_spoof) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &serde_json::json!({
            "target_node_id":node_id,
            "command":"start_session",
            "required_capability":"full_control",
            "expires_unix_ms":unix_ms()+30_000,
            "payload":{
                "agent_member_id":member_id,
                "session":{
                    "effective_permission_ceiling":"full_access"
                }
            }
        }),
        &spoof_headers,
    );
    assert_eq!(status, 400, "capability/session spoof: {capability_spoof}");
    assert_eq!(
        ledger_digest(&space_store_root),
        before_hostile_runtime,
        "caller-selected capability or AgentSession payload must have byte-zero side effects"
    );

    let recovery_payload = serde_json::json!({
        "session_id":member_session.id,
        "session_generation":member_session.runtime_generation,
        "operation":"stop_session",
        "delivery_id":"recovery-role-view-fixture"
    });
    let recovery_command = harness_core::agentfirm_api::ControlCommandEnvelope {
        id: "runtime-command-role-view-recovery".into(),
        execution_space_id: space_id.clone(),
        target_node_id: node_id.clone(),
        target_node_daemon_id: lease.daemon_id.clone(),
        target_node_daemon_generation: lease.generation,
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: lease.daemon_id.clone(),
        },
        command: harness_core::agentfirm_api::RuntimeCommandKind::StopSession,
        required_capability: "agent_session.stop".into(),
        idempotency_key: "runtime-command-role-view-recovery".into(),
        expected_version: 0,
        expires_unix_ms: unix_ms() + 30_000,
        binding: RuntimeCommandBinding {
            target_session_id: Some(member_session.id.clone()),
            target_runtime_generation: Some(member_session.runtime_generation),
            target_driver_generation: Some(member_session.control_state.driver_generation),
            target_driver: member_session.control_state.driver_ref.clone(),
            native_session_ref: member_session.native_session_ref.clone(),
            composition_fingerprint: member_session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: member_session.control_state.capability_fingerprint.clone(),
            permission_envelope_ref: Some(member_session.permission_envelope_ref.clone()),
            ..Default::default()
        },
        precondition: Default::default(),
        postcondition: Default::default(),
        payload_fingerprint: harness_store::canonical_json_fingerprint(&recovery_payload),
        payload: recovery_payload,
        issued_at: "2026-08-11T00:00:00Z".into(),
    };
    let recovery_fingerprint =
        harness_store::runtime_command_envelope_fingerprint(&recovery_command)
            .expect("recovery command fingerprint");
    let recovery_prepare_context = MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: recovery_command.authenticated_actor.clone(),
        authority_actor: Some(recovery_command.authenticated_actor.clone()),
        command_name: "node_daemon.runtime.prepare".into(),
        idempotency_key: recovery_command.idempotency_key.clone(),
        expected_version: 0,
        request_fingerprint: Some(recovery_fingerprint),
    };
    store
        .prepare_runtime_command(
            &recovery_prepare_context,
            &recovery_command,
            unix_ms(),
            "2026-08-11T00:00:01Z",
        )
        .expect("prepare ambiguous runtime effect");
    store
        .settle_runtime_command(
            &MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: recovery_command.authenticated_actor.clone(),
                authority_actor: Some(recovery_command.authenticated_actor.clone()),
                command_name: "node_daemon.runtime.settle".into(),
                idempotency_key: "runtime-command-role-view-recovery:settle".into(),
                expected_version: 1,
                request_fingerprint: None,
            },
            &recovery_command.id,
            harness_core::agentfirm_api::RuntimeCommandStatus::RecoveryRequired,
            harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown,
            None,
            Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
            "2026-08-11T00:00:02Z",
        )
        .expect("mark RecoveryRequired");
    let prepare_ambiguous_recovery = |command_id: &str| {
        let mut command = recovery_command.clone();
        command.id = command_id.into();
        command.idempotency_key = command_id.into();
        command.payload["delivery_id"] = command_id.into();
        command.payload_fingerprint = harness_store::canonical_json_fingerprint(&command.payload);
        let fingerprint = harness_store::runtime_command_envelope_fingerprint(&command)
            .expect("additional recovery command fingerprint");
        store
            .prepare_runtime_command(
                &MutationContext {
                    execution_space_id: space_id.clone(),
                    authenticated_actor: command.authenticated_actor.clone(),
                    authority_actor: Some(command.authenticated_actor.clone()),
                    command_name: "node_daemon.runtime.prepare".into(),
                    idempotency_key: command.idempotency_key.clone(),
                    expected_version: 0,
                    request_fingerprint: Some(fingerprint),
                },
                &command,
                unix_ms(),
                "2026-08-11T00:00:01Z",
            )
            .expect("prepare additional ambiguous runtime effect");
        store
            .settle_runtime_command(
                &MutationContext {
                    execution_space_id: space_id.clone(),
                    authenticated_actor: command.authenticated_actor.clone(),
                    authority_actor: Some(command.authenticated_actor.clone()),
                    command_name: "node_daemon.runtime.settle".into(),
                    idempotency_key: format!("{command_id}:settle"),
                    expected_version: 1,
                    request_fingerprint: None,
                },
                &command.id,
                harness_core::agentfirm_api::RuntimeCommandStatus::RecoveryRequired,
                harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown,
                None,
                Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
                "2026-08-11T00:00:02Z",
            )
            .expect("mark additional command RecoveryRequired");
        command
    };
    let operator_route = format!("/v1/views/operator/{node_id}?project={project_id}");
    let (status, operator_view) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator recovery view: {operator_view}");
    let recovery_action = operator_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action["kind"] == "resolve_runtime_recovery"
                    && action["target_ref"]["id"] == recovery_command.id
            })
        })
        .expect("Operator recovery action");
    assert_eq!(recovery_action["required_version"], 2);
    let recovery_route = format!(
        "/v1/agentfirm/nodes/{node_id}/runtime-commands/{}/resolve?project={project_id}",
        recovery_command.id
    );
    let recovery_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-resolve-runtime-recovery"),
        ("If-Match", "2"),
        ("X-AgentFirm-Confirm", "resolve_runtime_recovery"),
    ];
    let recovery_intent = serde_json::json!({
        "action":"resolve_runtime_recovery",
        "resolution":"confirm_not_applied",
        "evidence_ref":"check:provider-process-absent"
    });
    let operations_before_resolution = store.canonical_operations().expect("operations");
    let (status, missing_confirmation) =
        serve.post_json_with_headers(&recovery_route, &recovery_intent, &recovery_headers[..3]);
    assert_eq!(status, 409, "missing confirmation: {missing_confirmation}");
    assert_eq!(
        missing_confirmation["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    assert_eq!(
        store.canonical_operations().expect("operations"),
        operations_before_resolution,
        "confirmation failure cannot reach canonical persistence"
    );
    let (status, missing_evidence) = serve.post_json_with_headers(
        &recovery_route,
        &serde_json::json!({
            "action":"resolve_runtime_recovery",
            "resolution":"confirm_not_applied",
            "evidence_ref":"  "
        }),
        &recovery_headers,
    );
    assert_eq!(status, 409, "missing evidence: {missing_evidence}");
    assert_eq!(
        missing_evidence["error"]["code"],
        "INVALID_STATE_TRANSITION"
    );
    assert_eq!(
        store.canonical_operations().expect("operations"),
        operations_before_resolution,
        "evidence failure cannot append a canonical operation"
    );
    let (status, resolved) =
        serve.post_json_with_headers(&recovery_route, &recovery_intent, &recovery_headers);
    assert_eq!(status, 200, "resolve RecoveryRequired: {resolved}");
    assert_eq!(resolved["projection"]["status"], "failed");
    assert_eq!(resolved["projection"]["phase"], "rejected");
    assert_eq!(resolved["projection"]["effect_certainty"], "not_applied");
    assert_eq!(
        resolved["projection"]["failure_code"],
        "RECOVERY_CONFIRMED_NOT_APPLIED"
    );
    assert_eq!(
        resolved["projection"]["result"],
        serde_json::json!({
            "resolution":"confirm_not_applied",
            "evidence_ref":"check:provider-process-absent",
            "blind_replay":false
        })
    );
    let operations_after_resolution = store.canonical_operations().expect("operations");
    let (status, replayed) =
        serve.post_json_with_headers(&recovery_route, &recovery_intent, &recovery_headers);
    assert_eq!(status, 200, "replay recovery resolution: {replayed}");
    assert_eq!(replayed["replayed"], true);
    assert_eq!(
        store.canonical_operations().expect("operations"),
        operations_after_resolution,
        "replayed recovery resolution cannot repeat a provider or durable effect"
    );

    for (
        command_id,
        resolution,
        expected_status,
        expected_phase,
        expected_certainty,
        failure_code,
    ) in [
        (
            "runtime-command-role-view-confirm-applied",
            "confirm_applied",
            "applied",
            "settled",
            "applied",
            None,
        ),
        (
            "runtime-command-role-view-keep-required",
            "keep_recovery_required",
            "recovery_required",
            "recovery_required",
            "unknown",
            Some("RECOVERY_EVIDENCE_INSUFFICIENT"),
        ),
    ] {
        let command = prepare_ambiguous_recovery(command_id);
        let route = format!(
            "/v1/agentfirm/nodes/{node_id}/runtime-commands/{}/resolve?project={project_id}",
            command.id
        );
        let idempotency_key = format!("operator-resolve-{}", command.id);
        let headers = [
            ("X-AgentFirm-Token", OPERATOR_TOKEN),
            ("Idempotency-Key", idempotency_key.as_str()),
            ("If-Match", "2"),
            ("X-AgentFirm-Confirm", "resolve_runtime_recovery"),
        ];
        let evidence_ref = format!("check:{resolution}");
        let intent = serde_json::json!({
            "action":"resolve_runtime_recovery",
            "resolution":resolution,
            "evidence_ref":evidence_ref
        });
        let (status, outcome) = serve.post_json_with_headers(&route, &intent, &headers);
        assert_eq!(status, 200, "{resolution} outcome: {outcome}");
        assert_eq!(outcome["projection"]["status"], expected_status);
        assert_eq!(outcome["projection"]["phase"], expected_phase);
        assert_eq!(
            outcome["projection"]["effect_certainty"],
            expected_certainty
        );
        match failure_code {
            Some(code) => assert_eq!(outcome["projection"]["failure_code"], code),
            None => assert!(outcome["projection"]["failure_code"].is_null()),
        }
        assert_eq!(
            outcome["projection"]["result"],
            serde_json::json!({
                "resolution":resolution,
                "evidence_ref":format!("check:{resolution}"),
                "blind_replay":false
            })
        );

        let operations_after_outcome = store.canonical_operations().expect("operations");
        let (status, replay) = serve.post_json_with_headers(&route, &intent, &headers);
        assert_eq!(status, 200, "{resolution} replay: {replay}");
        assert_eq!(replay["replayed"], true);
        assert_eq!(replay["projection"], outcome["projection"]);
        assert_eq!(
            store.canonical_operations().expect("operations"),
            operations_after_outcome,
            "{resolution} HTTP replay must have zero durable delta"
        );

        let (status, conflict) = serve.post_json_with_headers(
            &route,
            &serde_json::json!({
                "action":"resolve_runtime_recovery",
                "resolution":resolution,
                "evidence_ref":format!("check:{resolution}:changed")
            }),
            &headers,
        );
        assert_eq!(status, 409, "{resolution} semantic conflict: {conflict}");
        assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_KEY_REUSED");
        assert_eq!(
            store.canonical_operations().expect("operations"),
            operations_after_outcome,
            "{resolution} HTTP semantic conflict must have zero durable delta"
        );
    }

    let host_view_route = format!("/v1/views/host-console/{}?project={project_id}", team.id);
    let (status, host_view) =
        serve.get_json_with_headers(&host_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Host RoleView: {host_view}");
    let host_action = host_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "send_message")
        })
        .expect("Host send action");
    assert!(host_action["disabled_reason"].is_null(), "{host_action}");
    let host_version = host_action["required_version"]
        .as_u64()
        .unwrap()
        .to_string();
    let host_route = format!("/v1/agentfirm/team-runs/{run_id}/messages/send?project={project_id}");
    let host_intent = serde_json::json!({
        "action":"send_message",
        "recipient_ids":[member_id],
        "body":"Host to Member canonical message",
        "response_required":true
    });
    let host_headers = action_headers(TOKEN, "host-member-canonical", &host_version);
    let (status, host_message) =
        serve.post_json_with_headers(&host_route, &host_intent, &host_headers);
    assert_eq!(status, 200, "Host message: {host_message}");
    let host_message_id = host_message["projection"]["id"].as_str().unwrap();
    let message_operations_after_first = store
        .canonical_operations_for_space(&space_id)
        .expect("message operations after first authoring");
    let messages_after_first = store
        .fabric_messages(&space_id)
        .expect("messages after first");
    let deliveries_after_first = store
        .fabric_message_deliveries(&space_id)
        .expect("deliveries after first");
    let (status, host_replay) =
        serve.post_json_with_headers(&host_route, &host_intent, &host_headers);
    assert_eq!(status, 200, "Host message exact replay: {host_replay}");
    assert_eq!(host_replay["replayed"], true);
    assert_eq!(host_replay["projection"], host_message["projection"]);
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("operations after replay"),
        message_operations_after_first,
        "exact Message replay must append no canonical operation"
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("messages after replay"),
        messages_after_first,
        "exact Message replay must not create another Message"
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("deliveries after replay"),
        deliveries_after_first,
        "exact Message replay must not create another Delivery"
    );
    let (status, changed_message) = serve.post_json_with_headers(
        &host_route,
        &serde_json::json!({
            "action":"send_message",
            "recipient_ids":[member_id],
            "body":"changed body under the same key",
            "response_required":true
        }),
        &host_headers,
    );
    assert_eq!(status, 409, "changed Message replay: {changed_message}");
    assert_eq!(changed_message["error"]["code"], "RUNTIME_COMMAND_REJECTED");
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("operations after changed replay"),
        message_operations_after_first,
        "changed Message replay must have zero canonical delta"
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("messages after changed replay"),
        messages_after_first
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("deliveries after changed replay"),
        deliveries_after_first
    );
    for changed_semantics in [
        serde_json::json!({
            "action":"send_message",
            "recipient_ids":[member_id],
            "body":"Host to Member canonical message",
            "response_required":false
        }),
        serde_json::json!({
            "action":"send_message",
            "recipient_ids":[host_id],
            "body":"Host to Member canonical message",
            "response_required":true
        }),
    ] {
        let (status, conflict) =
            serve.post_json_with_headers(&host_route, &changed_semantics, &host_headers);
        assert_eq!(status, 409, "changed Message semantics: {conflict}");
        assert_eq!(conflict["error"]["code"], "RUNTIME_COMMAND_REJECTED");
        assert_eq!(
            store
                .canonical_operations_for_space(&space_id)
                .expect("operations after semantic conflict"),
            message_operations_after_first
        );
    }

    let concurrent_intent = serde_json::json!({
        "action":"send_message",
        "recipient_ids":[member_id],
        "body":"concurrent exact canonical message",
        "response_required":false
    });
    let concurrent_headers = action_headers(TOKEN, "host-member-concurrent-exact", &host_version);
    let messages_before_concurrent = store
        .fabric_messages(&space_id)
        .expect("messages before concurrent exact requests");
    let deliveries_before_concurrent = store
        .fabric_message_deliveries(&space_id)
        .expect("deliveries before concurrent exact requests");
    let events_before_concurrent = store
        .current_team_run_events(run_id)
        .expect("events before concurrent exact requests");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let concurrent_results = std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let intent = concurrent_intent.clone();
                let serve = &serve;
                let route = &host_route;
                let headers = &concurrent_headers;
                scope.spawn(move || {
                    barrier.wait();
                    serve.post_json_with_headers(route, &intent, headers)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("concurrent message request"))
            .collect::<Vec<_>>()
    });
    assert!(
        concurrent_results.iter().all(|(status, _)| *status == 200),
        "concurrent exact results: {concurrent_results:?}"
    );
    assert_eq!(
        concurrent_results
            .iter()
            .filter(|(_, body)| body["replayed"] == false)
            .count(),
        1,
        "exactly one concurrent request must report the initial apply"
    );
    assert_eq!(
        concurrent_results
            .iter()
            .filter(|(_, body)| body["replayed"] == true)
            .count(),
        7,
        "all concurrent followers must report replay"
    );
    let concurrent_message_id = concurrent_results[0].1["projection"]["id"]
        .as_str()
        .expect("concurrent Message id");
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("messages after concurrent exact requests")
            .len(),
        messages_before_concurrent.len() + 1
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("deliveries after concurrent exact requests")
            .len(),
        deliveries_before_concurrent.len() + 1
    );
    let events_after_concurrent = store
        .current_team_run_events(run_id)
        .expect("events after concurrent exact requests");
    assert_eq!(
        events_after_concurrent.len(),
        events_before_concurrent.len() + 1
    );
    assert_eq!(
        events_after_concurrent
            .iter()
            .filter(|event| {
                event.entity_type == "message"
                    && event.entity_id == concurrent_message_id
                    && event.operation == "created"
            })
            .count(),
        1,
        "concurrent exact requests must ensure one durable TeamRun event"
    );

    let member_view_route =
        format!("/v1/views/member-workbench/{member_run_id}?project={project_id}");
    let (status, member_view) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "Member RoleView: {member_view}");
    let decision_action = member_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "request_decision")
        })
        .expect("Member decision action");
    assert!(
        decision_action["disabled_reason"].is_null(),
        "{decision_action}"
    );
    let member_version = decision_action["required_version"]
        .as_u64()
        .unwrap()
        .to_string();
    let decision_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/request-decision?project={project_id}");
    let decision_intent = serde_json::json!({
        "action":"request_decision",
        "body":"Member to Host canonical decision request"
    });
    let decision_headers = action_headers(MEMBER_TOKEN, "member-host-canonical", &member_version);
    let (status, member_message) =
        serve.post_json_with_headers(&decision_route, &decision_intent, &decision_headers);
    assert_eq!(status, 200, "Member message: {member_message}");
    let member_message_id = member_message["projection"]["id"].as_str().unwrap();
    let decision_operations = store
        .canonical_operations_for_space(&space_id)
        .expect("decision operations");
    let decision_messages = store.fabric_messages(&space_id).expect("decision messages");
    let decision_deliveries = store
        .fabric_message_deliveries(&space_id)
        .expect("decision deliveries");
    let (status, decision_replay) =
        serve.post_json_with_headers(&decision_route, &decision_intent, &decision_headers);
    assert_eq!(status, 200, "decision replay: {decision_replay}");
    assert_eq!(decision_replay["replayed"], true);
    assert_eq!(decision_replay["projection"], member_message["projection"]);
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("decision replay operations"),
        decision_operations
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("decision replay messages"),
        decision_messages
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("decision replay deliveries"),
        decision_deliveries
    );
    let (status, decision_conflict) = serve.post_json_with_headers(
        &decision_route,
        &serde_json::json!({
            "action":"request_decision",
            "body":"changed decision under the same key"
        }),
        &decision_headers,
    );
    assert_eq!(status, 409, "decision conflict: {decision_conflict}");
    assert_eq!(
        decision_conflict["error"]["code"],
        "RUNTIME_COMMAND_REJECTED"
    );
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("decision conflict operations"),
        decision_operations
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("decision conflict messages"),
        decision_messages
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("decision conflict deliveries"),
        decision_deliveries
    );
    let (status, actionable_host_view) =
        serve.get_json_with_headers(&host_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "actionable Host inbox: {actionable_host_view}");
    let host_inbox = actionable_host_view["data"]["host_inbox"]
        .as_array()
        .expect("Host inbox");
    assert_eq!(
        host_inbox.len(),
        1,
        "Host-authored broadcasts are not inbox pressure"
    );
    assert_eq!(host_inbox[0]["message_id"], member_message_id);
    assert!(host_inbox
        .iter()
        .all(|message| message["message_id"] != host_message_id));
    let reply_action = actionable_host_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "reply_message")
        })
        .expect("Host reply action");
    let reply_version = reply_action["required_version"]
        .as_u64()
        .unwrap()
        .to_string();
    let correlation_id = host_inbox[0]["correlation_id"].as_str().unwrap();
    let reply_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/reply?project={project_id}");
    let reply_intent = serde_json::json!({
        "action":"reply_message",
        "recipient_ids":[member_id],
        "body":"Host resolved the canonical decision request",
        "correlation_id":correlation_id,
        "causation_id":member_message_id
    });
    let reply_headers = action_headers(TOKEN, "host-member-canonical-reply", &reply_version);
    let (status, host_reply) =
        serve.post_json_with_headers(&reply_route, &reply_intent, &reply_headers);
    assert_eq!(status, 200, "Host reply: {host_reply}");
    let host_reply_id = host_reply["projection"]["id"].as_str().unwrap();
    let reply_operations = store
        .canonical_operations_for_space(&space_id)
        .expect("reply operations");
    let reply_messages = store.fabric_messages(&space_id).expect("reply messages");
    let reply_deliveries = store
        .fabric_message_deliveries(&space_id)
        .expect("reply deliveries");
    let (status, reply_replay) =
        serve.post_json_with_headers(&reply_route, &reply_intent, &reply_headers);
    assert_eq!(status, 200, "reply replay: {reply_replay}");
    assert_eq!(reply_replay["replayed"], true);
    assert_eq!(reply_replay["projection"], host_reply["projection"]);
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("reply replay operations"),
        reply_operations
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("reply replay messages"),
        reply_messages
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("reply replay deliveries"),
        reply_deliveries
    );
    let (status, reply_conflict) = serve.post_json_with_headers(
        &reply_route,
        &serde_json::json!({
            "action":"reply_message",
            "recipient_ids":[member_id],
            "body":"Host resolved the canonical decision request",
            "correlation_id":correlation_id,
            "causation_id":host_message_id
        }),
        &reply_headers,
    );
    assert_eq!(status, 409, "reply lineage conflict: {reply_conflict}");
    assert_eq!(reply_conflict["error"]["code"], "RUNTIME_COMMAND_REJECTED");
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("reply conflict operations"),
        reply_operations
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("reply conflict messages"),
        reply_messages
    );
    assert_eq!(
        store
            .fabric_message_deliveries(&space_id)
            .expect("reply conflict deliveries"),
        reply_deliveries
    );
    let (status, resolved_host_view) =
        serve.get_json_with_headers(&host_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "resolved Host inbox: {resolved_host_view}");
    assert_eq!(
        resolved_host_view["data"]["host_inbox"],
        serde_json::json!([])
    );

    let (status, followup_member_view) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(
        status, 200,
        "follow-up Member RoleView: {followup_member_view}"
    );
    let followup_version = followup_member_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "reply_message")
        })
        .and_then(|action| action["required_version"].as_u64())
        .expect("follow-up reply version")
        .to_string();
    let (status, followup_message) = serve.post_json_with_headers(
        &format!("/v1/agentfirm/team-runs/{run_id}/messages/reply?project={project_id}"),
        &serde_json::json!({
            "action":"reply_message",
            "recipient_ids":[host_id],
            "body":"Member follow-up after the Host reply",
            "correlation_id":correlation_id,
            "causation_id":host_reply_id,
            "response_required":true
        }),
        &action_headers(MEMBER_TOKEN, "member-host-followup", &followup_version),
    );
    assert_eq!(status, 200, "Member follow-up: {followup_message}");
    let followup_message_id = followup_message["projection"]["id"].as_str().unwrap();
    let (status, followup_host_view) =
        serve.get_json_with_headers(&host_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "follow-up Host inbox: {followup_host_view}");
    assert_eq!(
        followup_host_view["data"]["host_inbox"][0]["message_id"], followup_message_id,
        "a later member question in the same correlation remains actionable"
    );

    let messages = store
        .fabric_messages(&space_id)
        .expect("canonical Messages");
    for (id, sender) in [(host_message_id, host_id), (member_message_id, member_id)] {
        let message = messages.iter().find(|message| message.id == id).unwrap();
        assert_eq!(message.source_node_id, node_id);
        assert_eq!(message.source_node_daemon_id, lease.daemon_id);
        assert_eq!(message.source_authority_generation, lease.generation);
        assert_eq!(message.sender_actor_ref.id, sender);
        if sender == host_id {
            assert!(message.sender_agent_member_id.is_none());
            assert!(
                message.sender_session_id.is_none(),
                "external Host authoring must not fabricate a sender session: {message:?}"
            );
        } else {
            assert_eq!(message.sender_agent_member_id.as_deref(), Some(member_id));
            assert!(
                message.sender_session_id.is_some(),
                "managed Member authoring must freeze the exact sender session: {message:?}"
            );
        }
    }

    let host_delivery = store
        .fabric_message_deliveries(&space_id)
        .expect("canonical deliveries")
        .into_iter()
        .find(|delivery| {
            delivery.message_id == host_message_id
                && delivery.recipient_agent_member_id.as_deref() == Some(member_id)
        })
        .expect("Host to Member delivery");
    let daemon_context = MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: lease.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: "test.node_daemon.message_claim".into(),
        idempotency_key: "claim-host-member-canonical".into(),
        expected_version: 0,
        request_fingerprint: None,
    };
    store
        .claim_message_for_provider(
            &daemon_context,
            &host_delivery.id,
            &node_id,
            &lease.daemon_id,
            lease.generation,
            "claim-host-member-canonical",
            RuntimeDispatchMode::QueueOnly,
            "unix-ms:100",
        )
        .expect("target NodeDaemon claim");
    let mut receipt_context = daemon_context.clone();
    receipt_context.command_name = "test.node_daemon.message_receipt".into();
    receipt_context.idempotency_key = "receipt-host-member-canonical".into();
    store
        .record_message_provider_receipt(
            &receipt_context,
            &host_delivery.id,
            &node_id,
            &lease.daemon_id,
            lease.generation,
            "claim-host-member-canonical",
            "provider-receipt-host-member",
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
                idempotency_key: "ack-host-member-canonical".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &host_delivery.id,
            "unix-ms:102",
        )
        .expect("recipient ACK and cursor advance");
    let acknowledged = store
        .fabric_message_deliveries(&space_id)
        .expect("acknowledged deliveries")
        .into_iter()
        .find(|delivery| delivery.id == host_delivery.id)
        .unwrap();
    assert_eq!(
        acknowledged.status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Acknowledged
    );
    assert!(store
        .canonical_operations()
        .expect("canonical operations")
        .iter()
        .flat_map(|operation| &operation.immutable_side_records)
        .any(|record| record.get("cursor_revision").is_some()));

    // A canonical projection can remain syntactically valid while its
    // immutable content evidence is corrupt. Exact HTTP replay must reject
    // that state before returning the existing Message as success.
    let trust_ledger = space_store_root.join("agentfirm_trust_operations.jsonl");
    let mut rows = std::fs::read_to_string(&trust_ledger)
        .expect("trust ledger")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("trust row"))
        .collect::<Vec<_>>();
    let corrupted = rows.iter_mut().find(|row| {
        row["operation"]["resulting_projection"]["id"]
            == serde_json::Value::String(host_message_id.to_string())
    });
    corrupted.expect("host Message operation")["operation"]["resulting_projection"]
        ["content_fingerprint"] = serde_json::json!("sha256:corrupt");
    let corrupt_bytes = rows
        .into_iter()
        .map(|row| serde_json::to_string(&row).expect("serialize trust row"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&trust_ledger, corrupt_bytes).expect("inject content evidence corruption");
    let ledger_after_corruption = std::fs::read(&trust_ledger).expect("corrupt ledger bytes");
    let (status, corrupt_replay) =
        serve.post_json_with_headers(&host_route, &host_intent, &host_headers);
    assert_ne!(status, 200, "corrupt canonical replay: {corrupt_replay}");
    assert_eq!(
        corrupt_replay["error"]["code"],
        "RUNTIME_COMMAND_RECOVERY_REQUIRED"
    );
    assert_eq!(
        std::fs::read(&trust_ledger).expect("ledger after corrupt replay"),
        ledger_after_corruption,
        "fail-closed replay must not append over corrupt canonical evidence"
    );
}
