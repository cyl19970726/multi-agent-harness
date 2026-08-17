//! End-to-end ordinary peer-Team messaging over the canonical surfaces
//! (DOC-106, DEV-37): `team message send|inbox|claim` against a real local
//! NodeDaemon. A Team-addressed Message creates exactly one Team-subject
//! CanonicalMessageDelivery in the shared Team Inbox and never fans out to
//! Member deliveries; claim binds one exact TeamMembership generation;
//! idempotent replay never duplicates the Message or its delivery.

mod firm_env;

use firm_env::{create_canonical_agent_member, current_project_id, run_firm, TempHome};
use harness_store::HarnessStore;

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm(home, home.base(), &full);
    assert!(
        out.status.success(),
        "firm {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("command JSON")
}

fn run_err(home: &TempHome, project_id: &str, args: &[&str]) -> String {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm(home, home.base(), &full);
    assert!(
        !out.status.success(),
        "firm {args:?} unexpectedly passed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!stderr.is_empty(), "firm {args:?} failed without an error");
    stderr
}

fn seed_sender_session(home: &TempHome, project_id: &str, node_id: &str) {
    let store = HarnessStore::new(home.spaces_dir().join(project_id));
    let lease = store
        .latest_node_daemon_lease(node_id)
        .expect("read daemon lease")
        .expect("current daemon lease");
    store
        .create_agent_session(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: project_id.into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: lease.daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "integration_test.session.create".into(),
                idempotency_key: "integration-test-session:sender".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            harness_core::agentfirm_api::AgentSession {
                id: "session-sender".into(),
                agent_member_id: "sender".into(),
                node_id: node_id.into(),
                execution_space_id: project_id.into(),
                node_daemon_id: lease.daemon_id.clone(),
                node_daemon_generation: lease.generation,
                provider_kind: "codex".into(),
                provider_profile_ref: "codex-default".into(),
                permission_envelope_ref: "permission-default".into(),
                effective_permission_ceiling:
                    harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                lifecycle: harness_core::agentfirm_api::AgentSessionStatus::Idle,
                runtime_generation: 1,
                control_state: harness_core::agentfirm_api::AgentSessionControlState {
                    driver_generation: 1,
                    driver_ref: harness_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
                        node_daemon_id: lease.daemon_id.clone(),
                        node_daemon_generation: lease.generation,
                    },
                    composition_fingerprint: Some("composition:test".into()),
                    capability_fingerprint: Some("capability:test".into()),
                    ..Default::default()
                },
                native_session_ref: None,
                current_turn_id: None,
                queued_input_count: 0,
                version: 1,
                opened_at: "t1".into(),
                last_active_at: "t1".into(),
                closed_at: None,
            },
        )
        .expect("seed sender AgentSession");
}

