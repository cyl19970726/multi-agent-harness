//! HTTP boundary coverage for the Wave 4B local RoleViews.

mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm, ServeHandle,
    TempHome,
};
use harness_core::agentfirm_api::{ActorKind, ActorRef, DeliveryClaim, MutationContext};
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

    let member_run_id = created_run["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member run id");
    let member_view_route =
        format!("/v1/views/member-workbench/{member_run_id}?project={project_id}");
    let (status, member_view) =
        serve.get_json_with_headers(&member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 200, "Member RoleView: {member_view}");
    assert!(member_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| action["kind"] == "claim_work")));
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
    let accept_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/accept?project={project_id}",
        team.id
    );
    let canonical_before_accept = store
        .canonical_operations_for_space(&space_id)
        .expect("before accept")
        .len();
    let no_confirm_headers = action_headers(TOKEN, "accept-no-confirm", "3");
    let (status, no_confirm) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &no_confirm_headers,
    );
    assert_eq!(status, 409, "missing confirmation: {no_confirm}");
    let member_accept_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "accept-member-spoof"),
        ("If-Match", "3"),
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
        ("If-Match", "2"),
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
        ("If-Match", "3"),
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
    assert_eq!(
        store.work_operations().expect("accept roll-up").len(),
        before + 2,
        "canonical accept must not fabricate a second legacy Work transition"
    );
    assert!(store
        .canonical_operations_for_space(&space_id)
        .expect("canonical operations")
        .iter()
        .any(|operation| operation.event.aggregate_kind == "work"
            && operation.event.aggregate_id == "work-store-live-1"
            && operation.event.transition == "accepted"),
        "canonical acceptance and its delegation/gate/report roll-up must be one canonical operation");

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
