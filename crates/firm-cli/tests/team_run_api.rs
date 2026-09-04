use harness_core::agentfirm_api::CanonicalMessageDeliveryStatus;
use harness_store::HarnessStore;
use std::time::Duration;
mod fake_provider;
mod firm_env;
use firm_env::{
    collect_sse_data, create_canonical_agent_member, current_project_id, current_space_id,
    member_run_for_work_owner, run_firm, run_firm_with_env, ServeHandle, TempHome,
};

const NATIVE_SELECTOR_CLEAN_ENV: &[(&str, &str)] = &[
    ("FIRM_ROOT", ""),
    ("FIRM_PROJECT", ""),
    ("FIRM_PROJECT_ID", ""),
    ("FIRM_SPACE", ""),
    ("FIRM_COMPANY", ""),
    ("FIRM_MISSION_ID", ""),
    ("FIRM_ORIGIN_WAVE_ID", ""),
    ("FIRM_TEAM_RUN_ID", ""),
    ("FIRM_MEMBER_RUN_ID", ""),
    ("FIRM_WORK_ID", ""),
    ("FIRM_WORK_VERSION", ""),
    ("HARNESS_ROOT", ""),
    ("HARNESS_PROJECT", ""),
    ("HARNESS_PROJECT_ID", ""),
    ("HARNESS_SPACE", ""),
    ("HARNESS_COMPANY", ""),
    ("HARNESS_MISSION_ID", ""),
    ("HARNESS_ORIGIN_WAVE_ID", ""),
    ("HARNESS_TEAM_RUN_ID", ""),
    ("HARNESS_MEMBER_RUN_ID", ""),
    ("HARNESS_WORK_ID", ""),
    ("HARNESS_WORK_VERSION", ""),
    ("HARNESS_HOME", ""),
];

const FIXTURE_TEAM_ID: &str = "team-runtime-fixture";
const FIXTURE_MISSION_ID: &str = "mission-runtime-fixture";
const FIXTURE_HOST_ID: &str = "agent-runtime-host";

#[path = "team_run_api/fixtures.rs"]
mod fixtures;
use fixtures::{assert_trust_native_binding_synced, wait_for_file};

#[test]
fn dynamic_workflow_http_and_snapshot_surfaces_are_absent() {
    let home = TempHome::new("dynamic-workflow-http-retired");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (route_status, route_body) = serve.get_json("/v1/workflows");
    assert_eq!(route_status, 404, "body: {route_body}");

    let (snapshot_status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot_status, 200, "snapshot: {snapshot}");
    for key in [
        "workflow_runs",
        "workflow_steps",
        "workflow_patches",
        "workflow_artifact_manifests",
    ] {
        assert!(
            snapshot.get(key).is_none(),
            "retired key {key} leaked: {snapshot}"
        );
    }
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn replace_supervisor_lease(store: &HarnessStore, run_id: &str) {
    let lease = store
        .latest_team_supervisor_lease(run_id)
        .expect("read current lease")
        .expect("current lease");
    store
        .release_team_supervisor_lease(
            run_id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms(),
        )
        .expect("release current lease");
    store
        .acquire_team_supervisor_under_node_lease(
            run_id,
            &lease.node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            &lease.execution_space_id,
            &lease.project_binding_id,
            "terminal-frame-fencing-supervisor",
            std::process::id(),
            "tcp://127.0.0.1:1",
            current_unix_ms(),
            15_000,
        )
        .expect("replace current lease");
}

fn member_semantic_row_counts(store: &HarnessStore, member_id: &str) -> (usize, usize, usize) {
    let member_rows = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .filter(|member| member.id == member_id)
        .count();
    let actions = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .filter(|action| action.member_run_id == member_id)
        .count();
    let handoffs = store
        .legacy_team_messages()
        .expect("team messages")
        .into_iter()
        .filter(|message| {
            message.sender_runtime_id == member_id
                && message.kind == harness_core::ProviderDispatchIntent::Message
        })
        .count();
    (member_rows, actions, handoffs)
}

fn init_project_selector_clean(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm_with_env(home, &root, &["init"], NATIVE_SELECTOR_CLEAN_ENV);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    seed_runtime_team(home, &project_id, NATIVE_SELECTOR_CLEAN_ENV);
    project_id
}

/// `harness init` a project rooted at `<base>/<name>` and return its derived id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);
    seed_runtime_team(home, &project_id, &[]);
    project_id
}