#[test]
fn peer_team_message_send_inbox_claim_and_replay() {
    let home = TempHome::new("team-peer-messaging");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    run_json(
        &home,
        &project_id,
        &["company", "init", "--id", "company-test"],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    for (id, name) in [
        ("sender", "Sender"),
        ("target-host", "Target Host"),
        ("target-member", "Target Member"),
    ] {
        let created = create_canonical_agent_member(
            &home,
            home.base(),
            &project_id,
            id,
            name,
            "worker",
            "codex",
            &[],
        );
        assert!(created.status.success(), "create {id} failed: {created:?}");
    }
    for (mission_id, title) in [
        ("mission-source", "Source Mission"),
        ("mission-target", "Target Mission"),
    ] {
        run_json(
            &home,
            &project_id,
            &[
                "mission",
                "create",
                "--id",
                mission_id,
                "--title",
                title,
                "--objective",
                "peer messaging fixture",
                "--json",
            ],
        );
    }
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "source-team",
            "--name",
            "Source Team",
            "--description",
            "peer source",
            "--mission-id",
            "mission-source",
            "--host-agent-id",
            "sender",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "target-team",
            "--name",
            "Target Team",
            "--description",
            "peer target",
            "--mission-id",
            "mission-target",
            "--host-agent-id",
            "target-host",
        ],
    );
    let credentials = serde_json::json!([
        {"token": "peer-view-host", "actor": {"kind": "agent_member", "id": "target-host"}, "authority_actors": []},
        {"token": "peer-view-outsider", "actor": {"kind": "agent_member", "id": "sender"}, "authority_actors": []}
    ])
    .to_string();
    // The serve fixture owns the machine-scoped NodeDaemon for this home.
    let serve = firm_env::ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let store = HarnessStore::new(home.spaces_dir().join(&project_id));
        if store
            .latest_node_daemon_lease(&node_id)
            .ok()
            .flatten()
            .is_some_and(|lease| lease.status == harness_core::NodeDaemonLeaseStatus::Active)
        {
            break;
        }
        assert!(
            deadline.elapsed() < std::time::Duration::from_secs(30),
            "daemon lease never became active"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    seed_sender_session(&home, &project_id, &node_id);

    // Team-addressed send: one shared Team Inbox delivery, no Member fan-out.
    let sent = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--body",
            "hello peer team",
            "--correlation-id",
            "peer-correlation-1",
            "--idempotency-key",
            "peer-message-1",
        ],
    );
    let message_id = sent["message"]["id"].as_str().expect("message id");
    assert_eq!(sent["message"]["sender_agent_member_id"], "sender");
    assert_eq!(
        sent["message"]["collaboration_scope"]["source_team_id"],
        "source-team"
    );
    assert_eq!(
        sent["message"]["collaboration_scope"]["target_team_id"],
        "target-team"
    );
    assert_eq!(sent["message"]["work_id"], serde_json::Value::Null);
    let deliveries = sent["deliveries"].as_array().expect("deliveries");
    assert_eq!(deliveries.len(), 1, "exactly one canonical delivery");
    let delivery = &deliveries[0];
    assert_eq!(delivery["recipient_kind"], "team");
    assert_eq!(delivery["recipient_ref"], "target-team");
    assert_eq!(delivery["status"], "queued");
    assert_eq!(
        delivery["recipient_agent_member_id"],
        serde_json::Value::Null
    );
    assert_eq!(delivery["recipient_session_id"], serde_json::Value::Null);
    assert_eq!(
        delivery["resolved_team_membership_id"],
        serde_json::Value::Null
    );
    let delivery_id = delivery["id"].as_str().expect("delivery id").to_string();

    // Idempotent replay: the exact same send neither reauthors nor duplicates.
    let replayed = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--body",
            "hello peer team",
            "--correlation-id",
            "peer-correlation-1",
            "--idempotency-key",
            "peer-message-1",
        ],
    );
    assert_eq!(replayed["message"]["id"], message_id);
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let deliveries = store
        .fabric_message_deliveries(&project_id)
        .expect("deliveries")
        .into_iter()
        .filter(|delivery| delivery.message_id == message_id)
        .collect::<Vec<_>>();
    assert_eq!(deliveries.len(), 1, "replay never duplicates the delivery");
    assert_eq!(
        deliveries[0].status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
    );

    // The shared Team Inbox lists the queued delivery with full provenance.
    let inbox = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "inbox",
            "--team",
            "target-team",
            "--json",
        ],
    );
    assert_eq!(inbox["item_count"], 1);
    let item = &inbox["items"][0];
    assert_eq!(item["delivery_id"], delivery_id);
    assert_eq!(item["delivery_status"], "queued");
    assert_eq!(item["message"]["sender_agent_member_id"], "sender");
    assert_eq!(item["message"]["correlation_id"], "peer-correlation-1");
    assert_eq!(item["message"]["body"], "hello peer team");
    assert_eq!(item["message"]["source_team_id"], "source-team");

    // Claim binds the one exact target Host membership generation.
    let membership_id = "membership:target-team:target-host";
    let claimed = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "claim",
            "--team",
            "target-team",
            "--delivery-id",
            &delivery_id,
            "--membership-id",
            membership_id,
        ],
    );
    assert_eq!(claimed["status"], "routed");
    assert_eq!(claimed["resolved_team_membership_id"], membership_id);
    assert_eq!(claimed["recipient_agent_member_id"], "target-host");
    assert!(claimed["claim_id"].as_str().is_some());

    // A second distinct claim on the resolved delivery is side-effect free.
    let second = run_err(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "claim",
            "--team",
            "target-team",
            "--delivery-id",
            &delivery_id,
            "--membership-id",
            membership_id,
            "--claim-id",
            "peer-claim-second",
        ],
    );
    assert!(
        second.contains("only one unresolved queued Team-subject delivery"),
        "second claim must be rejected: {second}"
    );
    let inbox = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "inbox",
            "--team",
            "target-team",
            "--json",
        ],
    );
    assert_eq!(
        inbox["item_count"], 0,
        "claimed deliveries leave the actionable inbox"
    );
    let inbox = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "inbox",
            "--team",
            "target-team",
            "--all",
            "--json",
        ],
    );
    assert_eq!(inbox["item_count"], 1);
    assert_eq!(inbox["items"][0]["delivery_status"], "routed");

    // The HTTP RoleView emits the same Team Inbox with delivery status and
    // author/Team provenance for the operator surface (DEV-38 consumes it).
    let view_route = format!("/v1/views/team-inbox/target-team?project={project_id}");
    let (status, view) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", "peer-view-host")]);
    assert_eq!(status, 200, "team inbox view: {view}");
    assert_eq!(view["view_kind"], "team_inbox");
    assert_eq!(view["data"]["team"]["team_id"], "target-team");
    let items = view["data"]["items"].as_array().expect("view items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["delivery_id"], delivery_id);
    assert_eq!(items[0]["delivery_status"], "routed");
    assert_eq!(items[0]["resolved_team_membership_id"], membership_id);
    assert_eq!(items[0]["message"]["sender_agent_member_id"], "sender");
    assert_eq!(items[0]["message"]["correlation_id"], "peer-correlation-1");
    assert_eq!(
        items[0]["message"]["collaboration_scope"]["target_team_id"],
        "target-team"
    );
    assert_eq!(
        view["data"]["subscription"]["id"].as_str(),
        Some("team-inbox:target-team")
    );

    // A non-member identity cannot read another Team's inbox.
    let (status, denied) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", "peer-view-outsider")]);
    assert_eq!(status, 403, "outsider read must be denied: {denied}");
    let (status, missing) = serve.get_json_with_headers(
        &format!("/v1/views/team-inbox/missing-team?project={project_id}"),
        &[("X-AgentFirm-Token", "peer-view-host")],
    );
    assert_eq!(status, 404, "unknown Team inbox view: {missing}");
}

