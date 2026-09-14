#[path = "role_views_api/action_matrix_and_projection.rs"]
mod action_matrix_and_projection;
#[path = "role_views_api/agent_workspace_read_scope.rs"]
mod agent_workspace_read_scope;
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
mod fake_provider;
mod firm_env;
#[path = "role_views_api/provider_received_work_attempt.rs"]
mod provider_received_work_attempt;
#[path = "role_views_api/remote_fabric_health.rs"]
mod remote_fabric_health;
#[path = "role_views_api/role_action_fixture.rs"]
mod role_action_fixture;
#[path = "role_views_api/standalone_codex_session.rs"]
mod standalone_codex_session;
#[path = "role_views_api/store_reads.rs"]
mod store_reads;
#[path = "role_views_api/submission_evidence_refusal.rs"]
mod submission_evidence_refusal;
#[path = "role_views_api/submission_revision_loop.rs"]
mod submission_revision_loop;

use action_matrix_and_projection::{
    assert_action_matrix_and_final_projections, ActionMatrixContext,
};
use firm_env::{
    collect_named_sse_data, create_canonical_agent_member, current_project_id, current_space_id,
    run_firm, run_firm_with_env, unix_ms, ServeHandle, TempHome,
};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus,
    MutationContext, NativeSessionAvailability, NativeSessionRef, PermissionCeiling,
    RuntimeActivity, RuntimeCommandBinding, RuntimeDispatchMode, RuntimeDriverRef,
    RuntimeResidency, WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};