/// Every TeamRun now belongs to a durable flat AgentTeam. The broad runtime
/// regression suite predates that invariant, so give its scenarios one
/// explicit Mission-owned Team instead of weakening production admission.
fn seed_runtime_team(home: &TempHome, project_id: &str, env: &[(&str, &str)]) {
    let run = |args: &[&str]| {
        let mut full = vec!["--project", project_id];
        full.extend_from_slice(args);
        let out = run_firm_with_env(home, home.base(), &full, env);
        assert!(
            out.status.success(),
            "fixture command {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    let host = create_canonical_agent_member(
        home,
        home.base(),
        project_id,
        FIXTURE_HOST_ID,
        "Runtime Host",
        "host",
        "kimi",
        env,
    );
    assert!(
        host.status.success(),
        "canonical fixture Host failed: {}",
        String::from_utf8_lossy(&host.stderr)
    );
    let fixture_members = [
        ("lead", "coordinator", "kimi"),
        ("worker", "implementer", "kimi"),
        ("worker-a", "implementer", "kimi"),
        ("recoverer", "builder", "kimi"),
        ("solo", "observer", "kimi"),
        ("alice", "builder", "codex"),
        ("custom-reviewer", "reviewer", "custom"),
        ("ext-reviewer", "reviewer", "kimi"),
        ("codex-worker", "implementer", "codex"),
        ("worker-1", "implementer", "codex"),
        ("bob", "implementer", "codex"),
        ("charlie", "implementer", "codex"),
    ];
    for (id, role, provider) in fixture_members {
        let member = create_canonical_agent_member(
            home,
            home.base(),
            project_id,
            id,
            id,
            role,
            provider,
            env,
        );
        assert!(
            member.status.success(),
            "canonical fixture member {id} failed: {}",
            String::from_utf8_lossy(&member.stderr)
        );
    }
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    firm_env::seed_historical_mission(home, project_id, FIXTURE_MISSION_ID, "Runtime Regression");
    let node = run(&["node", "init"]);
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id");
    run(&[
        "node",
        "project",
        "register",
        "--node-id",
        node_id,
        "--execution-space-id",
        project_id,
        "--project-binding-id",
        project_id,
    ]);
    run(&[
        "team",
        "create",
        "--id",
        FIXTURE_TEAM_ID,
        "--name",
        "Runtime Fixture Team",
        "--description",
        "Flat Team used by TeamRun regression scenarios",
        "--mission-id",
        FIXTURE_MISSION_ID,
        "--host-agent-id",
        FIXTURE_HOST_ID,
        "--node-id",
        node_id,
        "--member",
        FIXTURE_HOST_ID,
    ]);
    for (id, _, _) in fixture_members {
        run(&[
            "team",
            "add-member",
            "--id",
            FIXTURE_TEAM_ID,
            "--member",
            id,
        ]);
    }
}

/// Seed a current Mission plus one pre-cutover Legacy Wave row. Current
/// TeamRun creation binds only the fixture AgentTeam; the Legacy row exists
/// solely to prove the historical snapshot/read projection.
fn seed_mission_with_legacy_wave(home: &TempHome, project_id: &str) {
    let store = home.spaces_dir().join(project_id);
    use std::io::Write as _;
    let mut missions = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.join("missions.jsonl"))
        .expect("open missions");
    writeln!(
        missions,
        "{}",
        serde_json::json!({
            "id": "mission-test",
            "title": "Test Mission",
            "objective": "Exercise team-run join",
            "desired_outcome": null,
            "status": "running",
            "wave_ids": ["wave-test"],
            "outcome_summary": null,
            "created_at": "2026-07-19T00:00:00Z",
            "updated_at": "2026-07-19T00:00:00Z",
            "completed_at": null
        })
    )
    .expect("seed mission");
    std::fs::write(
        store.join("waves.jsonl"),
        serde_json::json!({
            "id": "wave-test",
            "mission_id": "mission-test",
            "index": 2,
            "title": "Test Wave",
            "objective": "Exercise team run",
            "exit_criteria": null,
            "status": "planned",
            "executor_kind": "agent_team",
            "executor_run_ids": [],
            "accepted_run_id": null,
            "plan_note": null,
            "outcome_summary": null,
            "artifact_refs": [],
            "gate_status": "pending",
            "gate_note": null,
            "accepted_by": null,
            "accepted_at": null,
            "created_at": "2026-07-19T00:00:00Z",
            "updated_at": "2026-07-19T00:00:00Z"
        })
        .to_string()
            + "\n",
    )
    .expect("seed wave");
}

/// Seed one additional historical Wave row directly, bypassing the retired
/// `wave create` write path (ADR 0051). Unlike `seed_mission_with_legacy_wave`
/// (which overwrites `waves.jsonl` with exactly one row) this appends, so
/// tests needing more than one historical Wave -- or a Wave alongside one
/// already seeded by `seed_mission_with_legacy_wave` -- can call it repeatedly.
fn seed_historical_wave(
    home: &TempHome,
    project_id: &str,
    id: &str,
    mission_id: &str,
    index: u64,
    executor_kind: &str,
) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(project_id).join("waves.jsonl");
    let mut ledger = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open wave ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": id,
            "mission_id": mission_id,
            "index": index,
            "title": "Historical Wave",
            "objective": "Seeded pre-cutover row for read/navigation coverage",
            "executor_kind": executor_kind,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical wave");
}

/// Seed one historical Mission Log row directly, bypassing the retired
/// `mission log append` write path (DOC-108), so legacy log reads and the
/// snapshot projection can be proven against pre-cutover history.
fn seed_historical_mission_log(
    home: &TempHome,
    project_id: &str,
    mission_id: &str,
    revision: u64,
    kind: &str,
    body: &str,
    actor: &str,
) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(project_id).join("mission_log.jsonl");
    let mut ledger = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open mission log ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": format!("mission-log-{mission_id}-{revision}"),
            "mission_id": mission_id,
            "revision": revision,
            "kind": kind,
            "body": body,
            "actor": actor,
            "created_at": "unix-ms:1",
        })
    )
    .expect("append historical mission log entry");
}

