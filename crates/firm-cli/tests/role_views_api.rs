//! HTTP boundary coverage for the Wave 4B local RoleViews.

mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm, ServeHandle,
    TempHome,
};
use harness_core::agentfirm_api::{ActorKind, ActorRef, DeliveryClaim, MutationContext};
use harness_core::MemberRunStatus;
use harness_store::HarnessStore;

const TOKEN: &str = "role-view-local-capability";
const MEMBER_TOKEN: &str = "role-view-member-capability";
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
            name.ends_with(".jsonl")
                .then(|| (name, std::fs::read(entry.path()).expect("read ledger")))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
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
    let mission = run(&[
        "mission",
        "create",
        "--title",
        "Role action loop",
        "--objective",
        "Prove the authenticated local product loop",
    ]);
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();
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
    ]);
    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
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
        "token": OPERATOR_TOKEN,
        "actor": {"kind":"service","id":node_id},
        "authority_actors": []
    },{
        "token": WRONG_OPERATOR_TOKEN,
        "actor": {"kind":"service","id":"wrong-node"},
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
            "members": [{"agent_member_id":worker_id,"name":"worker","role":"builder","provider":"codex"}]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created_run}");
    let run_id = created_run["result"]["team_run"]["id"]
        .as_str()
        .expect("run id");

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
    let message_headers = action_headers(TOKEN, "message-current-team-revision", &team_revision);
    let (status, sent_message) =
        serve.post_json_with_headers(&message_route, &message_intent, &message_headers);
    assert_eq!(status, 200, "current Team Message CAS: {sent_message}");
    let (status, replayed_message) =
        serve.post_json_with_headers(&message_route, &message_intent, &message_headers);
    assert_eq!(status, 200, "Team Message replay: {replayed_message}");
    assert_eq!(replayed_message["replayed"], true);
    assert_eq!(replayed_message["event_id"], sent_message["event_id"]);

    let member_run_id = created_run["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member run id");
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
    let (status, decision) = serve.post_json_with_headers(
        &decision_route,
        &serde_json::json!({
            "action":"request_decision",
            "body":"Host decision is required",
            "work_id":"work-store-live-1",
            "evidence_refs":["check:member-request-decision"]
        }),
        &decision_headers,
    );
    assert_eq!(status, 200, "Member request-decision to Host: {decision}");
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
        .compare_and_append_member_run(&failed_provider_run, &successor_provider_run)
        .expect("append higher-generation replacement runtime");
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
        .create_trust_member_run(
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

    let company_route = format!("/v1/views/company-work?project={project_id}");
    let (status, company) =
        serve.get_json_with_headers(&company_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Company RoleView: {company}");
    assert!(company["data"]["items"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    let snapshot_vector = company["data"]["page"]["snapshot_vector"]
        .as_array()
        .expect("snapshot vector");
    assert_eq!(
        snapshot_vector.len(),
        2,
        "Company cursor must bind every space"
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
    let route = format!("/v1/views/company-work?project={project_id}");
    let (status, denied) = serve.get_json(&route);
    assert_eq!(status, 401, "unauthenticated RoleView: {denied}");
    assert_eq!(denied["error"]["code"], "NOT_AUTHORIZED");

    let before = ledger_digest(serve.fixture_store_root());
    let (status, company) = serve.get_json_with_headers(&route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Company RoleView: {company}");
    assert_eq!(company["schema_version"], "agentfirm.role_views.v1");
    assert_eq!(company["data"]["items"], serde_json::json!([]));
    assert_eq!(
        company["data"]["page"]["next_cursor"],
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
        "/v1/views/member-workbench/missing",
        "/v1/views/operator/missing",
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
    assert!(actions
        .iter()
        .any(|action| action["kind"] == "admit_provider" && action["disabled_reason"].is_null()));
    let node_revision = operator["data"]["node"]["node_revision"]
        .as_u64()
        .expect("node revision")
        .to_string();
    let initial_stop_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-initial-stop"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-stop"),
    ];
    let stop_route = format!("/v1/agentfirm/nodes/{node_id}/daemon-stop?project={project_id}");
    let (status, initial_stopped) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop"}),
        &initial_stop_headers,
    );
    assert_eq!(status, 200, "initial daemon stop: {initial_stopped}");
    let (status, after_stop) =
        serve.get_json_with_headers(&operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator after stop: {after_stop}");
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
        &serde_json::json!({"action":"daemon_start","max_concurrency":1}),
        &start_headers,
    );
    assert_eq!(status, 200, "daemon start: {started}");
    let (status, start_replay) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"daemon_start","max_concurrency":1}),
        &start_headers,
    );
    assert_eq!(status, 200, "daemon start replay: {start_replay}");
    assert_eq!(start_replay["replayed"], true);
    assert_eq!(start_replay["event_id"], started["event_id"]);
    let (status, changed_start) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"daemon_start","max_concurrency":2}),
        &start_headers,
    );
    assert_eq!(
        status, 409,
        "changed daemon start replay must fail closed: {changed_start}"
    );

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
    let intent = serde_json::json!({"action":"admit_provider","provider":"codex","execution_mode":"codex_app_server"});
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
    let hostile = serde_json::json!({"action":"admit_provider","provider":"codex","execution_mode":"codex_app_server","provider_version":"browser-spoof","evidence_refs":["browser-proof"]});
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

    let stop_headers = [
        ("X-AgentFirm-Token", OPERATOR_TOKEN),
        ("Idempotency-Key", "operator-daemon-stop"),
        ("If-Match", node_revision.as_str()),
        ("X-AgentFirm-Confirm", "daemon-stop"),
    ];
    let (status, stopped) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop"}),
        &stop_headers,
    );
    assert_eq!(status, 200, "daemon stop: {stopped}");
    let (status, stop_replay) = serve.post_json_with_headers(
        &stop_route,
        &serde_json::json!({"action":"daemon_stop"}),
        &stop_headers,
    );
    assert_eq!(status, 200, "daemon stop replay: {stop_replay}");
    assert_eq!(stop_replay["replayed"], true);
    assert_eq!(stop_replay["event_id"], stopped["event_id"]);
}