use harness_core::ExecutionSpaceId;
use harness_core::{
    ExecutionNode, ExecutionNodeStatus, MemberCoordinationStatus, MemberRunStatus,
    NodeProjectRegistration, NodeProjectRegistrationStatus, ProviderCompatibilityStatus,
};
use harness_store::{CurrentTeamMemberLifecycleTransition, HarnessStore};
use provider_received_work_attempt::{
    admit_provider_received_work_attempt, assert_released_provider_received_attempt,
    ProviderReceivedWorkAttempt, ProviderReceivedWorkAttemptInput,
};
use store_reads::{legacy_ledger_rows, work_journal};

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
    let role_action_fixture::RoleActionFixture {
        home,
        root,
        project_id,
        space_id,
        node_id,
        mission_id,
        store,
        team,
        credentials,
        other_project_id,
    } = role_action_fixture::seed_role_action_fixture();
    let node_id = node_id.as_str();
    let host_id = role_action_fixture::HOST_ID;
    let worker_id = role_action_fixture::WORKER_ID;
    let sibling_worker_id = role_action_fixture::SIBLING_WORKER_ID;
    let worker_native_session_id = role_action_fixture::WORKER_NATIVE_SESSION_ID;
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

    let before = legacy_ledger_rows(&store).len();
    let journal_before = work_journal(&store).len();
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
    assert_eq!(legacy_ledger_rows(&store).len(), before);

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
    assert_eq!(legacy_ledger_rows(&store).len(), before);

    let headers = action_headers(TOKEN, "create-store-live-1", "0");
    let (status, created) = serve.post_json_with_headers(&action_route, &intent, &headers);
    assert_eq!(status, 200, "authenticated create: {created}");
    assert_eq!(created["projection"]["id"], "work-store-live-1");
    assert_eq!(created["replayed"], false);
    let (status, replay) = serve.post_json_with_headers(&action_route, &intent, &headers);
    assert_eq!(status, 200, "idempotent replay: {replay}");
    assert_eq!(replay["event_id"], created["event_id"]);
    assert_eq!(replay["replayed"], true);

    let worker_membership = store
        .fabric_team_memberships(&space_id)
        .expect("TeamMemberships for Work assignment")
        .into_iter()
        .find(|membership| membership.team_id == team.id && membership.agent_member_id == worker_id)
        .expect("exact worker TeamMembership");
    let assign_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/assign?project={project_id}"
    );
    let (status, assigned) = serve.post_json_with_headers(
        &assign_route,
        &serde_json::json!({
            "action":"assign_work",
            "membership_id":worker_membership.id
        }),
        &action_headers(TOKEN, "assign-store-live-1", "1"),
    );
    assert_eq!(status, 200, "canonical membership assignment: {assigned}");
    assert_eq!(assigned["projection"]["version"], 2);
    let assigned_work = store
        .latest_works()
        .expect("Works after assignment")
        .into_iter()
        .find(|work| work.id == "work-store-live-1")
        .expect("assigned Work");
    let worker_session = store
        .fabric_agent_sessions(&space_id)
        .expect("AgentSessions for Work admission")
        .into_iter()
        .find(|session| session.id == "agent-session:role-view-owner:1")
        .expect("exact worker AgentSession");
    let first_attempt = admit_provider_received_work_attempt(ProviderReceivedWorkAttemptInput {
        store: &store,
        space_id: &space_id,
        node_id,
        daemon: &daemon,
        member_run_id,
        work: &assigned_work,
        team: &team,
        membership: &worker_membership,
        worker_id,
        session: &worker_session,
        binding_generation: 1,
    });
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
    let projected_work = refreshed["data"]["all_works"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|work| work["work_id"] == "work-store-live-1")
        })
        .expect("canonical Work projection");
    assert_eq!(
        projected_work["current_member_run_ref"], member_run_id,
        "canonical runtime projection: {projected_work}"
    );
    assert_eq!(
        projected_work["runtime_summary"]["work_execution_binding_id"],
        "work-binding:work-store-live-1:1"
    );
    assert_eq!(
        projected_work["runtime_summary"]["agent_session_id"],
        worker_session.id
    );
    assert!(refreshed["data"]["work_queues"]["ready"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    assert!(refreshed["data"]["work_queues"]["unassigned"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .all(|work| work["work_id"] != "work-store-live-1")));
    assert!(refreshed["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions
            .iter()
            .all(|action| action["required_version"].is_u64())));
    assert!(refreshed["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().all(|action| action["kind"] != "rebind_work")));
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

    let (status, retired_native_activity) = serve.get_json_with_headers(
        &format!("/v1/member-runs/{member_run_id}/native-activity?project={project_id}"),
        &[("X-AgentFirm-Token", MEMBER_TOKEN)],
    );
    assert_eq!(
        status, 410,
        "run-addressed native history reader must remain retired: {retired_native_activity}"
    );
    agent_workspace_read_scope::assert_agent_workspace_read_scope(
        agent_workspace_read_scope::AgentWorkspaceReadScopeContext {
            serve: &serve,
            home: &home,
            root: &root,
            store: &store,
            space_id: &space_id,
            project_id: &project_id,
            other_project_id: &other_project_id,
            run_id,
            worker_id,
            sibling_worker_id,
            member_run_id,
            team: &team,
        },
    );
    let member_view_route =
        format!("/v1/views/member-workbench/{member_run_id}?project={project_id}");
    let (status, member_view) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "Member RoleView: {member_view}");
    assert!(member_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["kind"] == "start_work")));
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
    let start_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/start?project={project_id}"
    );
    let start_headers = action_headers(MEMBER_TOKEN, "start-store-live-1", "2");
    let (status, started) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"start_work"}),
        &start_headers,
    );
    assert_eq!(status, 200, "member start: {started}");
    assert_eq!(started["projection"]["phase"], "active");
    assert_eq!(
        started["projection"]["active_member_run_id"],
        serde_json::Value::Null,
        "canonical stable responsibility must not copy runtime identity into Work"
    );
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
            &action_headers(SIBLING_MEMBER_TOKEN, key, "3"),
        );
        assert_eq!(status, 409, "sibling {operation} spoof: {rejected}");
    }
    let sibling_submit_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/submit?project={project_id}"
    );
    let (status, sibling_submit) = serve.post_json_with_headers(
        &sibling_submit_route,
        &serde_json::json!({"action":"submit_work","result_summary":"spoofed result","candidate_revision":"abcdef0123456789","check_refs":["check:spoof"]}),
        &action_headers(SIBLING_MEMBER_TOKEN, "sibling-submit-spoof", "3"),
    );
    assert_eq!(status, 409, "sibling submit spoof: {sibling_submit}");
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_sibling_attempts,
        "foreign Work mutations must have zero durable side effects"
    );
    let works_before_linked_messages = store.latest_works().expect("Works before linked Messages");
    let work_deliveries_before_sibling_link = store
        .fabric_work_deliveries(&project_id)
        .expect("Work deliveries before sibling Work link");
    let sibling_linked_message_headers = action_headers(
        SIBLING_MEMBER_TOKEN,
        "sibling-linked-message",
        &team_revision,
    );
    let linked_message_route =
        format!("/v1/agentfirm/team-runs/{run_id}/messages/send?project={project_id}");
    let (status, sibling_linked_message) = serve.post_json_with_headers(
        &linked_message_route,
        &serde_json::json!({
            "action":"send_message",
            "recipient_ids":[host_id],
            "body":"A sibling may use another member's Work as Message context",
            "work_id":"work-store-live-1",
            "response_required":false
        }),
        &sibling_linked_message_headers,
    );
    assert_eq!(
        status, 200,
        "sibling linked Message: {sibling_linked_message}"
    );
    let messages_after_sibling_link = store
        .fabric_messages(&project_id)
        .expect("Messages after sibling Work link");
    let linked_message_id = sibling_linked_message["projection"]["id"]
        .as_str()
        .expect("linked Message response carries its canonical id");
    assert!(messages_after_sibling_link.iter().any(|message| {
        message.id == linked_message_id
            && message.work_id.as_deref() == Some("work-store-live-1")
            && message.sender_agent_member_id.as_deref() == Some(sibling_worker_id)
    }));
    let message_deliveries_after_sibling_link = store
        .fabric_message_deliveries(&project_id)
        .expect("Message deliveries after sibling Work link");
    assert!(message_deliveries_after_sibling_link
        .iter()
        .any(|delivery| delivery.message_id == linked_message_id));
    assert_eq!(
        store
            .fabric_work_deliveries(&project_id)
            .expect("Work deliveries after rejected sibling Work link"),
        work_deliveries_before_sibling_link,
        "peer Work-linked Message must not mutate Work delivery authority"
    );
    assert_eq!(
        store
            .latest_works()
            .expect("Works after accepted sibling Work link"),
        works_before_linked_messages,
        "peer Work-linked Message must not mutate Work"
    );
    assert_eq!(
        store.latest_works().expect("Works after linked Messages"),
        works_before_linked_messages,
        "Message context must not mutate the linked Work"
    );
    let after_linked_messages = ledger_digest(serve.fixture_store_root());
    let (status, unknown_linked_message) = serve.post_json_with_headers(
        &linked_message_route,
        &serde_json::json!({
            "action":"send_message",
            "recipient_ids":[host_id],
            "body":"unknown Work linkage",
            "work_id":"missing-work",
            "response_required":false
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
        after_linked_messages,
        "unknown Work linkage must have zero durable side effects"
    );
    assert_ne!(member_run_id, sibling_member_run_id);
    let (status, start_replay) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"start_work"}),
        &start_headers,
    );
    assert_eq!(status, 200, "member start replay: {start_replay}");
    assert_eq!(start_replay["event_id"], started["event_id"]);
    assert_eq!(start_replay["replayed"], true);
    let operations_before_cli_replay = work_journal(&store);
    let start_operation = operations_before_cli_replay
        .iter()
        .find(|record| record.event.idempotency_key == "start-store-live-1")
        .expect("HTTP start Work revision")
        .clone();
    assert_eq!(start_operation.event.id, started["event_id"]);
    assert_eq!(start_operation.event.expected_version, 2);
    assert_eq!(start_operation.event.resulting_version, 3);
    assert_eq!(
        serde_json::to_value(&start_operation.work).expect("start Work projection"),
        started["projection"]
    );
    let cli_start_args = [
        "--space",
        space_id.as_str(),
        "--project",
        project_id.as_str(),
        "team-run",
        "work",
        "start",
        "--team-run-id",
        run_id,
        "--work-id",
        "work-store-live-1",
        "--expected-version",
        "2",
        "--member-run-id",
        member_run_id,
        "--idempotency-key",
        "start-store-live-1",
        "--event-id",
        "role-action:start-store-live-1",
    ];
    // There is no second member write entrance to keep in parity with: the
    // local `team-run work start` member verb is retired, so the same request
    // that the authenticated Role Action committed is refused here — twice,
    // and without appending anything to the authoritative ledger.
    for attempt in 1..=2 {
        let cli_start = run_firm_with_env(
            &home,
            &root,
            &cli_start_args,
            &[
                ("FIRM_TEAM_RUN_ID", run_id),
                ("FIRM_MEMBER_RUN_ID", member_run_id),
            ],
        );
        assert!(
            !cli_start.status.success(),
            "retired CLI member verb must refuse on attempt {attempt}: {cli_start:?}"
        );
        let refusal = String::from_utf8_lossy(&cli_start.stderr).to_string();
        assert!(
            refusal.contains("RETIRED_WRITE_AUTHORITY")
                && refusal.contains("firm member work start"),
            "refusal must name the authenticated entrance: {refusal}"
        );
        let operations_after_cli = work_journal(&store);
        assert_eq!(
            operations_after_cli.len(),
            operations_before_cli_replay.len(),
            "a refused CLI member write must not append a Work event"
        );
        assert!(
            legacy_ledger_rows(&store).is_empty(),
            "and nothing reaches the legacy ledger file either"
        );
        let unchanged_operation = operations_after_cli
            .iter()
            .find(|record| record.event.idempotency_key == "start-store-live-1")
            .expect("stable authenticated start revision");
        assert_eq!(unchanged_operation.event.id, start_operation.event.id);
        assert_eq!(
            unchanged_operation.event.resulting_version,
            start_operation.event.resulting_version
        );
    }
    let changed_start_cas = action_headers(MEMBER_TOKEN, "start-store-live-1", "3");
    let (status, changed_start_cas_result) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"start_work"}),
        &changed_start_cas,
    );
    assert_eq!(
        status, 409,
        "same lifecycle key with changed If-Match must fail: {changed_start_cas_result}"
    );
    let (status, reused_start_key) = serve.post_json_with_headers(
        &format!(
            "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/claim?project={project_id}"
        ),
        &serde_json::json!({"action":"claim_work"}),
        &start_headers,
    );
    assert_eq!(status, 409, "same key changed command: {reused_start_key}");

    let progress_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/reports?project={project_id}",
        team.id
    );
    let progress_headers = action_headers(MEMBER_TOKEN, "progress-store-live-1", "3");
    let progress_intent = serde_json::json!({
        "action":"write_report",
        "summary":"implementation progressing",
        "evidence_refs":["check:progress"]
    });
    let (status, progress) =
        serve.post_json_with_headers(&progress_route, &progress_intent, &progress_headers);
    assert_eq!(status, 200, "progress report: {progress}");
    assert_eq!(progress["projection"]["kind"], "progress");
    let (status, progress_replay) =
        serve.post_json_with_headers(&progress_route, &progress_intent, &progress_headers);
    assert_eq!(status, 200, "progress replay: {progress_replay}");
    assert_eq!(progress_replay["event_id"], progress["event_id"]);
    assert_eq!(progress_replay["replayed"], true);
    let changed_progress_cas = action_headers(MEMBER_TOKEN, "progress-store-live-1", "2");
    let (status, changed_progress) =
        serve.post_json_with_headers(&progress_route, &progress_intent, &changed_progress_cas);
    assert_eq!(
        status, 409,
        "progress replay with changed If-Match must fail: {changed_progress}"
    );

    submission_revision_loop::assert_submission_refusal_then_revision_and_accept(
        submission_revision_loop::SubmissionRevisionContext {
            serve: &serve,
            store: &store,
            space_id: &space_id,
            project_id: &project_id,
            node_id,
            run_id,
            worker_id,
            member_run_id,
            team: &team,
            daemon: &daemon,
            worker_membership: &worker_membership,
            worker_session: &worker_session,
            view_route: &view_route,
            start_route: &start_route,
            first_attempt: &first_attempt,
            journal_before,
        },
    );
    // Exactly the create, the membership assignment, the two exact starts, the
    // submission's Review revision, the request-changes, the second submission
    // and this accept — every one of them a `work` transition, none of them a
    // fabricated legacy row.
    assert_eq!(work_journal(&store).len(), journal_before + 8);
    assert!(
        legacy_ledger_rows(&store).is_empty(),
        "and no Work write reaches the legacy ledger file"
    );
    assert!(store
        .canonical_operations_for_space(&ExecutionSpaceId::new(&space_id))
        .expect("canonical operations")
        .iter()
        .any(|operation| operation.event.aggregate_kind == "work"
            && operation.event.aggregate_id == "work-store-live-1"
            && operation.event.transition == "accepted"),
        "canonical acceptance and its delegation/gate/report roll-up must be one canonical operation");

    submission_evidence_refusal::assert_report_only_submission_succeeds(
        &serve,
        &store,
        &space_id,
        run_id,
        &project_id,
        &team,
        worker_id,
        None,
    );
    submission_evidence_refusal::assert_report_only_submission_succeeds(
        &serve,
        &store,
        &space_id,
        run_id,
        &project_id,
        &team,
        worker_id,
        Some((&home, &root)),
    );

    let before_report_cli_retry = ledger_digest(store.root());
    for _ in 0..2 {
        let retry = run_firm(
            &home,
            &root,
            &[
                "--space",
                &space_id,
                "--project",
                &project_id,
                "team-run",
                "work",
                "accept",
                "--work-id",
                "work-store-live-report-only-1",
                "--expected-version",
                "4",
                "--idempotency-key",
                "accept-report-only-1",
            ],
        );
        assert!(retry.status.success(), "CLI report-only retry: {retry:?}");
        let projection: serde_json::Value = serde_json::from_slice(&retry.stdout).unwrap();
        assert_eq!(projection["resolution"], "accepted");
        assert_eq!(projection["version"], 5);
    }
    let after_report_cli_retry = ledger_digest(store.root());
    let changed = after_report_cli_retry
        .iter()
        .filter(|entry| !before_report_cli_retry.contains(entry))
        .map(|entry| entry.0.as_str())
        .collect::<Vec<_>>();
    assert!(
        changed.is_empty(),
        "CLI acceptance replay changed ledgers: {changed:?}"
    );

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