/// Run `harness team-run ...` in the given project and return parsed stdout JSON.
fn canonical_submit_work_fixture(
    home: &TempHome,
    project_id: &str,
    work_id: &str,
    source_version: u64,
    summary: &str,
) -> serde_json::Value {
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(project_id));
    let work = store
        .latest_works()
        .expect("canonical submit Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("canonical submit Work");
    let team_id = work
        .accountable_team_id
        .clone()
        .unwrap_or_else(|| FIXTURE_TEAM_ID.to_string());
    let agent_member_id = work
        .owner_member_id
        .clone()
        .expect("canonical accountable AgentMember");
    let result_version = source_version + 1;
    let report_id = format!("report-{work_id}-v{result_version}");
    let candidate = serde_json::json!({
        "kind": "content_digest",
        "value": format!("{work_id}-v{result_version}")
    });
    let fingerprint = harness_store::canonical_json_fingerprint(&candidate);
    let report = serde_json::json!({
        "command": "create_work_report",
        "team_id": team_id,
        "report": {
            "id": report_id,
            "work_id": work_id,
            "work_revision": result_version,
            "report_revision": 1,
            "kind": "result",
            "authored_by": {"kind": "agent_member", "id": agent_member_id},
            "summary": summary,
            "base_revision": null,
            "candidate": candidate,
            "candidate_fingerprint": fingerprint,
            "finding_refs": [],
            "failure_analysis_ref": null,
            "artifact_refs": [],
            "check_refs": [],
            "evidence_refs": ["integration-test-canonical-result"],
            "known_risks": [],
            "confidence": "high",
            "recommended_next_action": "accept",
            "created_at": "unix-ms:1"
        }
    })
    .to_string();
    let report_out = run_firm(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "member-trust",
            "mutate",
            "--actor-kind",
            "agent_member",
            "--actor-id",
            &agent_member_id,
            "--idempotency-key",
            &format!("fixture-report-{work_id}-v{result_version}"),
            "--expected-version",
            "0",
            "--json",
            &report,
        ],
    );
    assert!(
        report_out.status.success(),
        "canonical WorkReport fixture failed: {}",
        String::from_utf8_lossy(&report_out.stderr)
    );
    serde_json::json!({
        "id": work_id,
        "phase": "review",
        "condition": "normal",
        "version": result_version,
        "work_report_id": report_id,
        "candidate_fingerprint": fingerprint,
    })
}

fn team_run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    if args.starts_with(&["work", "accept"]) {
        let value = |flag: &str| {
            args.windows(2)
                .find_map(|pair| (pair[0] == flag).then_some(pair[1]))
                .unwrap_or_else(|| panic!("missing {flag} in canonical accept fixture"))
        };
        let work_id = value("--work-id");
        let expected_version = value("--expected-version")
            .parse::<u64>()
            .expect("canonical accept expected version");
        let store = harness_store::HarnessStore::new(home.spaces_dir().join(project_id));
        let work = store
            .latest_works()
            .expect("canonical accept Works")
            .into_iter()
            .find(|work| work.id == work_id)
            .expect("canonical accept Work");
        let team_id = work
            .accountable_team_id
            .clone()
            .unwrap_or_else(|| FIXTURE_TEAM_ID.to_string());
        let report_id = format!("report-{work_id}-v{expected_version}");
        let candidate = serde_json::json!({
            "kind": "content_digest",
            "value": format!("{work_id}-v{expected_version}")
        });
        let fingerprint = harness_store::canonical_json_fingerprint(&candidate);
        let accept = serde_json::json!({
            "command": "accept_work",
            "team_id": team_id,
            "work_id": work_id,
            "work_report_id": report_id,
            "candidate_fingerprint": fingerprint,
            "updated_at": "unix-ms:2"
        })
        .to_string();
        let accepted = run_firm(
            home,
            home.base(),
            &[
                "--project",
                project_id,
                "member-trust",
                "mutate",
                "--actor-kind",
                "agent_member",
                "--actor-id",
                FIXTURE_HOST_ID,
                "--idempotency-key",
                &format!("fixture-accept-{work_id}-v{expected_version}"),
                "--expected-version",
                &expected_version.to_string(),
                "--json",
                &accept,
            ],
        );
        assert!(
            accepted.status.success(),
            "canonical Work accept fixture failed: {}",
            String::from_utf8_lossy(&accepted.stderr)
        );
        let envelope: serde_json::Value =
            serde_json::from_slice(&accepted.stdout).expect("canonical accept JSON");
        return envelope["projection"].clone();
    }
    if matches!(args.first(), Some(&"create") | Some(&"add-member")) {
        let team_id = args
            .windows(2)
            .find_map(|pair| (pair[0] == "--agent-team-id").then_some(pair[1]))
            .unwrap_or(FIXTURE_TEAM_ID);
        for raw in args
            .windows(2)
            .filter_map(|pair| (pair[0] == "--member").then_some(pair[1]))
        {
            let identity = raw.split(['#', '@']).next().unwrap_or(raw);
            let parts = identity.split(':').collect::<Vec<_>>();
            if parts.len() < 3 {
                continue;
            }
            let id = parts[0];
            let role = parts[1];
            let provider = parts[2].split('/').next().unwrap_or(parts[2]);
            let existing = harness_store::HarnessStore::new(home.spaces_dir().join(project_id))
                .all_trust_agent_members()
                .expect("fixture identities")
                .into_iter()
                .any(|member| member.id == id);
            if !existing {
                let created = create_canonical_agent_member(
                    home,
                    home.base(),
                    project_id,
                    id,
                    id,
                    role,
                    provider,
                    &[],
                );
                assert!(
                    created.status.success(),
                    "create fixture AgentMember: {created:?}"
                );
            }
            let store = harness_store::HarnessStore::new(home.spaces_dir().join(project_id));
            let team = store
                .latest_teams()
                .expect("fixture teams")
                .remove(team_id)
                .expect("fixture Team");
            if team.host_agent_id != id && !team.member_ids.iter().any(|member| member == id) {
                let added = run_firm(
                    home,
                    home.base(),
                    &[
                        "--project",
                        project_id,
                        "team",
                        "add-member",
                        "--id",
                        team_id,
                        "--member",
                        id,
                    ],
                );
                assert!(added.status.success(), "add fixture AgentMember: {added:?}");
            }
        }
    }
    let mut full = vec!["--project", project_id, "team-run"];
    full.extend_from_slice(args);
    if args.first() == Some(&"create") && !args.contains(&"--agent-team-id") {
        full.push("--agent-team-id");
        full.push(FIXTURE_TEAM_ID);
    }
    let out = run_firm(home, home.base(), &full);
    assert!(
        out.status.success(),
        "team-run {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("team-run {args:?} stdout not JSON ({e})"))
}

