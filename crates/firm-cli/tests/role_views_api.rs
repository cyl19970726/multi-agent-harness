//! HTTP boundary coverage for the Wave 4B local RoleViews.

mod fake_provider;
mod firm_env;

#[path = "role_views_api/action_matrix_and_projection.rs"]
mod action_matrix_and_projection;
#[path = "role_views_api/authorization_and_store_purity.rs"]
mod authorization_and_store_purity;
#[path = "role_views_api/canonical_team_message.rs"]
mod canonical_team_message;
#[path = "role_views_api/daemon_admission.rs"]
mod daemon_admission;
#[path = "role_views_api/delivery_projection.rs"]
mod delivery_projection;
#[path = "role_views_api/exact_self_session.rs"]
mod exact_self_session;
#[path = "role_views_api/remote_fabric_health.rs"]
mod remote_fabric_health;
#[path = "role_views_api/standalone_codex_session.rs"]
mod standalone_codex_session;

use action_matrix_and_projection::{
    assert_action_matrix_and_final_projections, ActionMatrixContext,
};
use firm_env::{
    collect_named_sse_data, create_canonical_agent_member, current_project_id, current_space_id,
    run_firm, ServeHandle, TempHome,
};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus,
    MutationContext, NativeSessionAvailability, NativeSessionRef, PermissionCeiling,
    RuntimeActivity, RuntimeCommandBinding, RuntimeDispatchMode, RuntimeDriverRef,
    RuntimeResidency,
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
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("role-action-codex-bin"));
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
        ],
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
                workspace_cwd: None,
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
                    runtime_residency: RuntimeResidency::Detached,
                    activity: RuntimeActivity::Idle,
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
        "current_session",
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
        "effective_permission_ceiling",
        "resolved_workspace_cwd",
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
    let current_session = &member_self_workspace["data"]["current_session"];
    assert!(current_session["agent_session_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("agent-session:")));
    assert_eq!(current_session["provider"], "codex");
    assert_eq!(
        member_self_workspace["data"]["configuration"]["effective_permission_ceiling"],
        current_session["effective_permission_ceiling"]
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
    let (status, sibling_local_operator) = serve.get_json_with_headers(
        &member_agent_workspace_route,
        &[("X-AgentFirm-Token", SIBLING_MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "loopback sibling context gets only the local Operator read projection: {sibling_local_operator}"
    );
    assert_eq!(
        sibling_local_operator["data"]["projection_scope"],
        "host_member_public"
    );
    assert_eq!(
        sibling_local_operator["allowed_actions"],
        serde_json::json!([]),
        "local Operator read must not borrow sibling mutation authority"
    );
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
    let (status, member_local_operator_host) = serve.get_json_with_headers(
        &host_agent_workspace_route,
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 200,
        "loopback Member context gets only the local Operator Host projection: {member_local_operator_host}"
    );
    assert_eq!(
        member_local_operator_host["data"]["projection_scope"],
        "host_member_public"
    );
    assert_eq!(
        member_local_operator_host["allowed_actions"],
        serde_json::json!([]),
        "local Operator Host read must not borrow Host mutation authority"
    );
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

    let operator_route = format!("/v1/views/operator/{node_id}?project={project_id}");
    assert_action_matrix_and_final_projections(ActionMatrixContext {
        serve: &serve,
        store: &store,
        space_id: &space_id,
        project_id: &project_id,
        run_id,
        worker_id,
        member_run_id,
        action_route: &action_route,
        view_route: &view_route,
        team: &team,
        host_id,
        node_id,
        operator_route: &operator_route,
        member_view_route: &member_view_route,
    });
}