#[test]
fn peer_team_direct_member_send_binds_delivery_and_ambiguous_claim_stays_queued() {
    let home = TempHome::new("team-peer-messaging-direct");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    run_json(
        &home,
        &project_id,
        &["company", "init", "--id", "company-test"],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    for (id, name) in [
        ("sender", "Sender"),
        ("host-b", "Host B"),
        ("member-b", "Member B"),
    ] {
        let created = create_canonical_agent_member(
            &home,
            home.base(),
            &project_id,
            id,
            name,
            "worker",
            "codex",
            &[],
        );
        assert!(created.status.success(), "create {id} failed: {created:?}");
    }
    for (mission_id, title) in [
        ("mission-source", "Source Mission"),
        ("mission-target", "Target Mission"),
    ] {
        run_json(
            &home,
            &project_id,
            &[
                "mission",
                "create",
                "--id",
                mission_id,
                "--title",
                title,
                "--objective",
                "peer messaging fixture",
                "--json",
            ],
        );
    }
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "source-team",
            "--name",
            "Source Team",
            "--description",
            "peer source",
            "--mission-id",
            "mission-source",
            "--host-agent-id",
            "sender",
        ],
    );
    // The target Team has two eligible Members from the start.
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "target-team",
            "--name",
            "Target Team",
            "--description",
            "peer target",
            "--mission-id",
            "mission-target",
            "--host-agent-id",
            "host-b",
            "--member",
            "member-b",
        ],
    );
    let daemon = run_firm(
        &home,
        home.base(),
        &["--project", &project_id, "daemon", "start"],
    );
    assert!(
        daemon.status.success(),
        "daemon start failed: {}",
        String::from_utf8_lossy(&daemon.stderr)
    );
    seed_sender_session(&home, &project_id, &node_id);

    // A Team-addressed message never selects between the two Members: the
    // single shared delivery stays queued and no Member delivery exists.
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--body",
            "team broadcast",
            "--idempotency-key",
            "peer-broadcast-1",
        ],
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let deliveries = store
        .fabric_message_deliveries(&project_id)
        .expect("deliveries")
        .into_iter()
        .filter(|delivery| delivery.message_id == "message:peer-broadcast-1")
        .collect::<Vec<_>>();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].recipient_kind,
        harness_core::agentfirm_api::MessageSubjectKind::Team
    );
    let claim_error = run_err(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "claim",
            "--team",
            "target-team",
            "--delivery-id",
            &deliveries[0].id,
            "--membership-id",
            "membership:target-team:host-b",
        ],
    );
    assert!(
        claim_error.contains("exactly one eligible"),
        "ambiguous Team membership must keep the delivery queued: {claim_error}"
    );
    let deliveries = store
        .fabric_message_deliveries(&project_id)
        .expect("deliveries")
        .into_iter()
        .filter(|delivery| delivery.message_id == "message:peer-broadcast-1")
        .collect::<Vec<_>>();
    assert_eq!(
        deliveries[0].status,
        harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued,
        "the rejected claim has zero side effects"
    );

    // A direct Member target binds exactly one delivery at admission and never
    // enters the shared Team Inbox.
    let direct = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--to-member",
            "member-b",
            "--body",
            "direct note",
            "--idempotency-key",
            "peer-direct-1",
        ],
    );
    let direct_deliveries = direct["deliveries"].as_array().expect("deliveries");
    assert_eq!(direct_deliveries.len(), 1);
    assert_eq!(direct_deliveries[0]["recipient_kind"], "agent_member");
    assert_eq!(direct_deliveries[0]["recipient_ref"], "member-b");
    assert_eq!(
        direct_deliveries[0]["resolved_team_membership_id"],
        "membership:target-team:member-b"
    );
    assert_eq!(direct_deliveries[0]["status"], "queued");
    let inbox = run_json(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "inbox",
            "--team",
            "target-team",
            "--all",
            "--json",
        ],
    );
    assert_eq!(
        inbox["item_count"], 1,
        "the direct Member delivery never enters the shared Team Inbox"
    );
}

#[test]
fn peer_team_message_send_requires_exact_surfaces() {
    let home = TempHome::new("team-peer-messaging-usage");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("project root");
    let initialized = run_firm(&home, &project_root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    // No Node identity, Company, Teams, or daemon: every surface fails closed
    // with a precise error instead of guessing authority.
    let error = run_err(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--body",
            "hello",
        ],
    );
    assert!(
        error.contains("local ExecutionNode is not initialized"),
        "missing Node identity must fail closed: {error}"
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    let error = run_err(
        &home,
        &project_id,
        &[
            "team",
            "message",
            "send",
            "--from-team",
            "source-team",
            "--from-member",
            "sender",
            "--to-team",
            "target-team",
            "--body",
            "hello",
        ],
    );
    assert!(
        error.contains("peer-Team target Team is not in this Execution Space"),
        "missing target Team must fail closed: {error}"
    );
    let error = run_err(
        &home,
        &project_id,
        &["team", "message", "inbox", "--team", "target-team"],
    );
    assert!(
        error.contains("team not found"),
        "unknown Team inbox: {error}"
    );
}