/// Run a member-authorized `harness team-run ...` command with the same
/// runtime binding that a persistent provider process receives.
fn member_team_run_json(
    home: &TempHome,
    project_id: &str,
    team_run_id: &str,
    member_run_id: &str,
    args: &[&str],
) -> serde_json::Value {
    if args.starts_with(&["work", "submit"]) {
        let value = |flag: &str| {
            args.windows(2)
                .find_map(|pair| (pair[0] == flag).then_some(pair[1]))
                .unwrap_or_else(|| panic!("missing {flag} in canonical submit fixture"))
        };
        return canonical_submit_work_fixture(
            home,
            project_id,
            value("--work-id"),
            value("--expected-version")
                .parse()
                .expect("canonical submit expected version"),
            args.windows(2)
                .find_map(|pair| (pair[0] == "--result").then_some(pair[1]))
                .unwrap_or("canonical integration-test result"),
        );
    }
    let mut full = vec!["--project", project_id, "team-run"];
    full.extend_from_slice(args);
    let out = run_firm_with_env(
        home,
        home.base(),
        &full,
        &[
            ("FIRM_TEAM_RUN_ID", team_run_id),
            ("FIRM_MEMBER_RUN_ID", member_run_id),
        ],
    );
    assert!(
        out.status.success(),
        "member team-run {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("member team-run {args:?} stdout not JSON ({e})"))
}

fn command_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm(home, home.base(), &full);
    assert!(
        out.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("command {args:?} stdout not JSON ({e})"))
}

