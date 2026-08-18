//! HTTP boundary coverage for the Wave 4B local RoleViews.

mod fake_provider;
mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm, ServeHandle,
    TempHome,
};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus, DeliveryClaim,
    MutationContext, NativeSessionAvailability, NativeSessionRef, PermissionCeiling,
    RuntimeCommandBinding, RuntimeDispatchMode, RuntimeDriverRef,
};
use harness_core::{
    ExecutionNode, ExecutionNodeStatus, MemberCoordinationStatus, MemberRunStatus,
    NodeProjectRegistration, NodeProjectRegistrationStatus, ProviderCompatibilityStatus,
};
use harness_store::{CurrentTeamMemberLifecycleTransition, HarnessStore};

const TOKEN: &str = "role-view-local-capability";
const MEMBER_TOKEN: &str = "role-view-member-capability";
const SIBLING_MEMBER_TOKEN: &str = "role-view-sibling-member-capability";
const OPERATOR_TOKEN: &str = "role-view-operator-capability";
const WRONG_OPERATOR_TOKEN: &str = "role-view-wrong-operator-capability";
const DELEGATED_OPERATOR_TOKEN: &str = "role-view-delegated-operator-capability";

fn ledger_digest(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut rows = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // Lease ledgers are autonomous control bookkeeping: the NodeDaemon
            // and Team Supervisor renew and compact them on their own timers,
            // independent of any HTTP action under test. Whole-ledger purity
            // here must cover product state only, or a legitimate background
            // renewal racing the before/after snapshots fails the comparison.
            let autonomous_bookkeeping = matches!(
                name.as_str(),
                "node_daemon_leases.jsonl" | "team_supervisor_leases.jsonl"
            );
            (name.ends_with(".jsonl") && !autonomous_bookkeeping)
                .then(|| (name, std::fs::read(entry.path()).expect("read ledger")))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

fn file_tree_digest(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn visit(base: &std::path::Path, current: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(current) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(base, &path, out);
            } else if path.is_file() {
                out.push((
                    path.strip_prefix(base)
                        .expect("digest path belongs to base")
                        .display()
                        .to_string(),
                    std::fs::read(&path).expect("read digest file"),
                ));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn action_headers<'a>(token: &'a str, key: &'a str, version: &'a str) -> [(&'a str, &'a str); 3] {
    [
        ("X-AgentFirm-Token", token),
        ("Idempotency-Key", key),
        ("If-Match", version),
    ]
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn assert_exact_role_action_replay(
    serve: &ServeHandle,
    route: &str,
    body: &serde_json::Value,
    headers: &[(&str, &str)],
    label: &str,
) -> serde_json::Value {
    let (status, committed) = serve.post_json_with_headers(route, body, headers);
    assert_eq!(status, 200, "{label} commit: {committed}");
    assert_eq!(committed["replayed"], false, "{label} first write");
    let (status, replayed) = serve.post_json_with_headers(route, body, headers);
    assert_eq!(status, 200, "{label} replay: {replayed}");
    assert_eq!(replayed["replayed"], true, "{label} replay marker");
    assert_eq!(
        replayed["event_id"], committed["event_id"],
        "{label} event identity"
    );
    committed
}

#[test]
fn role_action_loop_is_authenticated_cas_bound_and_legacy_writers_are_gone() {
    let home = TempHome::new("role-action-loop");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let space_id = current_space_id(&home);
    let worker_native_session_id = "019f-role-view-owner-session";
    let rollout_dir = home.home().join(".codex/sessions/2026/08/13");
    std::fs::create_dir_all(&rollout_dir).expect("Codex rollout fixture root");
    std::fs::write(
        rollout_dir.join(format!(
            "rollout-2026-08-13T00-00-00-{worker_native_session_id}.jsonl"
        )),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{worker_native_session_id}\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_reasoning\",\"turn_id\":\"turn-owner-1\",\"text\":\"raw-chain-of-thought-must-not-appear\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"turn_id\":\"turn-owner-1\",\"message\":\"display-safe authored result\"}}}}\n\
             {{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn-owner-1\"}}}}\n"
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
    let mission_id = "mission-role-action-loop".to_string();
    firm_env::seed_historical_mission(&home, &project_id, &mission_id, "Role action loop");
    let host_id = "agent-role-action-host";
    let host = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        host_id,
        "Role Action Host",
        "host",
        "codex",
        &[],
    );
    assert!(host.status.success(), "host: {host:?}");
    let worker_id = "agent-role-action-worker";
    let worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        worker_id,
        "Role Action Worker",
        "builder",
        "codex",
        &[],
    );
    assert!(worker.status.success(), "worker: {worker:?}");
    let sibling_worker_id = "agent-role-action-sibling";
    let sibling_worker = create_canonical_agent_member(
        &home,
        &root,
        &project_id,
        sibling_worker_id,
        "Role Action Sibling",
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
        "Role action team",
        "--description",
        "Store-live action integration",
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
    let sibling_node_id = "10000000-0000-4000-8000-000000000002";
    store
        .insert_execution_node(&ExecutionNode {
            id: sibling_node_id.into(),
            display_name: "Sibling execution node".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "2026-08-10T00:00:00Z".into(),
            updated_at: "2026-08-10T00:00:00Z".into(),
        })
        .expect("insert sibling Node");
    store
        .register_node_project(
            &NodeProjectRegistration {
                node_id: sibling_node_id.into(),
                execution_space_id: space_id.clone(),
                project_binding_id: project_id.clone(),
                status: NodeProjectRegistrationStatus::Active,
                created_at: "2026-08-10T00:00:00Z".into(),
                updated_at: "2026-08-10T00:00:00Z".into(),
            },
            &space_id,
        )
        .expect("register sibling Node");
    let team = store
        .latest_teams()
        .expect("teams")
        .into_values()
        .next()
        .expect("default team");
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
    },{
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
        "authority_actors": []
    },{
        "token": WRONG_OPERATOR_TOKEN,
        "actor": {"kind":"service","id":sibling_node_id},
        "authority_actors": []
    },{
        "token": DELEGATED_OPERATOR_TOKEN,
        "actor": {"kind":"human","id":"operator-human"},
        "authority_actors": [{"kind":"service","id":node_id}]
    }])
    .to_string();
    let other_space = run_firm(
        &home,
        &root,
        &[
            "space",
            "init",
            "--id",
            "role-action-empty-space",
            "--name",
            "Role Action Empty Space",
            "--project-binding",
            &project_id,
        ],
    );
    assert!(other_space.status.success(), "other space: {other_space:?}");
    let other_project_root = home.base().join("other-project-binding");
    std::fs::create_dir_all(&other_project_root).expect("other project root");
    let other_project = run_firm(&home, &other_project_root, &["init"]);
    assert!(
        other_project.status.success(),
        "other Project Binding: {other_project:?}"
    );
    let other_project_id = current_project_id(&home);
    assert_ne!(
        other_project_id, project_id,
        "cross-binding test requires two distinct Project Bindings"
    );
    let restored_project = run_firm(&home, &root, &["project", "switch", project_id.as_str()]);
    assert!(
        restored_project.status.success(),
        "restore primary Project Binding: {restored_project:?}"
    );
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &["--space", &space_id],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let (status, created_run) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "agent_team_id": team.id,
            "objective": "Store-live AgentFirm action loop",
            "members": [
                {"agent_member_id":worker_id,"name":"worker","role":"builder","provider":"codex","resume_native_session_id":worker_native_session_id},
                {"agent_member_id":sibling_worker_id,"name":"sibling","role":"builder","provider":"codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");
    let daemon = store
        .latest_node_daemon_lease(node_id)
        .expect("NodeDaemon lease")
        .expect("active NodeDaemon lease");
    store
        .create_agent_session(
            &MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: daemon.daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "test.provider_projection.session".into(),
                idempotency_key: "test-provider-projection-worker-session".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            AgentSession {
                id: "agent-session:role-view-owner:1".into(),
                agent_member_id: worker_id.into(),
                node_id: node_id.into(),
                execution_space_id: space_id.clone(),
                node_daemon_id: daemon.daemon_id.clone(),
                node_daemon_generation: daemon.generation,
                provider_kind: "codex".into(),
                provider_profile_ref: "codex-app-server-v1".into(),
                permission_envelope_ref: format!("agent-member:{worker_id}:permission"),
                effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
                lifecycle: AgentSessionStatus::Idle,
                runtime_generation: 1,
                control_state: AgentSessionControlState {
                    driver_generation: 1,
                    driver_ref: RuntimeDriverRef::NodeDaemon {
                        node_daemon_id: daemon.daemon_id.clone(),
                        node_daemon_generation: daemon.generation,
                    },
                    composition_fingerprint: Some("role-view:composition".into()),
                    capability_fingerprint: Some("role-view:capability".into()),
                    ..Default::default()
                },
                native_session_ref: Some(NativeSessionRef {
                    provider: "codex".into(),
                    execution_mode: "codex_app_server".into(),
                    native_session_id: worker_native_session_id.into(),
                    native_locator_kind: "codex_rollout".into(),
                    provider_version: None,
                    adapter_contract_version: "codex-app-server-v1".into(),
                    availability: NativeSessionAvailability::Available,
                    supports_resume: true,
                    last_verified_at: Some("2026-08-13T00:00:00Z".into()),
                    parent_native_session_id: None,
                }),
                current_turn_id: None,
                queued_input_count: 0,
                version: 1,
                opened_at: "2026-08-13T00:00:00Z".into(),
                last_active_at: "2026-08-13T00:00:00Z".into(),
                closed_at: None,
            },
        )
        .expect("provider projection AgentSession");

    let before = store.work_operations().expect("before operations").len();
    let legacy_route = format!("/v1/team-runs/{run_id}/works?project={project_id}");
    let (status, retired) = serve.post_json(
        &legacy_route,
        &serde_json::json!({"title":"bypass","completion_criteria_markdown":"must not persist"}),
    );
    assert_eq!(status, 410, "legacy writer: {retired}");
    let (status, retired_delegation) = serve.post_json(
        &format!("/v1/work-delegations?project={project_id}"),
        &serde_json::json!({"performed_by_actor":{"kind":"host","id":"spoof"}}),
    );
    assert_eq!(
        status, 410,
        "legacy delegation writer: {retired_delegation}"
    );
    assert_eq!(
        store.work_operations().expect("after retired").len(),
        before
    );

    let action_route = format!("/v1/agentfirm/team-runs/{run_id}/works?project={project_id}");
    let intent = serde_json::json!({
        "action":"create_work",
        "work_id":"work-store-live-1",
        "title":"Close the local product loop",
        "completion_criteria_markdown":"Authenticated browser action is visible after refetch",
        "claim_mode":"team_claim"
    });
    let (status, denied) = serve.post_json(&action_route, &intent);
    assert_eq!(status, 401, "unauth action: {denied}");
    assert_eq!(store.work_operations().expect("after unauth").len(), before);

    let headers = action_headers(TOKEN, "create-store-live-1", "0");
    let (status, created) = serve.post_json_with_headers(&action_route, &intent, &headers);
    assert_eq!(status, 200, "authenticated create: {created}");
    assert_eq!(created["projection"]["id"], "work-store-live-1");
    assert_eq!(created["replayed"], false);
    let (status, replay) = serve.post_json_with_headers(&action_route, &intent, &headers);
    assert_eq!(status, 200, "idempotent replay: {replay}");
    assert_eq!(replay["event_id"], created["event_id"]);
    assert_eq!(replay["replayed"], true);

    let view_route = format!("/v1/views/host-console/{}?project={project_id}", team.id);
    let (status, refreshed) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Host RoleView: {refreshed}");
    assert_eq!(refreshed["data"]["mission_context"]["id"], mission_id);
    assert!(refreshed["data"]["mission_context"]["log"].is_array());
    assert!(refreshed["data"]["host_inbox"].is_array());
    assert!(refreshed["data"]["member_runtime"].is_array());
    assert!(refreshed["data"]["runtime_recovery"].is_array());
    assert_eq!(refreshed["data"]["pressure_summary"]["ready_work"], 1);
    assert_eq!(
        refreshed["data"]["pressure_summary"]["total_members"], 2,
        "Team Lead must not be synthesized into execution capacity"
    );
    assert!(refreshed["data"]["all_works"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    assert!(refreshed["data"]["work_queues"]["unassigned"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    assert!(refreshed["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions
            .iter()
            .all(|action| action["required_version"].is_u64())));
    let run_identity_route = format!("/v1/views/host-console/{run_id}?project={project_id}");
    let (status, run_identity_view) =
        serve.get_json_with_headers(&run_identity_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 200,
        "TeamRun-addressed Host RoleView: {run_identity_view}"
    );
    assert_eq!(run_identity_view["data"]["team_ref"], team.id);
    let message_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/send?project={project_id}");
    let message_intent = serde_json::json!({
        "action":"send_message",
        "recipient_ids":[worker_id],
        "body":"Store-live CAS-bound Team Message",
        "response_required":true
    });
    let before_stale_message = ledger_digest(serve.fixture_store_root());
    let stale_message_headers = action_headers(TOKEN, "message-stale-team-revision", "0");
    let (status, stale_message) =
        serve.post_json_with_headers(&message_route, &message_intent, &stale_message_headers);
    assert_eq!(status, 409, "stale Team Message CAS: {stale_message}");
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_stale_message,
        "stale Team Message changed durable state"
    );
    let team_revision = refreshed["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "send_message")
        })
        .and_then(|action| action["required_version"].as_u64())
        .expect("Team revision from send_message action")
        .to_string();
    assert!(
        refreshed["allowed_actions"]
            .as_array()
            .and_then(|actions| actions
                .iter()
                .find(|action| action["kind"] == "send_message"))
            .is_some_and(|action| action["disabled_reason"].is_null()),
        "durable Team membership/subscription fabric must enable canonical Message authoring"
    );
    let message_headers = action_headers(TOKEN, "message-current-team-revision", &team_revision);
    let before_message = ledger_digest(serve.fixture_store_root());
    let (status, published_message) =
        serve.post_json_with_headers(&message_route, &message_intent, &message_headers);
    assert_eq!(status, 200, "canonical Team Message: {published_message}");
    assert_eq!(
        published_message["projection"]["body"],
        message_intent["body"]
    );
    assert_ne!(
        ledger_digest(serve.fixture_store_root()),
        before_message,
        "canonical Team Message did not change durable state"
    );

    let member_runs = created_run["result"]["member_runs"]
        .as_array()
        .expect("member runs");
    let member_run_id = member_runs
        .iter()
        .find(|run| run["agent_member_id"] == worker_id)
        .and_then(|run| run["id"].as_str())
        .expect("member run id");
    let sibling_member_run_id = member_runs
        .iter()
        .find(|run| run["agent_member_id"] == sibling_worker_id)
        .and_then(|run| run["id"].as_str())
        .expect("sibling member run id");
    let (status, retired_native_activity) = serve.get_json_with_headers(
        &format!("/v1/member-runs/{member_run_id}/native-activity?project={project_id}"),
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 410,
        "run-addressed native history reader must remain retired: {retired_native_activity}"
    );
    let member_agent_workspace_route =
        format!("/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={worker_id}");
    let (status, host_selected_member) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", TOKEN)],
    );
    assert_eq!(
        status, 200,
        "Host-selected Member AgentWorkspace: {host_selected_member}"
    );
    assert_eq!(host_selected_member["view_kind"], "agent_workspace");
    assert_eq!(
        host_selected_member["data"]["projection_scope"],
        "host_member_public"
    );
    assert_eq!(
        host_selected_member["data"]["selected_agent"]["agent_member_ref"]["id"],
        worker_id
    );
    for private_field in [
        "sessions",
        "selected_session_id",
        "session_activity",
        "session_event_projection",
        "live_provider_activity",
    ] {
        assert!(
            host_selected_member["data"].get(private_field).is_none(),
            "Host-selected Member must structurally omit private field {private_field}"
        );
    }
    assert_eq!(
        host_selected_member["data"]["selected_agent"]["current_member_run_ref"],
        serde_json::Value::Null,
        "Host-selected Member must not receive the private MemberRun binding"
    );
    for key in ["provider", "execution_mode", "runtime_status"] {
        assert!(
            host_selected_member["data"]["selected_agent"][key].is_null(),
            "Host-selected Member leaked selected_agent.{key}"
        );
    }
    for key in [
        "provider_profile_ref",
        "model_preference",
        "workspace_policy",
        "permission_ceiling",
        "workspace_binding",
    ] {
        assert!(
            host_selected_member["data"]["configuration"][key].is_null(),
            "Host-selected Member leaked configuration.{key}"
        );
    }
    assert_eq!(
        host_selected_member["data"]["configuration"]["tool_refs"],
        serde_json::json!([]),
        "Host-selected Member leaked configured tools"
    );
    assert!(host_selected_member["data"]["roster"]
        .as_array()
        .expect("public roster")
        .iter()
        .all(|member| member.get("runtime_state").is_none()
            && member["coordination_status"].is_null()
            && member["capacity"] == "not_projected"));
    assert!(host_selected_member["data"].get("runtime_fabric").is_none());
    assert!(host_selected_member["data"]["messages"]
        .as_array()
        .expect("public messages")
        .iter()
        .all(|message| message["deliveries"] == serde_json::json!([])));
    assert!(host_selected_member["data"]["works"]
        .as_array()
        .expect("public works")
        .iter()
        .all(|work| work["current_member_run_ref"].is_null()
            && work["runtime_summary"]["state"] == "not_projected"
            && work["workspace_summary"]["binding_id"].is_null()));
    assert!(
        host_selected_member["allowed_actions"]
            .as_array()
            .expect("Host public controls")
            .iter()
            .any(|action| action["kind"] == "close_member_run"
                && action["target_ref"]["id"] == member_run_id),
        "Host control projection must remain available without private Session projection"
    );
    let before_owner_projection_ledgers = ledger_digest(serve.fixture_store_root());
    let before_owner_projection_source = file_tree_digest(&home.home().join(".codex"));
    let (status, member_self_workspace) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "exact-self Member AgentWorkspace: {member_self_workspace}"
    );
    assert!(
        member_self_workspace["data"]
            .get("live_provider_activity")
            .is_some(),
        "exact-self view must carry the nullable volatile live slot"
    );
    assert!(
        member_self_workspace["data"]
            .get("session_event_projection")
            .is_some(),
        "exact-self view must carry an on-demand Session projection or an explicit unavailable result"
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
        "turn-owner-1"
    );
    assert_eq!(owner_projection["episodes"][0]["terminal"], true);
    let serialized_owner_projection =
        serde_json::to_string(owner_projection).expect("projection JSON");
    assert!(serialized_owner_projection.contains("display-safe authored result"));
    assert!(!serialized_owner_projection.contains("raw-chain-of-thought-must-not-appear"));
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_owner_projection_ledgers,
        "on-demand provider projection must not write Harness ledgers"
    );
    assert_eq!(
        file_tree_digest(&home.home().join(".codex")),
        before_owner_projection_source,
        "on-demand provider projection must not rewrite provider-native storage"
    );
    let cross_binding_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={other_project_id}&agent_id={worker_id}"
    );
    let (status, cross_binding_workspace) =
        serve.get_json_with_headers(&cross_binding_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "cross-binding owner view");
    let cross_binding_projection = &cross_binding_workspace["data"]["session_event_projection"];
    assert!(
        cross_binding_projection["disabled_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("another Project Binding")),
        "same Execution Space must not expose a Session through another Project Binding: {cross_binding_workspace}"
    );
    assert_eq!(
        cross_binding_projection["agent_session_id"],
        serde_json::Value::Null,
        "cross-binding projection must not expose an AgentSession id"
    );
    assert_eq!(
        cross_binding_workspace["data"]["live_provider_activity"],
        serde_json::Value::Null,
        "same Execution Space must not expose a live overlay through another Project Binding"
    );
    for retired_history_field in ["sessions", "selected_session_id", "session_activity"] {
        assert!(
            member_self_workspace["data"]
                .get(retired_history_field)
                .is_none(),
            "legacy provider field {retired_history_field} must remain retired"
        );
    }
    let (status, sibling_denied) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", SIBLING_MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 403,
        "sibling Agent private Session must fail closed: {sibling_denied}"
    );
    assert_eq!(sibling_denied["error"]["code"], "NOT_AUTHORIZED");
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
    let host_agent_workspace_route = format!(
        "/v1/views/agent-workspace/{run_id}?project={project_id}&agent_id={}",
        team.host_agent_id
    );
    let (status, exact_host_workspace) =
        serve.get_json_with_headers(&host_agent_workspace_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 200,
        "exact Host AgentWorkspace: {exact_host_workspace}"
    );
    assert_eq!(
        exact_host_workspace["data"]["selected_agent"]["is_host"],
        true
    );
    assert!(
        exact_host_workspace["data"]
            .get("session_event_projection")
            .is_some(),
        "exact Host self view carries an explicit owner projection state"
    );
    let (status, member_denied_host) = serve.get_json_with_headers(
        &host_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 403,
        "Member must not read Host provider Session: {member_denied_host}"
    );
    assert_eq!(member_denied_host["error"]["code"], "NOT_AUTHORIZED");
    let member_run_version = store
        .trust_member_runs(&space_id)
        .expect("MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("canonical MemberRun")
        .version
        .to_string();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "role-view@example.invalid"],
        vec!["config", "user.name", "Role View Test"],
        vec!["add", "-A"],
        vec!["commit", "--allow-empty", "-m", "workspace proof fixture"],
    ] {
        let output = std::process::Command::new("git")
            .current_dir(&root)
            .args(&args)
            .output()
            .expect("run git workspace fixture command");
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }
    let before_hostile_workspace = ledger_digest(serve.fixture_store_root());
    let workspace_route = format!(
        "/v1/agentfirm/member-runs/{member_run_id}/workspace/provision?project={project_id}"
    );
    let workspace_headers = action_headers(
        MEMBER_TOKEN,
        "hostile-workspace-escape",
        member_run_version.as_str(),
    );
    let (status, workspace_rejected) = serve.post_json_with_headers(
        &workspace_route,
        &serde_json::json!({
            "action":"provision_workspace",
            "project_binding_id":project_id,
            "mode":"inherit",
            "ownership":"shared_project",
            "canonical_root":home.base()
        }),
        &workspace_headers,
    );
    assert_eq!(
        status, 409,
        "workspace escape must fail closed: {workspace_rejected}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_hostile_workspace,
        "hostile workspace intent changed durable state"
    );
    let safe_workspace_headers = action_headers(
        TOKEN,
        "safe-workspace-provision",
        member_run_version.as_str(),
    );
    let (status, provisioned_workspace) = serve.post_json_with_headers(
        &workspace_route,
        &serde_json::json!({
            "action":"provision_workspace",
            "project_binding_id":project_id,
            "mode":"worktree",
            "ownership":"managed",
            "canonical_root":root
        }),
        &safe_workspace_headers,
    );
    assert_eq!(
        status, 200,
        "server-observed workspace provision: {provisioned_workspace}"
    );
    assert!(provisioned_workspace["projection"]["git_common_dir"]
        .as_str()
        .is_some());
    assert_eq!(provisioned_workspace["projection"]["lifecycle"], "ready");
    let attach_workspace_route =
        format!("/v1/agentfirm/member-runs/{member_run_id}/workspace/attach?project={project_id}");
    let attached_workspace = assert_exact_role_action_replay(
        &serve,
        &attach_workspace_route,
        &serde_json::json!({"action":"attach_workspace"}),
        &action_headers(TOKEN, "safe-workspace-attach", "3"),
        "workspace attach",
    );
    assert_eq!(attached_workspace["projection"]["lifecycle"], "attached");
    let member_view_route =
        format!("/v1/views/member-workbench/{member_run_id}?project={project_id}");
    let (status, member_view) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "Member RoleView: {member_view}");
    assert!(member_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["kind"] == "claim_work")));
    let decision_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/request-decision?project={project_id}");
    let decision_headers = action_headers(MEMBER_TOKEN, "request-host-decision", &team_revision);
    assert!(member_view["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|action| action["kind"] == "reply_message")
        })
        .is_some_and(|action| action["disabled_reason"].is_null()));
    let before_decision = ledger_digest(serve.fixture_store_root());
    let (status, decision) = serve.post_json_with_headers(
        &decision_route,
        &serde_json::json!({
            "action":"request_decision",
            "body":"Host decision is required",
            "evidence_refs":["check:member-request-decision"]
        }),
        &decision_headers,
    );
    assert_eq!(status, 200, "canonical Member request-decision: {decision}");
    assert_eq!(decision["projection"]["kind"], "request_decision");
    assert_ne!(
        ledger_digest(serve.fixture_store_root()),
        before_decision,
        "canonical request-decision did not change durable state"
    );
    let claim_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/claim?project={project_id}"
    );
    let claim_headers = action_headers(MEMBER_TOKEN, "claim-store-live-1", "1");
    let (status, claimed) = serve.post_json_with_headers(
        &claim_route,
        &serde_json::json!({"action":"claim_work"}),
        &claim_headers,
    );
    assert_eq!(status, 200, "member claim: {claimed}");
    assert_eq!(claimed["projection"]["active_member_run_id"], member_run_id);
    let before_sibling_attempts = ledger_digest(serve.fixture_store_root());
    let sibling_record_attempts = [
        (
            "reports",
            "sibling-progress-spoof",
            serde_json::json!({"action":"write_report","summary":"spoofed sibling progress"}),
        ),
        (
            "findings",
            "sibling-finding-spoof",
            serde_json::json!({"action":"write_finding","kind":"discovery","summary":"spoofed finding","detail_markdown":"not the owner","confidence":"high"}),
        ),
        (
            "failure-analyses",
            "sibling-failure-spoof",
            serde_json::json!({"action":"write_failure","observed_failure":"spoofed failure","impact":"none","primary_cause_status":"unknown","retry_safety":"unknown","recommended_host_decision":"reject","confidence":"high"}),
        ),
        (
            "revise",
            "sibling-revise-spoof",
            serde_json::json!({"action":"revise_work","result_summary":"spoofed revision","candidate_revision":"abcdef0123456789","check_refs":["check:spoof"]}),
        ),
    ];
    for (operation, key, intent) in sibling_record_attempts {
        let route = format!(
            "/v1/agentfirm/teams/{}/works/work-store-live-1/{operation}?project={project_id}",
            team.id
        );
        let (status, rejected) = serve.post_json_with_headers(
            &route,
            &intent,
            &action_headers(SIBLING_MEMBER_TOKEN, key, "2"),
        );
        assert_eq!(status, 409, "sibling {operation} spoof: {rejected}");
    }
    let sibling_submit_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/submit?project={project_id}"
    );
    let (status, sibling_submit) = serve.post_json_with_headers(
        &sibling_submit_route,
        &serde_json::json!({"action":"submit_work","result_summary":"spoofed result","candidate_revision":"abcdef0123456789","check_refs":["check:spoof"]}),
        &action_headers(SIBLING_MEMBER_TOKEN, "sibling-submit-spoof", "2"),
    );
    assert_eq!(status, 409, "sibling submit spoof: {sibling_submit}");
    let linked_message_headers = action_headers(
        SIBLING_MEMBER_TOKEN,
        "sibling-linked-message-spoof",
        &team_revision,
    );
    let (status, sibling_linked_message) = serve.post_json_with_headers(
        &decision_route,
        &serde_json::json!({
            "action":"request_decision",
            "body":"false Work linkage",
            "work_id":"work-store-live-1"
        }),
        &linked_message_headers,
    );
    assert_eq!(
        status, 409,
        "sibling linked message spoof: {sibling_linked_message}"
    );
    let (status, unknown_linked_message) = serve.post_json_with_headers(
        &decision_route,
        &serde_json::json!({
            "action":"request_decision",
            "body":"unknown Work linkage",
            "work_id":"missing-work"
        }),
        &action_headers(
            SIBLING_MEMBER_TOKEN,
            "unknown-linked-message",
            &team_revision,
        ),
    );
    assert_eq!(status, 409, "unknown linked Work: {unknown_linked_message}");
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_sibling_attempts,
        "rejected sibling/unknown Work mutations must have zero durable side effects"
    );
    assert_ne!(member_run_id, sibling_member_run_id);
    let (status, claim_replay) = serve.post_json_with_headers(
        &claim_route,
        &serde_json::json!({"action":"claim_work"}),
        &claim_headers,
    );
    assert_eq!(status, 200, "member claim replay: {claim_replay}");
    assert_eq!(claim_replay["event_id"], claimed["event_id"]);
    assert_eq!(claim_replay["replayed"], true);
    let (status, reused_claim_key) = serve.post_json_with_headers(
        &format!(
            "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/start?project={project_id}"
        ),
        &serde_json::json!({"action":"start_work"}),
        &claim_headers,
    );
    assert_eq!(status, 409, "same key changed command: {reused_claim_key}");

    // Seed a genuinely claimed canonical delivery so the Operator action is
    // proven against Store-live state, not an empty RoleView fixture.
    let trust_member_run = store
        .trust_member_runs(&space_id)
        .expect("trust MemberRuns")
        .into_iter()
        .find(|member_run| member_run.id == member_run_id)
        .expect("canonical MemberRun");
    let supervisor = match store
        .latest_team_supervisor_lease(run_id)
        .expect("Supervisor lease")
    {
        Some(lease) => lease,
        None => {
            let daemon = store
                .latest_node_daemon_lease(node_id)
                .expect("NodeDaemon lease")
                .expect("live NodeDaemon lease");
            store
                .acquire_team_supervisor_under_node_lease(
                    run_id,
                    node_id,
                    &daemon.daemon_id,
                    daemon.generation,
                    &space_id,
                    &project_id,
                    "role-action-test-supervisor",
                    std::process::id(),
                    "test://role-action-loop",
                    unix_ms(),
                    60_000,
                )
                .expect("acquire test Supervisor")
        }
    };
    let delivery_context = MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: ActorRef {
            kind: ActorKind::AgentMember,
            id: team.host_agent_id.clone(),
        },
        authority_actor: None,
        command_name: "work_delivery.create".into(),
        idempotency_key: "seed-store-live-delivery".into(),
        expected_version: 0,
        request_fingerprint: None,
    };
    let created_deliveries = store
        .create_trust_work_deliveries(
            &delivery_context,
            claimed["event_id"].as_str().expect("claim event id"),
            "work-store-live-1",
            2,
            &[member_run_id.to_string()],
            "2026-08-10T00:00:00Z",
        )
        .expect("create Store-live WorkDelivery");
    let delivery_id = created_deliveries.projection[0].id.clone();
    let claim_context = MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: supervisor.supervisor_id.clone(),
        },
        authority_actor: None,
        command_name: "work_delivery.claim".into(),
        idempotency_key: "claim-store-live-delivery".into(),
        expected_version: 0,
        request_fingerprint: None,
    };
    store
        .claim_trust_work_delivery(
            &claim_context,
            &delivery_id,
            DeliveryClaim {
                claim_id: "store-live-delivery-claim".into(),
                supervisor_generation: supervisor.generation,
                member_generation: trust_member_run.runtime_generation,
                claim_expires_at: "2099-01-01T00:00:00Z".into(),
            },
            2,
            "2026-08-10T00:00:01Z",
        )
        .expect("claim Store-live WorkDelivery");

    let operator_route = format!("/v1/views/operator/{node_id}?project={project_id}");
    let (status, operator_before_reconcile) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(
        status, 200,
        "Operator RoleView: {operator_before_reconcile}"
    );
    let (status, delegated_operator_view) = serve.get_json_with_headers(
        &operator_route,
        &[("X-AgentFirm-Token", DELEGATED_OPERATOR_TOKEN)],
    );
    assert_eq!(
        status, 403,
        "delegated Service authority must not receive executable Operator actions: {delegated_operator_view}"
    );
    assert!(operator_before_reconcile["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions
            .iter()
            .any(|action| action["kind"] == "reconcile_delivery"
                && action["target_ref"]["id"] == delivery_id
                && action["required_version"] == 1)));
    let reconcile_route = format!(
        "/v1/agentfirm/nodes/{node_id}/work-deliveries/{delivery_id}/reconcile?project={project_id}"
    );
    let canonical_before_reconcile = store
        .canonical_operations_for_space(&space_id)
        .expect("before operator rejection")
        .len();
    let wrong_operator_headers = [
        ("X-AgentFirm-Token", WRONG_OPERATOR_TOKEN),
        ("Idempotency-Key", "wrong-node-reconcile"),
        ("If-Match", "1"),
        ("X-AgentFirm-Confirm", "reconcile_delivery"),
    ];
    let (status, wrong_operator) = serve.post_json_with_headers(
        &reconcile_route,
        &serde_json::json!({"action":"reconcile_delivery","evidence_ref":"check:wrong-node"}),
        &wrong_operator_headers,
    );
    assert_eq!(status, 409, "wrong node operator: {wrong_operator}");
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("after operator rejection")
            .len(),
        canonical_before_reconcile,
        "wrong-machine Operator must have zero canonical side effects"
    );
    let delegated_operator_headers = [
        ("X-AgentFirm-Token", DELEGATED_OPERATOR_TOKEN),
        ("Idempotency-Key", "delegated-node-reconcile"),
        ("If-Match", "1"),
        ("X-AgentFirm-Confirm", "reconcile_delivery"),
    ];
    let (status, delegated_operator) = serve.post_json_with_headers(
        &reconcile_route,
        &serde_json::json!({"action":"reconcile_delivery","evidence_ref":"check:delegated-node"}),
        &delegated_operator_headers,
    );
    assert_eq!(
        status, 409,
        "delegated node authority: {delegated_operator}"
    );
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("after delegated operator rejection")
            .len(),
        canonical_before_reconcile,
        "Operator route requires the exact machine Service actor, not delegated authority"
    );
    let reconcile_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "reconcile-store-live-delivery"),
        ("If-Match", "1"),
        ("X-AgentFirm-Confirm", "reconcile_delivery"),
    ];
    let (status, reconciled) = serve.post_json_with_headers(
        &reconcile_route,
        &serde_json::json!({"action":"reconcile_delivery","evidence_ref":"check:operator-recovery"}),
        &reconcile_headers,
    );
    assert_eq!(status, 200, "Operator reconcile: {reconciled}");
    assert_eq!(reconciled["projection"]["status"], "failed");
    let (status, reconcile_replay) = serve.post_json_with_headers(
        &reconcile_route,
        &serde_json::json!({"action":"reconcile_delivery","evidence_ref":"check:operator-recovery"}),
        &reconcile_headers,
    );
    assert_eq!(status, 200, "Operator reconcile replay: {reconcile_replay}");
    assert_eq!(reconcile_replay["event_id"], reconciled["event_id"]);
    assert_eq!(reconcile_replay["replayed"], true);

    let submit_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/submit?project={project_id}"
    );
    let submit_headers = action_headers(MEMBER_TOKEN, "submit-store-live-1", "2");
    let (status, submitted) = serve.post_json_with_headers(
        &submit_route,
        &serde_json::json!({
            "action":"submit_work",
            "result_summary":"Store-live loop complete",
            "candidate_revision":"0123456789abcdef0123456789abcdef01234567",
            "check_refs":["check:role-action-loop"]
        }),
        &submit_headers,
    );
    assert_eq!(status, 200, "member submit: {submitted}");
    assert_eq!(submitted["projection"]["kind"], "result");
    assert_eq!(submitted["projection"]["work_revision"], 3);
    let (status, submit_replay) = serve.post_json_with_headers(
        &submit_route,
        &serde_json::json!({
            "action":"submit_work",
            "result_summary":"Store-live loop complete",
            "candidate_revision":"0123456789abcdef0123456789abcdef01234567",
            "check_refs":["check:role-action-loop"]
        }),
        &submit_headers,
    );
    assert_eq!(status, 200, "submit replay: {submit_replay}");
    assert_eq!(submit_replay["event_id"], submitted["event_id"]);
    assert_eq!(submit_replay["replayed"], true);
    assert_eq!(
        store
            .work_operations()
            .expect("submission is canonical-only")
            .len(),
        before + 2,
        "result submission must not create a second legacy Work transition"
    );
    let (status, review_view) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "review Host RoleView: {review_view}");
    assert!(review_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions
            .iter()
            .any(|action| action["kind"] == "accept_work" && action["required_version"] == 3)));
    let request_changes_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/request-changes?project={project_id}",
        team.id
    );
    let request_changes_headers = action_headers(TOKEN, "request-changes-store-live-1", "3");
    let request_changes_intent =
        serde_json::json!({"action":"request_changes","reason":"tighten exact replay evidence"});
    let (status, changes_requested) = serve.post_json_with_headers(
        &request_changes_route,
        &request_changes_intent,
        &request_changes_headers,
    );
    assert_eq!(status, 200, "request changes: {changes_requested}");
    assert_eq!(changes_requested["projection"]["version"], 4);
    let (status, changes_replay) = serve.post_json_with_headers(
        &request_changes_route,
        &request_changes_intent,
        &request_changes_headers,
    );
    assert_eq!(status, 200, "request changes replay: {changes_replay}");
    assert_eq!(changes_replay["event_id"], changes_requested["event_id"]);
    assert_eq!(changes_replay["replayed"], true);
    let revise_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/revise?project={project_id}",
        team.id
    );
    let revise_headers = action_headers(MEMBER_TOKEN, "revise-store-live-1", "4");
    let revise_intent = serde_json::json!({"action":"revise_work","result_summary":"Revised Store-live loop","candidate_revision":"1123456789abcdef0123456789abcdef01234567","check_refs":["check:role-action-revise"]});
    let (status, revised) =
        serve.post_json_with_headers(&revise_route, &revise_intent, &revise_headers);
    assert_eq!(status, 200, "member revise: {revised}");
    assert_eq!(revised["projection"]["work_revision"], 5);
    let (status, revise_replay) =
        serve.post_json_with_headers(&revise_route, &revise_intent, &revise_headers);
    assert_eq!(status, 200, "member revise replay: {revise_replay}");
    assert_eq!(revise_replay["event_id"], revised["event_id"]);
    assert_eq!(revise_replay["replayed"], true);
    let accept_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/accept?project={project_id}",
        team.id
    );
    let canonical_before_accept = store
        .canonical_operations_for_space(&space_id)
        .expect("before accept")
        .len();
    let no_confirm_headers = action_headers(TOKEN, "accept-no-confirm", "5");
    let (status, no_confirm) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &no_confirm_headers,
    );
    assert_eq!(status, 409, "missing confirmation: {no_confirm}");
    let member_accept_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "accept-member-spoof"),
        ("If-Match", "5"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, member_accept) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &member_accept_headers,
    );
    assert_eq!(status, 409, "Member authority spoof: {member_accept}");
    let stale_accept_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "accept-stale"),
        ("If-Match", "4"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, stale_accept) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &stale_accept_headers,
    );
    assert_eq!(status, 409, "stale accept: {stale_accept}");
    assert_eq!(
        store
            .canonical_operations_for_space(&space_id)
            .expect("rejected accepts")
            .len(),
        canonical_before_accept,
        "rejected critical actions must have zero canonical side effects"
    );
    let accept_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "accept-store-live-1"),
        ("If-Match", "5"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, accepted) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &accept_headers,
    );
    assert_eq!(status, 200, "Host accept: {accepted}");
    assert_eq!(accepted["projection"]["phase"], "closed");
    assert_eq!(accepted["projection"]["resolution"], "accepted");
    let (status, accept_replay) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &accept_headers,
    );
    assert_eq!(status, 200, "accept replay: {accept_replay}");
    assert_eq!(accept_replay["event_id"], accepted["event_id"]);
    assert_eq!(accept_replay["replayed"], true);
    assert_eq!(
        store.work_operations().expect("accept roll-up").len(),
        before + 3,
        "canonical accept must not fabricate a second legacy Work transition beyond request-changes"
    );
    assert!(store
        .canonical_operations_for_space(&space_id)
        .expect("canonical operations")
        .iter()
        .any(|operation| operation.event.aggregate_kind == "work"
            && operation.event.aggregate_id == "work-store-live-1"
            && operation.event.transition == "accepted"),
        "canonical acceptance and its delegation/gate/report roll-up must be one canonical operation");

    // Every mutable Work action exposed by the closed RoleAction matrix must
    // replay the original commit before consulting the now-advanced Work
    // revision. These sequences are deliberately table-driven so future
    // actions cannot silently regress to create-only idempotency coverage.
    let host_matrix_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-host-replay-matrix",
        "title":"Host replay matrix",
        "completion_criteria_markdown":"Every Host mutation replays exactly",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        &serve,
        &action_route,
        &host_matrix_work,
        &action_headers(TOKEN, "matrix-host-create", "0"),
        "host create",
    );
    let provider_run = store
        .member_runs()
        .expect("provider runtime projections")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("provider runtime for MemberRun");
    let mut failed_provider_run = provider_run.clone();
    failed_provider_run.status = MemberRunStatus::Failed;
    failed_provider_run.finished_at = Some("unix-ms:matrix-failed".into());
    store
        .compare_and_append_member_run(&provider_run, &failed_provider_run)
        .expect("record failed provider runtime generation");
    let mut successor_provider_run = provider_run.clone();
    successor_provider_run.runtime_generation += 1;
    successor_provider_run.status = MemberRunStatus::Idle;
    successor_provider_run.started_at = "unix-ms:matrix-successor".into();
    successor_provider_run.finished_at = None;
    let successor_run_id = successor_provider_run.id.clone();
    store
        .compare_and_advance_member_run_generation(&failed_provider_run, &successor_provider_run)
        .expect("append higher-generation replacement runtime");

    // Interrupt is a live-only semantic action, not a durable lifecycle
    // mutation. Prove malformed and stale HTTP requests fail at authorization
    // before provider control. Provider acknowledgement and the running-turn
    // precondition are exercised by provider-adapter journeys; synthesizing a
    // Running row here would let the independent NodeDaemon legitimately
    // settle it while this test compares store bytes.
    let interrupt_version = store
        .trust_member_runs(&space_id)
        .expect("Interrupt canonical MemberRun")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("Interrupt MemberRun")
        .version
        .to_string();
    let interrupt_route =
        format!("/v1/agentfirm/member-runs/{member_run_id}/interrupt?project={project_id}");
    for (key, body) in [
        (
            "matrix-member-interrupt-wrong-action",
            serde_json::json!({"action":"close_member_run"}),
        ),
        (
            "matrix-member-interrupt-empty-reason",
            serde_json::json!({"action":"interrupt_member_run","reason":"   "}),
        ),
    ] {
        let before = ledger_digest(serve.fixture_store_root());
        let (status, rejected) = serve.post_json_with_headers(
            &interrupt_route,
            &body,
            &action_headers(TOKEN, key, &interrupt_version),
        );
        assert_eq!(status, 409, "invalid Interrupt intent: {rejected}");
        assert_eq!(
            rejected["error"]["code"], "INVALID_STATE_TRANSITION",
            "invalid Interrupt must fail at the closed semantic boundary"
        );
        assert_eq!(
            ledger_digest(serve.fixture_store_root()),
            before,
            "invalid Interrupt intent changed durable state"
        );
    }
    let stale_interrupt_version = u64::MAX.to_string();
    let before_stale_interrupt = ledger_digest(serve.fixture_store_root());
    let (status, rejected_stale_interrupt) = serve.post_json_with_headers(
        &interrupt_route,
        &serde_json::json!({
            "action":"interrupt_member_run",
            "reason":"stop exactly the current provider turn"
        }),
        &action_headers(
            TOKEN,
            "matrix-member-interrupt-stale",
            &stale_interrupt_version,
        ),
    );
    assert_eq!(
        status, 409,
        "stale Interrupt must fail before provider control: {rejected_stale_interrupt}"
    );
    assert_eq!(
        rejected_stale_interrupt["error"]["code"], "VERSION_CONFLICT",
        "valid semantic body must still bind the exact current MemberRun revision"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_stale_interrupt,
        "stale Interrupt changed durable state"
    );
    let host_steps = [
        (
            "assign",
            "assign_work",
            "1",
            "matrix-host-assign",
            serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
            None,
        ),
        (
            "release",
            "release_work",
            "2",
            "matrix-host-release",
            serde_json::json!({"action":"release_work"}),
            None,
        ),
        (
            "assign",
            "assign_work",
            "3",
            "matrix-host-reassign",
            serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
            None,
        ),
        (
            "rebind",
            "rebind_work",
            "4",
            "matrix-host-rebind",
            serde_json::json!({"action":"rebind_work","member_run_id":successor_run_id}),
            None,
        ),
        (
            "cancel",
            "cancel_work",
            "5",
            "matrix-host-cancel",
            serde_json::json!({"action":"cancel_work","reason":"matrix complete"}),
            Some("cancel"),
        ),
    ];
    for (route_suffix, label, version, key, body, confirmation) in host_steps {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-host-replay-matrix/{route_suffix}?project={project_id}");
        let mut headers = vec![
            ("X-AgentFirm-Token", TOKEN),
            ("Idempotency-Key", key),
            ("If-Match", version),
        ];
        if let Some(confirmation) = confirmation {
            headers.push(("X-AgentFirm-Confirm", confirmation));
        }
        assert_exact_role_action_replay(&serve, &route, &body, &headers, label);
    }
    let member_claim_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-member-claim-replay",
        "title":"Member claim replay",
        "completion_criteria_markdown":"Team claim replays exactly",
        "claim_mode":"team_claim",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        &serve,
        &action_route,
        &member_claim_work,
        &action_headers(TOKEN, "matrix-member-claim-create", "0"),
        "member-claim create",
    );
    assert_exact_role_action_replay(
        &serve,
        &format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-claim-replay/claim?project={project_id}"),
        &serde_json::json!({"action":"claim_work"}),
        &action_headers(MEMBER_TOKEN, "matrix-member-claim", "1"),
        "claim_work",
    );
    for (route_suffix, label, version, key, body) in [
        (
            "block",
            "claimed block_work",
            "2",
            "matrix-claimed-block",
            serde_json::json!({"action":"block_work","reason":"claim-path blocker"}),
        ),
        (
            "resume",
            "claimed unblock_work",
            "3",
            "matrix-claimed-resume",
            serde_json::json!({"action":"unblock_work","resolution":"claim-path blocker resolved"}),
        ),
        (
            "submit",
            "claimed submit_work",
            "4",
            "matrix-claimed-submit",
            serde_json::json!({"action":"submit_work","result_summary":"claim path complete","candidate_revision":"3123456789abcdef0123456789abcdef01234567","check_refs":["check:claim-replay-matrix"]}),
        ),
    ] {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-claim-replay/{route_suffix}?project={project_id}");
        assert_exact_role_action_replay(
            &serve,
            &route,
            &body,
            &action_headers(MEMBER_TOKEN, key, version),
            label,
        );
    }

    let member_matrix_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-member-replay-matrix",
        "title":"Member replay matrix",
        "completion_criteria_markdown":"Every assigned Member mutation replays exactly",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        &serve,
        &action_route,
        &member_matrix_work,
        &action_headers(TOKEN, "matrix-member-create", "0"),
        "member-matrix create",
    );
    assert_exact_role_action_replay(
        &serve,
        &format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-replay-matrix/assign?project={project_id}"),
        &serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
        &action_headers(TOKEN, "matrix-member-assign", "1"),
        "member-matrix assign",
    );
    let member_steps = [
        (
            "start",
            "start_work",
            "2",
            "matrix-member-start",
            serde_json::json!({"action":"start_work"}),
        ),
        (
            "block",
            "block_work",
            "3",
            "matrix-member-block",
            serde_json::json!({"action":"block_work","reason":"deterministic matrix blocker"}),
        ),
        (
            "resume",
            "unblock_work",
            "4",
            "matrix-member-resume",
            serde_json::json!({"action":"unblock_work","resolution":"matrix blocker resolved"}),
        ),
        (
            "submit",
            "submit_work",
            "5",
            "matrix-member-submit",
            serde_json::json!({"action":"submit_work","result_summary":"member replay matrix complete","candidate_revision":"2123456789abcdef0123456789abcdef01234567","check_refs":["check:member-replay-matrix"]}),
        ),
    ];
    for (route_suffix, label, version, key, body) in member_steps {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-replay-matrix/{route_suffix}?project={project_id}");
        assert_exact_role_action_replay(
            &serve,
            &route,
            &body,
            &action_headers(MEMBER_TOKEN, key, version),
            label,
        );
    }

    // An idle member on an unreviewed provider tuple keeps its runtime
    // availability fact but must not be counted Ready, and the adapter review
    // state rides along as its own fact with remediation metadata.
    let review_team_view_route =
        format!("/v1/views/team-workspace/{}?project={project_id}", team.id);
    let (status, current_team_view) =
        serve.get_json_with_headers(&review_team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "current Team RoleView: {current_team_view}");
    let current_capability_admission = current_team_view["data"]["members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["current_member_run_ref"] == successor_provider_run.id)
        })
        .map(|member| member["provider_capability_admission"].clone())
        .expect("current member capability admission");
    let mut review_run = successor_provider_run.clone();
    review_run.runtime_generation += 1;
    review_run.started_at = "unix-ms:matrix-review-required".into();
    let review_native_session = review_run
        .native_session
        .as_mut()
        .expect("review fixture keeps the discovered native session");
    review_native_session.availability = harness_core::NativeSessionAvailability::Available;
    review_native_session.supports_resume = true;
    review_native_session.last_verified_at = Some("unix-ms:matrix-native-verified".into());
    let mut review_profile = review_run
        .provider_profile
        .clone()
        .expect("provider profile snapshot");
    review_profile.provider_version = Some("0.146.0".into());
    review_profile.compatibility_status = ProviderCompatibilityStatus::ReviewRequired;
    review_profile.compatibility_note = Some(
        "Installed provider version has not been reviewed against this adapter contract; \
         regenerate protocol schemas and run provider acceptance before promotion."
            .into(),
    );
    review_run.provider_profile = Some(review_profile);
    store
        .compare_and_advance_member_run_generation(&successor_provider_run, &review_run)
        .expect("append review-required runtime generation");
    let (status, review_team_view) =
        serve.get_json_with_headers(&review_team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 200,
        "review-required Team RoleView: {review_team_view}"
    );
    let review_member = review_team_view["data"]["members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["current_member_run_ref"] == review_run.id)
        })
        .cloned()
        .expect("review-required member row");
    assert_eq!(review_member["capacity"], "available");
    assert_eq!(
        review_member["provider_compatibility"], "review_required",
        "unreviewed exact tuple renders review_required as a separate fact"
    );
    assert_eq!(review_member["provider_version"], "0.146.0");
    assert_eq!(
        review_member["provider_capability_admission"], current_capability_admission,
        "changing only source/version review must not rewrite executable capability admission"
    );
    assert!(review_member["provider_compatibility_note"]
        .as_str()
        .is_some_and(|note| note.contains("run provider acceptance")));
    let ready_members = review_team_view["data"]["pressure_summary"]["ready_members"]
        .as_u64()
        .expect("ready_members");
    let review_members = review_team_view["data"]["members"]
        .as_array()
        .expect("members");
    let honestly_ready_members = review_members
        .iter()
        .filter(|member| {
            member["capacity"] == "available"
                && member["provider_compatibility"] == "current"
                && member["provider_capability_admission"] == "active"
        })
        .count() as u64;
    let review_blocked_available = review_members
        .iter()
        .filter(|member| {
            member["capacity"] == "available"
                && matches!(
                    member["provider_compatibility"].as_str(),
                    Some("review_required") | Some("incompatible") | Some("unavailable")
                )
        })
        .count() as u64;
    assert!(
        review_blocked_available >= 1,
        "the review-required member is present and still runtime-available"
    );
    assert_eq!(
        ready_members, honestly_ready_members,
        "Ready requires both a current exact provider tuple and active verified core capabilities"
    );
    // The public lifecycle surface keeps Close/Reopen and Resume distinct.
    // Closing an Active member advertises Reopen only; a caller cannot invoke
    // Resume against that Closed+Stopped projection as an alias for Reopen.
    let (status, lifecycle_before_close) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "lifecycle RoleView: {lifecycle_before_close}");
    let close_action = lifecycle_before_close["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action["kind"] == "close_member_run" && action["target_ref"]["id"] == member_run_id
            })
        })
        .expect("Active MemberRun close action");
    let lifecycle_route = |operation: &str| {
        format!("/v1/agentfirm/member-runs/{member_run_id}/{operation}?project={project_id}")
    };
    let stale_close_version = u64::MAX.to_string();
    let stale_close_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "matrix-member-close-stale"),
        ("If-Match", stale_close_version.as_str()),
        ("X-AgentFirm-Confirm", "close_member_run"),
    ];
    let before_stale_close = ledger_digest(serve.fixture_store_root());
    let (status, rejected_close) = serve.post_json_with_headers(
        &lifecycle_route("close"),
        &serde_json::json!({"action":"close_member_run"}),
        &stale_close_headers,
    );
    assert_eq!(
        status, 409,
        "a stale Close must fail before provider control: {rejected_close}"
    );
    assert_eq!(
        rejected_close["error"]["code"], "VERSION_CONFLICT",
        "Close remains exact-CAS bound"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_stale_close,
        "stale Close must have byte-zero durable side effects"
    );

    // The rest of this test covers RoleView lifecycle projection, not a live
    // provider control journey. Seed the closed state through the canonical
    // Store transition explicitly rather than pretending this HTTP fixture
    // closed a physical handle.
    let seeded_closed = store
        .transition_current_team_member_lifecycle(
            &MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: team.host_agent_id.clone(),
                },
                authority_actor: None,
                command_name: "test_fixture.member_run.close".into(),
                idempotency_key: "seed-matrix-member-closed".into(),
                expected_version: close_action["required_version"]
                    .as_u64()
                    .expect("close action version"),
                request_fingerprint: None,
            },
            member_run_id,
            CurrentTeamMemberLifecycleTransition::Close,
            "unix-ms:matrix-member-closed-fixture",
        )
        .expect("seed canonical closed lifecycle fixture");
    assert_eq!(
        seeded_closed.runtime_projection.coordination_status,
        MemberCoordinationStatus::Closed
    );
    assert_eq!(
        seeded_closed.runtime_projection.status,
        MemberRunStatus::Stopped
    );
    let closed_version = seeded_closed.canonical.projection.version.to_string();
    let (status, lifecycle_closed) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "closed lifecycle RoleView: {lifecycle_closed}");
    let closed_actions = lifecycle_closed["allowed_actions"]
        .as_array()
        .expect("closed actions");
    assert!(closed_actions.iter().any(|action| {
        action["kind"] == "reopen_member_run" && action["target_ref"]["id"] == member_run_id
    }));
    assert!(!closed_actions.iter().any(|action| {
        action["kind"] == "resume_native_session" && action["target_ref"]["id"] == member_run_id
    }));
    let before_closed_resume = ledger_digest(serve.fixture_store_root());
    let (status, rejected_generic_closed_resume) = serve.post_json_with_headers(
        &format!("/v1/member-runs/{member_run_id}/resume-native-session?project={project_id}"),
        &serde_json::json!({
            "command":"resume_native_session",
            "member_run_id":member_run_id,
            "updated_at":"unix-ms:generic-closed-resume"
        }),
        &action_headers(
            TOKEN,
            "matrix-member-invalid-generic-closed-resume",
            &closed_version,
        ),
    );
    assert_eq!(
        status, 409,
        "generic HTTP Closed Resume must fail: {rejected_generic_closed_resume}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_closed_resume,
        "generic HTTP Closed Resume must have byte-zero durable side effects"
    );
    let (status, rejected_closed_resume) = serve.post_json_with_headers(
        &lifecycle_route("resume-native-session"),
        &serde_json::json!({"action":"resume_native_session"}),
        &action_headers(
            TOKEN,
            "matrix-member-invalid-closed-resume",
            &closed_version,
        ),
    );
    assert_eq!(
        status, 409,
        "Closed Resume must fail: {rejected_closed_resume}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_closed_resume,
        "Closed Resume must have byte-zero durable side effects"
    );
    let reopened = assert_exact_role_action_replay(
        &serve,
        &lifecycle_route("reopen"),
        &serde_json::json!({"action":"reopen_member_run"}),
        &action_headers(TOKEN, "matrix-member-reopen", &closed_version),
        "reopen Closed MemberRun",
    );
    assert_eq!(reopened["projection"]["coordination_status"], "active");
    assert_eq!(reopened["projection"]["runtime_status"], "queued");

    // Historical/corrupt stores can contain two active generations for one
    // AgentMember in the same TeamRun. Reads must never choose one
    // arbitrarily: MemberWorkbench fails closed and Host loses all mutations
    // until the identity conflict is reconciled.
    let mut duplicate_run = trust_member_run.clone();
    duplicate_run.id = "member-run-duplicate-active".into();
    if let Some(session) = duplicate_run.native_session.as_mut() {
        session.native_session_id = "duplicate-active-session".into();
    }
    store
        .legacy_import_create_trust_member_run_projection(
            &MutationContext {
                execution_space_id: space_id.clone(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: team.host_agent_id.clone(),
                },
                authority_actor: None,
                command_name: "member_run.create".into(),
                idempotency_key: "seed-duplicate-active-run".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            duplicate_run,
        )
        .expect("seed duplicate active MemberRun");
    let (status, duplicate_member) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 409, "duplicate MemberRun: {duplicate_member}");
    assert_eq!(duplicate_member["error"]["code"], "IDENTITY_CONFLICT");
    let (status, conflicted_host) =
        serve.get_json_with_headers(&view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "conflicted Host RoleView: {conflicted_host}");
    assert_eq!(conflicted_host["allowed_actions"], serde_json::json!([]));
    assert!(conflicted_host["attention"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item["reason_code"] == "multiple_active_member_runs")));

    let global_route = format!("/v1/views/global-work?project={project_id}");
    let (status, global) =
        serve.get_json_with_headers(&global_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Global Work RoleView: {global}");
    assert_eq!(global["view_kind"], "global_work");
    assert!(global["data"]["items"].as_array().is_some_and(|items| items
        .iter()
        .any(|work| work["work_id"] == "work-store-live-1")));
    let snapshot_vector = global["data"]["page"]["snapshot_vector"]
        .as_array()
        .expect("snapshot vector");
    assert_eq!(
        snapshot_vector.len(),
        2,
        "Global Work cursor must bind every space"
    );
    assert!(snapshot_vector
        .iter()
        .any(|point| point["execution_space_id"] == space_id));
    assert!(snapshot_vector
        .iter()
        .any(|point| point["execution_space_id"] == "role-action-empty-space"));
    let team_view_route = format!("/v1/views/team-workspace/{}?project={project_id}", team.id);
    let (status, team_view) =
        serve.get_json_with_headers(&team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Team RoleView: {team_view}");
    assert!(team_view["data"]["works"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    assert_eq!(
        team_view["data"]["team"]["display_name"],
        "Role action team"
    );
    assert_eq!(team_view["data"]["team"]["host_agent_id"], host_id);
    assert_eq!(team_view["data"]["team"]["viewer_role"], "host");
    assert_eq!(team_view["data"]["team"]["latest_run"]["id"], run_id);
    let projected_work = team_view["data"]["works"]
        .as_array()
        .and_then(|works| {
            works
                .iter()
                .find(|work| work["work_id"] == "work-store-live-1")
        })
        .expect("projected Work");
    assert_eq!(projected_work["title"], "Close the local product loop");
    assert_eq!(projected_work["claim_mode"], "team_claim");
    assert!(projected_work["eligible_member_ids"].is_array());
    assert!(projected_work["artifact_refs"].is_array());
    assert!(projected_work["latest_event"].is_object());
    // DOC-106: every RoleView reads the one Work/WorkOperation authority, so
    // the same Work id carries the identical revision everywhere.
    let global_work = global["data"]["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|work| work["work_id"] == "work-store-live-1")
        })
        .expect("Global Work row");
    assert_eq!(
        global_work["work_revision"], projected_work["work_revision"],
        "Global and Team projections must carry the identical Work revision"
    );
    assert_eq!(
        global_work["accountable_team_id"],
        serde_json::json!(team.id)
    );
    assert_eq!(
        projected_work["accountable_team_id"],
        global_work["accountable_team_id"]
    );
    assert!(projected_work["assignee_ref"].is_object());
    assert!(team_view["data"]["members"]
        .as_array()
        .is_some_and(|members| members.iter().all(|member| {
            member["display_name"].is_string()
                && member["active_work_count"].is_u64()
                && member["queued_work_count"].is_u64()
                && member["review_work_count"].is_u64()
                && member["blocked_work_count"].is_u64()
        })));
    assert!(team_view["data"]["messages"]
        .as_array()
        .is_some_and(|messages| messages
            .iter()
            .all(|message| { message["body"].is_string() && message["deliveries"].is_array() })));
    assert!(team_view["data"]["activity"].is_array());
    assert!(team_view["data"]["activity_truncated"].is_boolean());
    assert_eq!(
        team_view["data"]["pressure_summary"]["total_members"].as_u64(),
        team_view["data"]["members"]
            .as_array()
            .map(|members| members.len() as u64)
    );
    assert!(team_view["data"]["pressure_summary"]["ready_work"].is_u64());
    let (status, operator) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator RoleView: {operator}");
    assert_eq!(operator["data"]["node"]["node_id"], node_id);
    assert!(operator["data"]["node"]["node_revision"]
        .as_u64()
        .is_some_and(|v| v >= 1));

    let (status, cross_team_denied) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 403,
        "Host must not read MemberWorkbench: {cross_team_denied}"
    );
}

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
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start: {started}");

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
}

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
            "members":[{"agent_member_id":member_id,"name":"member","role":"builder","provider":"codex"}]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id");
    let member_run_id = created_run["result"]["member_runs"][0]["id"]
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
    for identity in [host_id, member_id] {
        let current = sessions
            .iter()
            .filter(|session| {
                session.agent_member_id == identity
                    && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
            })
            .collect::<Vec<_>>();
        assert_eq!(current.len(), 1, "one current session for {identity}");
        assert_eq!(current[0].node_daemon_id, lease.daemon_id);
        assert_eq!(current[0].node_daemon_generation, lease.generation);
    }

    let host_session = sessions
        .iter()
        .find(|session| session.agent_member_id == host_id)
        .expect("Host AgentSession");
    let space_store_root = home.spaces_dir().join(&space_id);
    let before_hostile_runtime = ledger_digest(&space_store_root);
    let hostile_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "hostile-sibling-runtime-control"),
        ("If-Match", "0"),
    ];
    let (status, hostile_runtime) = serve.post_json_with_headers(
        "/v1/agentfirm/runtime-commands",
        &serde_json::json!({
            "command":"stop_session",
            "expires_unix_ms":unix_ms()+30_000,
            "payload":{
                "session_id":host_session.id,
                "session_generation":host_session.runtime_generation
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

    let member_session = sessions
        .iter()
        .find(|session| session.agent_member_id == member_id)
        .expect("Member AgentSession");
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
    let (status, resolved) =
        serve.post_json_with_headers(&recovery_route, &recovery_intent, &recovery_headers);
    assert_eq!(status, 200, "resolve RecoveryRequired: {resolved}");
    assert_eq!(resolved["projection"]["status"], "failed");
    assert_eq!(resolved["projection"]["effect_certainty"], "not_applied");
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
    let (status, host_message) = serve.post_json_with_headers(
        &host_route,
        &serde_json::json!({
            "action":"send_message",
            "recipient_ids":[member_id],
            "body":"Host to Member canonical message",
            "response_required":true
        }),
        &action_headers(TOKEN, "host-member-canonical", &host_version),
    );
    assert_eq!(status, 200, "Host message: {host_message}");
    let host_message_id = host_message["projection"]["id"].as_str().unwrap();

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
    let (status, member_message) = serve.post_json_with_headers(
        &format!("/v1/agentfirm/team-runs/{run_id}/messages/request-decision?project={project_id}"),
        &serde_json::json!({
            "action":"request_decision",
            "body":"Member to Host canonical decision request"
        }),
        &action_headers(MEMBER_TOKEN, "member-host-canonical", &member_version),
    );
    assert_eq!(status, 200, "Member message: {member_message}");
    let member_message_id = member_message["projection"]["id"].as_str().unwrap();
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
    let (status, host_reply) = serve.post_json_with_headers(
        &format!("/v1/agentfirm/team-runs/{run_id}/messages/reply?project={project_id}"),
        &serde_json::json!({
            "action":"reply_message",
            "recipient_ids":[member_id],
            "body":"Host resolved the canonical decision request",
            "correlation_id":correlation_id,
            "causation_id":member_message_id
        }),
        &action_headers(TOKEN, "host-member-canonical-reply", &reply_version),
    );
    assert_eq!(status, 200, "Host reply: {host_reply}");
    let host_reply_id = host_reply["projection"]["id"].as_str().unwrap();
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
        assert!(
            message.sender_session_id.is_some(),
            "agent-authored Message must freeze the exact sender session: {message:?}"
        );
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
}

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
            "members":[{"agent_member_id":member_id,"name":"member","role":"builder","provider":"codex"}]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id")
        .to_string();
    let member_run_id = created_run["result"]["member_runs"][0]["id"]
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
    assert_eq!(host_projection["disabled_reason"], serde_json::Value::Null);
    assert!(host_projection["agent_session_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("agent-session:")));
    assert_eq!(
        host_projection["episodes"][0]["provider_turn_id"],
        "turn-host-1"
    );
    let serialized_host_projection =
        serde_json::to_string(host_projection).expect("projection JSON");
    assert!(serialized_host_projection.contains("display-safe host observation"));
    assert!(!serialized_host_projection.contains("raw-chain-of-thought-must-not-appear"));
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

    // A TeamRun without a bound Host thread is honestly unbound.
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
        "unbound"
    );
    assert_eq!(
        unbound_workspace["data"]["session_event_projection"]["disabled_reason"],
        "No provider-native Session is bound to the selected Host run."
    );
}

#[test]
fn role_views_require_local_capability_and_gets_are_store_pure() {
    let home = TempHome::new("role-views-http");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let credentials = serde_json::json!([{
        "token": TOKEN,
        "actor": {"kind":"human","id":"local-operator"},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let route = format!("/v1/views/global-work?project={project_id}");
    let (status, denied) = serve.get_json(&route);
    assert_eq!(status, 401, "unauthenticated RoleView: {denied}");
    assert_eq!(denied["error"]["code"], "NOT_AUTHORIZED");

    let before = ledger_digest(serve.fixture_store_root());
    let (status, global) = serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Global Work RoleView: {global}");
    assert_eq!(global["view_kind"], "global_work");
    assert_eq!(global["schema_version"], "agentfirm.role_views.v1");
    assert_eq!(global["data"]["items"], serde_json::json!([]));
    assert_eq!(
        global["data"]["pending_migration_work_ids"],
        serde_json::json!([])
    );
    assert_eq!(
        global["data"]["page"]["next_cursor"],
        serde_json::Value::Null
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before,
        "GET changed canonical ledgers"
    );

    for path in [
        "/v1/views/team-workspace/missing",
        "/v1/views/host-console/missing",
        "/v1/views/agent-workspace/missing",
        "/v1/views/member-workbench/missing",
        "/v1/views/operator/missing",
        // Retired by the Global Work cutover (DOC-106): the Company Work view
        // name no longer resolves.
        "/v1/views/company-work",
    ] {
        let route = format!("{path}?project={project_id}");
        let (status, body) = serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", TOKEN)]);
        assert_eq!(status, 404, "{path}: {body}");
    }
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before,
        "404 GETs changed canonical ledgers"
    );
}

#[test]
fn operator_remote_fabric_never_invents_green_health_from_local_journal() {
    let home = TempHome::new("operator-remote-fabric-health");
    let root = home.base().join("project");
    std::fs::create_dir_all(&root).expect("project root");
    let initialized = run_firm(&home, &root, &["init"]);
    assert!(initialized.status.success(), "init failed: {initialized:?}");
    let project_id = current_project_id(&home);
    let node: serde_json::Value =
        serde_json::from_slice(&run_firm(&home, &root, &["node", "init"]).stdout)
            .expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");

    let layout =
        harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(home.firm_home())
            .expect("Remote Fabric layout");
    let local = layout
        .open_node_local("company-test", node_id)
        .expect("Node-local journal");
    local
        .bind_gateway_session(&harness_fabric::transport::FabricSessionFence {
            company_id: "company-test".into(),
            node_id: node_id.into(),
            gateway_generation: 1,
            node_daemon_id: format!("node-daemon:{node_id}"),
            node_daemon_generation: 1,
            control_plane_generation: 1,
        })
        .expect("local journal has an observed session");

    let credentials = serde_json::json!([{
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
        "authority_actors": []
    }])
    .to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        &root,
        &[],
        &[("AGENTFIRM_HTTP_CREDENTIALS_JSON", credentials.as_str())],
    );
    let route = format!("/v1/views/operator/{node_id}?project={project_id}&company=company-test");
    let (status, operator) =
        serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator view: {operator}");
    let fabric = &operator["data"]["remote_fabric"];
    assert_eq!(fabric["state"], "unknown");
    assert_eq!(fabric["control_plane_online"], serde_json::Value::Null);
    assert_eq!(fabric["control_plane_metrics"], serde_json::Value::Null);
    assert!(fabric["reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("not health truth")));
}

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

    let store = HarnessStore::new(home.spaces_dir().join(&space_id))
        .with_provider_compatibility_scope(&project_id, format!("execution-space:{space_id}"));
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