#[cfg(any())] // Historical CLI-send/persistent-TeamRun authority; canonical fabric journeys cover current behavior.
fn persistent_codex_supervisor_survives_handoffs_transport_loss_and_team_completion() {
    let home = TempHome::new("team-run-persistent-codex-supervisor");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_codex_team_shim(&home.base().join("fakebin-persistent-codex"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let name_marker = home.base().join("codex-thread-names.jsonl");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PATH", path.as_str()),
            ("FAKE_CODEX_AUTO_COMPLETE", "1"),
            ("FAKE_CODEX_EXIT_AFTER_FIRST_TURN", "1"),
            // This test intentionally sends follow-up mail after observing
            // idle. Keep the test-only supervisor bound well above slow CI
            // HTTP/snapshot latency; explicit Close still ends both members.
            ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
            (
                "FAKE_CODEX_NAME_MARKER",
                name_marker.to_str().expect("name marker"),
            ),
        ],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise persistent supervisor semantics",
            "members": [
                {"name": "Builder", "role": "implementer", "provider": "codex",
                 "initial_work": "Build and report the result"},
                {"name": "Reviewer", "role": "reviewer", "provider": "codex",
                 "initial_work": "Review and report the result"}
            ]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let builder = member_run_for_work_owner(&created["result"], 0);
    let reviewer = member_run_for_work_owner(&created["result"], 1);
    let builder_id = builder["id"].as_str().unwrap().to_string();
    let reviewer_id = reviewer["id"].as_str().unwrap().to_string();
    let builder_agent_member_id = builder["agent_member_id"]
        .as_str()
        .expect("Builder AgentMember");
    let builder_work_id = created["result"]["works"]
        .as_array()
        .expect("Works")
        .iter()
        .find(|work| work["owner_member_id"].as_str() == Some(builder_agent_member_id))
        .inspect(|work| {
            assert_eq!(
                (
                    work["active_member_run_id"].is_null(),
                    work["assignee_membership_id"].as_str().is_some()
                ),
                (true, true),
                "initial Work must bind stable responsibility without runtime identity: {work}"
            );
        })
        .and_then(|work| work["id"].as_str())
        .expect("Builder Work")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut recovered_idle = false;
    for _ in 0..200 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let builder = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(builder_id.as_str()));
        let disconnected = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(builder_id.as_str())
                    && action["action_type"].as_str() == Some("disconnected")
            });
        recovered_idle = builder.is_some_and(|member| {
            member["status"].as_str() == Some("idle")
                && member["native_session"]["native_session_id"].as_str()
                    == Some("thread_fake_codex_app_server")
        }) && disconnected;
        if recovered_idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        recovered_idle,
        "transport loss was not exposed and resumed on the same native session"
    );

    // A TeamRun cannot be completed while its durable Works remain unfinished.
    // Provider RESULT only ends a native turn; the members must explicitly
    // submit their Works and the Host must explicitly accept them.
    let (status, rejected_completion) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 400, "body: {rejected_completion}");
    assert!(
        rejected_completion
            .to_string()
            .contains("Works remain non-terminal"),
        "completion guard should explain the unfinished Works: {rejected_completion}"
    );

    let mut both_idle = false;
    for _ in 0..200 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        both_idle = [&builder_id, &reviewer_id].iter().all(|member_id| {
            snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("idle")
                })
        });
        if both_idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        both_idle,
        "both members must be idle before explicit Work review"
    );

    let (_, before_review) = serve.get_json("/v1/snapshot");
    let owned_works = before_review["works"]
        .as_array()
        .expect("Works in snapshot")
        .iter()
        .filter(|work| work["team_run_id"].as_str() == Some(run_id.as_str()))
        .map(|work| {
            let owner_member_id = work["owner_member_id"]
                .as_str()
                .expect("stable Work owner AgentMember");
            let member_run_id = before_review["member_runs"]
                .as_array()
                .expect("MemberRuns in snapshot")
                .iter()
                .find(|member| {
                    member["team_run_id"].as_str() == Some(run_id.as_str())
                        && member["agent_member_id"].as_str() == Some(owner_member_id)
                })
                .and_then(|member| member["id"].as_str())
                .expect("exact current MemberRun for stable Work responsibility");
            (
                work["id"].as_str().expect("Work id").to_string(),
                member_run_id.to_string(),
                work["version"].as_u64().expect("Work version"),
                work["phase"].as_str().expect("Work phase").to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(owned_works.len(), 2, "expected one Work per member");
    for (work_id, member_run_id, version, phase) in owned_works {
        let active_version = if phase == "open" {
            let started = member_team_run_json(
                &home,
                &project_id,
                &run_id,
                &member_run_id,
                &[
                    "work",
                    "start",
                    "--team-run-id",
                    &run_id,
                    "--work-id",
                    &work_id,
                    "--expected-version",
                    &version.to_string(),
                    "--member-run-id",
                    &member_run_id,
                    "--json",
                ],
            );
            started["version"].as_u64().expect("started version")
        } else {
            assert_eq!(phase, "active", "unexpected Work phase before submit");
            version
        };
        let submitted = member_team_run_json(
            &home,
            &project_id,
            &run_id,
            &member_run_id,
            &[
                "work",
                "submit",
                "--team-run-id",
                &run_id,
                "--work-id",
                &work_id,
                "--expected-version",
                &active_version.to_string(),
                "--member-run-id",
                &member_run_id,
                "--result",
                "native turn completed; explicit Work submitted for Host review",
                "--json",
            ],
        );
        let submitted_version = submitted["version"].as_u64().expect("submitted version");
        let accepted = team_run_json(
            &home,
            &project_id,
            &[
                "work",
                "accept",
                "--team-run-id",
                &run_id,
                "--work-id",
                &work_id,
                "--expected-version",
                &submitted_version.to_string(),
                "--summary",
                "Host accepted the explicit Work result",
                "--json",
            ],
        );
        assert_eq!(accepted["phase"].as_str(), Some("closed"));
        assert_eq!(accepted["resolution"].as_str(), Some("accepted"));
    }

    // The TeamRun decision remains independent of persistent Member runtime
    // lifetime once all durable Works are accepted.
    let (status, completed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {completed}");
    assert_eq!(completed["result"]["status"].as_str(), Some("completed"));

    let (status, host_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [builder_id],
            "kind": "message",
            "body": "HOST FOLLOW-UP after TeamRun completion",
        }),
    );
    assert_eq!(status, 200, "body: {host_mail}");
    let host_message_id = host_mail["result"]["id"].as_str().unwrap().to_string();
    let conversation_correlation = host_mail["result"]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, peer_mail) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": reviewer_id,
            "recipient_runtime_ids": [builder_id],
            "kind": "message",
            "response_intent": "response_required",
            "body": "PEER FOLLOW-UP after TeamRun completion",
            "correlation_id": conversation_correlation,
            "causation_id": host_message_id,
        }),
    );
    assert_eq!(status, 200, "body: {peer_mail}");
    let peer_message_id = peer_mail["result"]["id"].as_str().unwrap().to_string();

    let mut delivered_once = false;
    let mut builder_completed_rounds = 0;
    for _ in 0..300 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        let deliveries = snapshot["canonical_message_deliveries"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let delivered = |message_id: &str| {
            deliveries
                .iter()
                .find(|delivery| delivery["message_id"].as_str() == Some(message_id))
                .is_some_and(|delivery| {
                    delivery["status"].as_str() == Some("acknowledged")
                        && delivery["attempt"].as_u64() == Some(1)
                })
        };
        delivered_once = delivered(&host_message_id) && delivered(&peer_message_id);
        builder_completed_rounds = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|action| {
                action["member_run_id"].as_str() == Some(builder_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            })
            .count();
        if delivered_once && builder_completed_rounds >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        delivered_once,
        "Host and peer canonical MessageDelivery rows were not each acknowledged exactly once: {}",
        serve.get_json("/v1/snapshot").1
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        builder_completed_rounds >= 2,
        "initial Work and follow-up conversation should produce provider rounds without fabricating Handoff messages: {builder_completed_rounds}"
    );
    let builder_work = snapshot["works"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|work| work["id"].as_str() == Some(builder_work_id.as_str()))
        .expect("Builder Work in snapshot");
    assert_eq!(
        builder_work["phase"].as_str(),
        Some("closed"),
        "the provider RESULT alone did not close Work; the explicit member submit and Host accept above did: {builder_work}"
    );
    assert_eq!(builder_work["resolution"].as_str(), Some("accepted"));

    let native_names = std::fs::read_to_string(&name_marker).expect("thread/name/set requests");
    assert!(
        native_names.contains("\"name\":\"Agent Team · Builder\"")
            && native_names.contains("\"name\":\"Agent Team · Reviewer\""),
        "native Codex threads were not named from Member identity: {native_names}"
    );

    for member_id in [&builder_id, &reviewer_id] {
        let (status, closed) = serve.post_json(
            &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
            &serde_json::json!({"requested_by": "host", "reason": "dogfood lane accepted"}),
        );
        assert_eq!(status, 200, "body: {closed}");
    }
    let mut all_stopped = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        all_stopped = [&builder_id, &reviewer_id].iter().all(|member_id| {
            snapshot["member_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| {
                    member["id"].as_str() == Some(member_id.as_str())
                        && member["status"].as_str() == Some("stopped")
                })
        });
        if all_stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        all_stopped,
        "explicit Host close did not stop both runtimes"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        snapshot["team_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|run| {
                run["id"].as_str() == Some(run_id.as_str())
                    && run["status"].as_str() == Some("completed")
            }),
        "Member close must not rewrite the TeamRun decision"
    );
}

// ---------------------------------------------------------------------------
// Decision-shaped board reads (issue #305): `work list --brief`, `work list
// --since`, and `team-run board-summary`. All three read the same
// authoritative store as `work list`'s full JSON; they only change the
// projection.
// ---------------------------------------------------------------------------

/// A TeamRun with three members -- alice and bob each own Work, charlie stays
/// idle -- and six Works spanning the Work lifecycle axes. Every board-read test
/// seeds its own fixture so the three read paths stay independent.
struct BoardReadFixture {
    home: TempHome,
    project_id: String,
    run_id: String,
    alice_agent_member_id: String,
    bob_agent_member_id: String,
    #[allow(dead_code)] // read by the board-summary test only
    charlie_id: String,
    work_open_id: String,
    work_in_progress_id: String,
    work_review_id: String,
    work_blocked_id: String,
    work_done_id: String,
    work_cancelled_id: String,
}

/// Create one Work and return its id. `owner` is the owning ProviderRuntimeProjection id, or
/// `None` to leave it unassigned in the shared Ready Pool.
fn create_fixture_work(
    home: &TempHome,
    project_id: &str,
    run_id: &str,
    title: &str,
    owner: Option<&str>,
) -> String {
    let args = vec![
        "work",
        "create",
        "--team-run-id",
        run_id,
        "--title",
        title,
        "--completion-criteria",
        "Done when the fixture says so",
    ];
    let created = team_run_json(home, project_id, &args);
    let work_id = created["id"].as_str().expect("Work id");
    if let Some(owner) = owner {
        firm_env::work_execution::assign_work_for_member_run(
            home, project_id, work_id, owner, true,
        );
        firm_env::provider_received_work::record_provider_received_work(
            home,
            project_id,
            work_id,
            &format!("board-fixture-{work_id}"),
        );
    }
    work_id.to_string()
}

fn seed_board_read_fixture(tag: &str) -> BoardReadFixture {
    let home = TempHome::new(tag);
    let project_id = init_project(&home, "alpha");

    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--objective",
            "Exercise decision-shaped board reads",
            "--member",
            "alice:implementer:codex",
            "--member",
            "bob:implementer:codex",
            "--member",
            "charlie:implementer:codex",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    let members = status["members"].as_array().expect("members").clone();
    let member_identity = |name: &str| -> (String, String) {
        let member = &members
            .iter()
            .find(|entry| entry["member_run"]["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("member {name} not found: {members:?}"))["member_run"];
        (
            member["id"].as_str().expect("MemberRun id").to_string(),
            member["agent_member_id"]
                .as_str()
                .expect("AgentMember id")
                .to_string(),
        )
    };
    let (alice_id, alice_agent_member_id) = member_identity("alice");
    let (bob_id, bob_agent_member_id) = member_identity("bob");
    let (charlie_id, _) = member_identity("charlie");

    // Work A: created unassigned, never claimed -- stays `open`. Title is
    // deliberately >60 chars to exercise --brief's title truncation.
    let long_title =
        "Open unassigned Work whose title runs well past the sixty character brief cutoff";
    let work_open_id = create_fixture_work(&home, &project_id, &run_id, long_title, None);

    // Work D: alice owns it, starts it, and the Host blocks it -- `blocked`.
    // Driven to completion before Work B starts so alice never holds two
    // simultaneously `in_progress` Works (the store rejects that as
    // MEMBER_BUSY).
    let work_blocked_id =
        create_fixture_work(&home, &project_id, &run_id, "Blocked Work", Some(&alice_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &alice_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_blocked_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &alice_id,
        ],
    );
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "block",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_blocked_id,
            "--expected-version",
            "3",
            "--reason",
            "Waiting on an external dependency",
        ],
    );

    // Work B: alice owns it and starts it -- stays `in_progress`.
    let work_in_progress_id = create_fixture_work(
        &home,
        &project_id,
        &run_id,
        "In-progress Work",
        Some(&alice_id),
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &alice_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_in_progress_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &alice_id,
        ],
    );

    // Work C: bob owns it, starts it, and submits -- `review`. Driven to
    // completion before Work E for the same MEMBER_BUSY reason as above.
    let work_review_id =
        create_fixture_work(&home, &project_id, &run_id, "Review Work", Some(&bob_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_review_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &bob_id,
        ],
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_review_id,
            "--expected-version",
            "3",
            "--member-run-id",
            &bob_id,
            "--result",
            "Submitted for Host review",
        ],
    );

    // Work E: bob owns it, starts, submits, and the Host accepts -- `done`.
    let work_done_id = create_fixture_work(&home, &project_id, &run_id, "Done Work", Some(&bob_id));
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "2",
            "--member-run-id",
            &bob_id,
        ],
    );
    member_team_run_json(
        &home,
        &project_id,
        &run_id,
        &bob_id,
        &[
            "work",
            "submit",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "3",
            "--member-run-id",
            &bob_id,
            "--result",
            "Done and submitted",
        ],
    );
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "accept",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_done_id,
            "--expected-version",
            "4",
        ],
    );

    // Work F: created unassigned, then the Host cancels it -- `cancelled`.
    let work_cancelled_id =
        create_fixture_work(&home, &project_id, &run_id, "Cancelled Work", None);
    team_run_json(
        &home,
        &project_id,
        &[
            "work",
            "cancel",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_cancelled_id,
            "--expected-version",
            "1",
            "--reason",
            "No longer needed",
        ],
    );

    BoardReadFixture {
        home,
        project_id,
        run_id,
        alice_agent_member_id,
        bob_agent_member_id,
        charlie_id,
        work_open_id,
        work_in_progress_id,
        work_review_id,
        work_blocked_id,
        work_done_id,
        work_cancelled_id,
    }
}

#[path = "team_run_api/busy_host_close_interrupts_then_closes_kimi_0361_as_distinct_effects.rs"]
mod busy_host_close_interrupts_then_closes_kimi_0361_as_distinct_effects;
#[path = "team_run_api/busy_kimi_member_batches_mail_in_order_and_withholds_stale_handoff.rs"]
mod busy_kimi_member_batches_mail_in_order_and_withholds_stale_handoff;
#[path = "team_run_api/canonical_team_message_routes_member_to_host_identity_without_special_inbox_authority.rs"]
mod canonical_team_message_routes_member_to_host_identity_without_special_inbox_authority;
#[path = "team_run_api/close_cancels_kimi_provider_request_without_resuming_member.rs"]
mod close_cancels_kimi_provider_request_without_resuming_member;
#[path = "team_run_api/codex_app_server_member_can_be_steered_in_place.rs"]
mod codex_app_server_member_can_be_steered_in_place;
#[cfg(any())]
#[path = "team_run_api/codex_app_server_member_interrupt_waits_for_provider_terminal_event.rs"]
mod codex_app_server_member_interrupt_waits_for_provider_terminal_event;
#[path = "team_run_api/codex_app_server_multi_question_fails_closed_without_interaction_rows.rs"]
mod codex_app_server_multi_question_fails_closed_without_interaction_rows;
#[path = "team_run_api/codex_app_server_post_handoff_steer_is_independent_and_converges_before_follow_up_round.rs"]
mod codex_app_server_post_handoff_steer_is_independent_and_converges_before_follow_up_round;
#[path = "team_run_api/codex_app_server_question_routes_to_lead_and_resumes_same_turn.rs"]
mod codex_app_server_question_routes_to_lead_and_resumes_same_turn;
#[path = "team_run_api/codex_provider_reported_interruption_is_not_attributed_to_harness.rs"]
mod codex_provider_reported_interruption_is_not_attributed_to_harness;
#[path = "team_run_api/codex_terminal_frame_is_fenced_before_stale_semantic_writes.rs"]
mod codex_terminal_frame_is_fenced_before_stale_semantic_writes;
#[path = "team_run_api/crashed_kimi_transport_requires_recovery_without_replaying_provider_effect.rs"]
mod crashed_kimi_transport_requires_recovery_without_replaying_provider_effect;
#[cfg(any())]
#[path = "team_run_api/external_interactive_member_joins_and_exchanges_mail.rs"]
mod external_interactive_member_joins_and_exchanges_mail;
#[path = "team_run_api/historical_wave_executor_kind_no_longer_controls_team_run_admission.rs"]
mod historical_wave_executor_kind_no_longer_controls_team_run_admission;
#[path = "team_run_api/host_can_explicitly_close_a_live_codex_member.rs"]
mod host_can_explicitly_close_a_live_codex_member;
#[path = "team_run_api/host_close_reports_bounded_store_contention_as_retryable_503.rs"]
mod host_close_reports_bounded_store_contention_as_retryable_503;
#[path = "team_run_api/idle_kimi_member_consumes_late_mail_on_the_same_native_session.rs"]
mod idle_kimi_member_consumes_late_mail_on_the_same_native_session;
#[path = "team_run_api/installed_kimi_upgrade_to_unreviewed_blocks_reopen_and_recovery_without_reusing_native_session.rs"]
mod installed_kimi_upgrade_to_unreviewed_blocks_reopen_and_recovery_without_reusing_native_session;
#[path = "team_run_api/interrupt_cancels_waiting_provider_message_before_kimi_prompt.rs"]
mod interrupt_cancels_waiting_provider_message_before_kimi_prompt;
#[path = "team_run_api/kimi_acp_member_can_be_cancelled_cooperatively.rs"]
mod kimi_acp_member_can_be_cancelled_cooperatively;
#[path = "team_run_api/kimi_empty_terminal_rounds_trip_the_bounded_circuit_and_real_output_resets_it.rs"]
mod kimi_empty_terminal_rounds_trip_the_bounded_circuit_and_real_output_resets_it;
#[path = "team_run_api/kimi_incomplete_stop_reason_requires_recovery_without_replay.rs"]
mod kimi_incomplete_stop_reason_requires_recovery_without_replay;
#[path = "team_run_api/kimi_model_switch_uses_only_the_new_models_advertised_effort_controls.rs"]
mod kimi_model_switch_uses_only_the_new_models_advertised_effort_controls;
#[path = "team_run_api/kimi_null_error_key_on_a_successful_response_is_not_a_provider_error.rs"]
mod kimi_null_error_key_on_a_successful_response_is_not_a_provider_error;
#[path = "team_run_api/kimi_prompt_rejected_before_any_prompt_update_never_burns_the_work.rs"]
mod kimi_prompt_rejected_before_any_prompt_update_never_burns_the_work;
#[path = "team_run_api/kimi_provider_error_after_receipt_requires_recovery_without_replay.rs"]
mod kimi_provider_error_after_receipt_requires_recovery_without_replay;
#[path = "team_run_api/kimi_quota_like_failure_requires_recovery_without_fabricating_capacity.rs"]
mod kimi_quota_like_failure_requires_recovery_without_fabricating_capacity;
#[path = "team_run_api/kimi_terminal_frame_is_fenced_before_stale_semantic_writes.rs"]
mod kimi_terminal_frame_is_fenced_before_stale_semantic_writes;
#[path = "team_run_api/mission_log_cli_and_legacy_wave_read_are_independent_of_team_run.rs"]
mod mission_log_cli_and_legacy_wave_read_are_independent_of_team_run;
#[path = "team_run_api/post_mission_and_retired_wave_write_routes.rs"]
mod post_mission_and_retired_wave_write_routes;
#[path = "team_run_api/post_mutation_response_is_bounded_and_dashboard_can_refresh_from_get_snapshot.rs"]
mod post_mutation_response_is_bounded_and_dashboard_can_refresh_from_get_snapshot;
#[path = "team_run_api/post_team_run_creates_entities_and_get_snapshot_projects_them.rs"]
mod post_team_run_creates_entities_and_get_snapshot_projects_them;
#[path = "team_run_api/post_team_run_message_and_start_async.rs"]
mod post_team_run_message_and_start_async;
#[path = "team_run_api/post_team_run_transition_and_compatibility_lineage.rs"]
mod post_team_run_transition_and_compatibility_lineage;
#[path = "team_run_api/retry_lineage_is_scoped_by_agent_team_not_retired_wave_identity.rs"]
mod retry_lineage_is_scoped_by_agent_team_not_retired_wave_identity;
#[path = "team_run_api/review_required_kimi_033_blocks_initial_start_and_http_work_rebind_before_acp.rs"]
mod review_required_kimi_033_blocks_initial_start_and_http_work_rebind_before_acp;
#[path = "team_run_api/reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session.rs"]
mod reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session;
#[path = "team_run_api/sse_invalidates_team_run_projection_and_snapshot_converges.rs"]
mod sse_invalidates_team_run_projection_and_snapshot_converges;
#[cfg(any())]
#[path = "team_run_api/stale_supervisor_quiesces_and_successor_resumes_mail_once.rs"]
mod stale_supervisor_quiesces_and_successor_resumes_mail_once;
#[path = "team_run_api/team_run_board_summary_is_bounded_and_reports_counts_and_member_state.rs"]
mod team_run_board_summary_is_bounded_and_reports_counts_and_member_state;
#[cfg(any())]
#[path = "team_run_api/team_run_cli_create_list_status_send_events.rs"]
mod team_run_cli_create_list_status_send_events;
#[cfg(any())]
#[path = "team_run_api/team_run_cli_message_reuses_conversation_lineage_only_within_its_run.rs"]
mod team_run_cli_message_reuses_conversation_lineage_only_within_its_run;
#[path = "team_run_api/team_run_dashboard_urls.rs"]
mod team_run_dashboard_urls;
#[path = "team_run_api/team_run_host_message_revision.rs"]
mod team_run_host_message_revision;
#[path = "team_run_api/team_run_host_message_send.rs"]
mod team_run_host_message_send;
#[path = "team_run_api/team_run_recover_prints_mission_log_tail_before_the_report.rs"]
mod team_run_recover_prints_mission_log_tail_before_the_report;
#[path = "team_run_api/two_peer_ack_only_mail_converges_without_extra_rounds_and_batches_on_next_trigger.rs"]
mod two_peer_ack_only_mail_converges_without_extra_rounds_and_batches_on_next_trigger;
#[path = "team_run_api/unauthenticated_team_member_inbox_http_route_is_retired.rs"]
mod unauthenticated_team_member_inbox_http_route_is_retired;
#[path = "team_run_api/work_list_brief_prints_one_stable_line_per_work_with_truncated_title.rs"]
mod work_list_brief_prints_one_stable_line_per_work_with_truncated_title;
#[path = "team_run_api/work_list_since_returns_only_works_changed_after_cursor.rs"]
mod work_list_since_returns_only_works_changed_after_cursor;
