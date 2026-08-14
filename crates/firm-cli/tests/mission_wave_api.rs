//! End-to-end acceptance for the Mission control plane, including the
//! ADR 0051 Mission Log cutover: Mission absorbs Wave as an append-only
//! judgment log, Wave write commands (`create`/`update`/`advance`/`gate`)
//! retire on every surface (CLI, HTTP, MCP), and `wave list`/`show`/`history`
//! remain historical reads only.
//!
//! This deliberately exercises the public CLI and HTTP surfaces rather than
//! constructing core objects directly: Mission Log revisions, TeamRun retry
//! lineage, and snapshot projections must agree across the surfaces a Host
//! uses. Historical Wave rows are seeded directly via `seed_historical_wave`
//! (the only way a Wave can exist post-cutover) rather than through the
//! retired `wave create`, so tests can still prove reads and Wave-id
//! citation/validation keep working against pre-cutover data.

use std::time::{Duration, Instant};

mod fake_provider;
mod firm_env;
use firm_env::{
    create_canonical_agent_member, current_project_id, run_firm, run_firm_with_env, ServeHandle,
    TempHome,
};

const COMPANY_OS_TEST_TOKEN: &str = "mission-wave-company-os-test-capability";

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

fn run_json(home: &TempHome, project_id: &str, args: &[&str]) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(
        home,
        home.base(),
        &full,
        &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    );
    assert!(
        out.status.success(),
        "harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("harness {args:?} stdout was not JSON ({error})"))
}

fn run_member_json(
    home: &TempHome,
    project_id: &str,
    team_run_id: &str,
    member_run_id: &str,
    args: &[&str],
) -> serde_json::Value {
    let mut full = vec!["--project", project_id];
    full.extend_from_slice(args);
    let out = run_firm_with_env(
        home,
        home.base(),
        &full,
        &[
            ("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN),
            ("FIRM_TEAM_RUN_ID", team_run_id),
            ("FIRM_MEMBER_RUN_ID", member_run_id),
        ],
    );
    assert!(
        out.status.success(),
        "member harness {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|error| panic!("member harness {args:?} stdout was not JSON ({error})"))
}

#[cfg(any())]
fn force_team_run_reviewing(home: &TempHome, project_id: &str, run_id: &str, mission_id: &str) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(project_id).join("team_runs.jsonl");
    let mut row = std::fs::read_to_string(&path)
        .expect("read team run ledger")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .rfind(|candidate| candidate["id"].as_str() == Some(run_id))
        .expect("current TeamRun row");
    row["status"] = serde_json::json!("reviewing");
    row["updated_at"] = serde_json::json!("unix-ms:2");
    let _ = mission_id;
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open team run ledger");
    writeln!(ledger, "{row}").expect("append reviewing team run");
}

/// Seed one historical Wave row directly, bypassing the retired `wave
/// create` write path (ADR 0051), so tests can prove reads,
/// `source_plan_ref` navigation, and existing cross-Mission/executor-kind
/// validation still resolve a pre-cutover Wave row without exercising a
/// live write. Every field with `#[serde(default)]` is omitted;
/// `executor_kind`/`created_at`/`updated_at` have no default and are set
/// explicitly.
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

#[test]
fn host_plan_waves_keep_one_mission_team_and_member_sessions_alive() {
    let home = TempHome::new("host-plan-mission-team");
    let project_id = init_project(&home, "host-plan");

    for (id, name, role, provider) in [
        ("agent-build", "PrimaryBuilder", "primary builder", "codex"),
        ("agent-review", "ReviewPartner", "reviewer", "kimi"),
        ("agent-repair", "RepairFixer", "repair specialist", "codex"),
    ] {
        let created = create_canonical_agent_member(
            &home,
            home.base(),
            &project_id,
            id,
            name,
            role,
            provider,
            &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
        );
        assert!(
            created.status.success(),
            "canonical member create failed: {created:?}"
        );
    }
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-human",
            "--id",
            "human-owner",
            "--name",
            "Human Owner",
            "--responsibility",
            "Final company authority",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-agent",
            "--authority",
            "human-owner",
            "--id",
            "agent-review",
            "--agent-member",
            "agent-review",
            "--execution-space",
            &project_id,
            "--responsibility",
            "Same-id collision must remain unlinked",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-agent",
            "--authority",
            "human-owner",
            "--id",
            "agent-build",
            "--agent-member",
            "agent-build",
            "--execution-space",
            &project_id,
            "--responsibility",
            "Own persistent implementation work",
        ],
    );
    let mission = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-host-plan",
            "--title",
            "Ship host plan",
            "--objective",
            "Prove members can continue across plan revisions",
            "--context",
            "# Mission context\n\nKeep provider-native sessions.",
            "--json",
        ],
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
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-platform",
            "--name",
            "Platform Team",
            "--description",
            "Long-lived Mission team",
            "--mission-id",
            "mission-host-plan",
            "--host-agent-id",
            "agent-build",
            "--node-id",
            &node_id,
            "--member",
            "agent-build",
            "--member",
            "agent-review",
        ],
    );
    assert!(mission["context"]
        .as_str()
        .is_some_and(|context| context.contains("provider-native")));
    // Host plan judgment is now a Mission Log entry, not a Wave (ADR 0051).
    let judgment_1 = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "judgment",
            "--body",
            "# Baseline\n\nTwo lanes start; review may carry forward.",
            "--json",
        ],
    );
    assert_eq!(judgment_1["kind"].as_str(), Some("judgment"));
    assert_eq!(judgment_1["revision"].as_u64(), Some(1));

    let created = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "create",
            "--objective",
            "Work across Host plan revisions",
            "--agent-team-id",
            "team-platform",
            "--resume-member",
            "PrimaryBuilder:codex-session-1",
            "--member-owned-path",
            "PrimaryBuilder:crates",
            "--member-owned-path",
            "PrimaryBuilder:apps",
            "--json",
        ],
    );
    assert_eq!(
        created["team_run"]["agent_team_id"].as_str(),
        Some("team-platform")
    );
    assert_eq!(
        run_json(
            &home,
            &project_id,
            &["team", "show", "--id", "team-platform"]
        )["mission_id"]
            .as_str(),
        Some("mission-host-plan")
    );
    assert_eq!(
        created["member_runs"][0]["agent_member_id"].as_str(),
        Some("agent-build")
    );
    assert_eq!(
        created["member_runs"][0]["owned_paths"],
        serde_json::json!(["crates", "apps"]),
        "Mission-owned team identity survives member-owned-path overrides"
    );
    assert_eq!(
        created["member_runs"][1]["agent_member_id"].as_str(),
        Some("agent-review")
    );
    let snapshot = run_json(&home, &project_id, &["dashboard", "snapshot"]);
    let membership_projections = snapshot["company_os"]["agent_members"]
        .as_array()
        .expect("AgentMember membership projection");
    let projected_ids = membership_projections
        .iter()
        .filter_map(|member| member["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        projected_ids.is_superset(&std::collections::BTreeSet::from([
            "agent-build",
            "agent-review",
        ])),
        "Company projection must include the Team's canonical AgentMembers: {projected_ids:?}"
    );
    assert!(membership_projections.iter().all(|member| {
        member.get("member_run_id").is_none()
            && member.get("work_id").is_none()
            && member.get("native_session").is_none()
    }));
    assert!(created["team_run"]["wave_id"].is_null());
    let team_run_id = created["team_run"]["id"].as_str().unwrap();
    let builder_member_id = created["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        created["member_runs"][0]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1")
    );

    let judgment_2 = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "judgment",
            "--body",
            "Baseline lane is ready; review continues",
            "--json",
        ],
    );
    assert_eq!(judgment_2["revision"].as_u64(), Some(2));
    // wave-plan-2 is seeded as a historical row (not created -- `wave create`
    // is retired) purely so the source_plan_ref navigation check below has a
    // real pre-cutover Wave id to cite.
    seed_historical_wave(
        &home,
        &project_id,
        "wave-plan-2",
        "mission-host-plan",
        2,
        "host",
    );
    let replan = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "replan",
            "--body",
            "# Repair if needed\n\nIntegrate completed work while review continues; carry ReviewPartner forward and add RepairFixer.",
            "--json",
        ],
    );
    assert_eq!(replan["revision"].as_u64(), Some(3));
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "add-member",
            "--id",
            "team-platform",
            "--member",
            "agent-repair",
        ],
    );
    let joined = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "add-member",
            "--id",
            team_run_id,
            "--member",
            "agent-repair:repair specialist:codex",
            "--initial-work",
            "Repair any issue found by the review lane",
            "--origin-wave-id",
            "wave-plan-2",
        ],
    );
    assert!(joined["work"]["context_markdown"]
        .as_str()
        .is_some_and(|context| context.contains("wave-plan-2")));
    assert_eq!(
        joined["team_run"]["member_run_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let repair_member_id = joined["member_run"]["id"].as_str().unwrap().to_string();
    let renamed = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "rename-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--name",
            "TargetedRepair",
        ],
    );
    assert_eq!(renamed["name"].as_str(), Some("TargetedRepair"));
    let deactivated = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "deactivate-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--reason",
            "No reproducible defect remained after review",
        ],
    );
    assert_eq!(deactivated["status"].as_str(), Some("stopped"));

    let status = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    let builder = status["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["member_run"]["id"].as_str() == Some(&builder_member_id))
        .unwrap();
    assert_eq!(
        builder["member_run"]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1"),
        "Recording Mission Log judgment must not replace the ProviderRuntimeProjection or provider-native session"
    );

    // Explicit retry lineage cannot jump to another stable Team or Mission.
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-other",
            "--title",
            "Other Mission",
            "--objective",
            "Retry isolation fixture",
            "--json",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-other",
            "--name",
            "Other Team",
            "--description",
            "Retry isolation fixture",
            "--mission-id",
            "mission-other",
            "--host-agent-id",
            "agent-build",
            "--node-id",
            &node_id,
            "--member",
            "agent-build",
        ],
    );
    let cross_team_retry = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-team retry",
            "--agent-team-id",
            "team-other",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_team_retry.status.success());
    assert!(
        String::from_utf8_lossy(&cross_team_retry.stderr).contains("not for the same agent team")
    );

    // A seeded historical wave cannot revive the retired CLI message writer.
    // Mission/source-plan validation now belongs to canonical Message authoring.
    seed_historical_wave(&home, &project_id, "wave-other", "mission-other", 1, "host");
    let cross_mission_origin = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            team_run_id,
            "--from",
            "host",
            "--to",
            &builder_member_id,
            "--kind",
            "message",
            "--body",
            "invalid cross-Mission origin",
            "--origin-wave-id",
            "wave-other",
        ],
    );
    assert!(!cross_mission_origin.status.success());
    assert!(
        String::from_utf8_lossy(&cross_mission_origin.stderr).contains("RETIRED_WRITE_AUTHORITY")
    );
    // Mission is immutable Team metadata; callers cannot rebind a TeamRun by
    // supplying a different Mission at attempt creation.
    let cross_mission_retry = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-mission retry",
            "--agent-team-id",
            "team-platform",
            "--mission-id",
            "mission-other",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_mission_retry.status.success());
    assert!(String::from_utf8_lossy(&cross_mission_retry.stderr)
        .contains("mission_id and wave_id were removed"));

    let closeout = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "closeout_evidence",
            "--body",
            "Host plan complete",
            "--json",
        ],
    );
    assert_eq!(closeout["revision"].as_u64(), Some(4));
    // Mission close no longer requires a Wave gate (ADR 0051): this Mission's
    // wave_ids is empty (wave create is retired, and the historical rows
    // above were seeded directly, not through insert_wave_and_update_mission),
    // yet close still succeeds on its own outcome.
    let closed_mission = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "close",
            "--id",
            "mission-host-plan",
            "--outcome",
            "Mission completed while its Team history remains durable",
            "--json",
        ],
    );
    assert_eq!(closed_mission["wave_ids"], serde_json::json!([]));
    let team = run_json(
        &home,
        &project_id,
        &["team", "show", "--id", "team-platform"],
    );
    assert_eq!(team["status"].as_str(), Some("active"));
    let run = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    assert_eq!(run["team_run"]["status"].as_str(), Some("planning"));
    let cancelled = run_json(
        &home,
        &project_id,
        &["team-run", "cancel", "--id", team_run_id, "--json"],
    );
    assert_eq!(
        cancelled["status"].as_str(),
        Some("cancelled"),
        "Mission closeout must not prevent its TeamRun from settling"
    );
}

#[test]
fn mission_close_no_longer_gates_on_wave_and_wave_writes_are_retired_everywhere() {
    let home = TempHome::new("host-wave-gate");
    let project_id = init_project(&home, "host-wave");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-host",
            "--title",
            "Direct host work",
            "--objective",
            "Record an honest Host outcome",
            "--json",
        ],
    );

    // By convention the Host records closeout evidence in the Mission Log
    // before closing, but the store does not gate on it (ADR 0051: closeout
    // no longer depends on any Wave -- or Wave-equivalent -- acceptance).
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host",
            "--kind",
            "closeout_evidence",
            "--body",
            "Direct work verified without a fake executor run.",
            "--json",
        ],
    );
    let (status, body) = serve.post_json(
        "/v1/missions/mission-host/close",
        &serde_json::json!({
            "outcome": "Mission intent satisfied",
            "completed_by": "dashboard-host"
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));
    assert_eq!(
        body["result"]["completed_by"].as_str(),
        Some("dashboard-host")
    );
    assert!(body["result"]["completed_at"].is_string());
    assert_eq!(
        body["result"]["wave_ids"],
        serde_json::json!([]),
        "no Wave was ever created for this Mission -- wave create is retired"
    );

    // Identical closeout is idempotent; a conflicting actor/outcome is
    // rejected, exactly as before this cutover.
    let repeated = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "close",
            "--id",
            "mission-host",
            "--outcome",
            "Mission intent satisfied",
            "--completed-by",
            "dashboard-host",
            "--json",
        ],
    );
    assert_eq!(repeated["status"].as_str(), Some("completed"));
    let conflict = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "close",
            "--id",
            "mission-host",
            "--outcome",
            "different",
            "--completed-by",
            "another-host",
        ],
    );
    assert!(!conflict.status.success());

    // Wave write commands are retired on every surface (ADR 0051), regardless
    // of Mission state or whether the referenced Wave exists at all.
    for (command, extra) in [
        (
            "create",
            vec![
                "--mission-id",
                "mission-host",
                "--title",
                "Too late",
                "--objective",
                "Must be rejected",
            ],
        ),
        (
            "update",
            vec!["--id", "wave-does-not-exist", "--context", "x"],
        ),
        (
            "advance",
            vec!["--id", "wave-does-not-exist", "--outcome", "x"],
        ),
        (
            "gate",
            vec!["--id", "wave-does-not-exist", "--status", "accepted"],
        ),
    ] {
        let mut args = vec!["--project", project_id.as_str(), "wave", command];
        args.extend(extra);
        let out = run_firm(&home, home.base(), &args);
        assert!(!out.status.success(), "wave {command} must fail: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("retired") && stderr.contains("mission log append"),
            "wave {command} stderr: {stderr}"
        );
    }

    // ...and over HTTP, the same four routes.
    for (path, payload) in [
        (
            "/v1/waves",
            serde_json::json!({"mission_id": "mission-host", "title": "x", "objective": "y", "executor_kind": "host"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/context",
            serde_json::json!({"context": "x"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/advance",
            serde_json::json!({"outcome": "x"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/gate",
            serde_json::json!({"status": "accepted"}),
        ),
    ] {
        let (status, body) = serve.post_json(path, &payload);
        assert_eq!(status, 400, "{path} body: {body}");
        let error = body["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("retired") && error.contains("mission log append"),
            "{path} error: {error}"
        );
    }

    // Historical reads remain functional: seed one pre-cutover Wave row
    // directly (the only way a Wave can exist post-cutover) and prove
    // `wave list`/`show`/`history` still see it.
    seed_historical_wave(
        &home,
        &project_id,
        "wave-host-historical",
        "mission-host",
        1,
        "host",
    );
    let waves = run_json(
        &home,
        &project_id,
        &["wave", "list", "--mission-id", "mission-host"],
    );
    assert_eq!(waves.as_array().map(Vec::len), Some(1));
    let shown = run_json(
        &home,
        &project_id,
        &["wave", "show", "--id", "wave-host-historical"],
    );
    assert_eq!(shown["id"].as_str(), Some("wave-host-historical"));
    let history = run_json(
        &home,
        &project_id,
        &["wave", "history", "--id", "wave-host-historical"],
    );
    assert_eq!(history.as_array().map(Vec::len), Some(1));
}

// Historical Wave/TeamRun retry umbrella. Its middle section exercises the
// retired run-addressed HTTP message writer without a current NodeDaemon;
// current RoleAction/Message fabric and explicit 410 route inventory have
// independent executable coverage.
#[cfg(any())]
#[test]
fn mission_team_run_retry_lineage_wave_retirement_and_snapshot_contract() {
    let home = TempHome::new("mission-wave-api");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Public JSON parsing and domain validation reject malformed TeamRuns
    // before any run/member/message/event row is appended. Unaffected by
    // ADR 0051: `wave_index` was already retired compatibility, separately
    // from the Wave-write retirement this test now exercises below.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "obsolete wave index",
            "wave_index": 2,
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the attempt and provide evidence."}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("was retired"),
        "body: {body}"
    );

    for invalid in [
        serde_json::json!({
            "objective": "no executable member",
            "members": [],
        }),
        serde_json::json!({
            "objective": "incomplete native linkage",
            "mission_id": "mission-alpha",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the attempt and provide evidence."}],
        }),
    ] {
        let (status, body) = serve.post_json("/v1/team-runs", &invalid);
        assert_eq!(status, 400, "body: {body}");
    }
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(0));
    assert_eq!(snapshot["member_runs"].as_array().map(Vec::len), Some(0));
    assert_eq!(snapshot["team_messages"].as_array().map(Vec::len), Some(0));

    // HTTP authoring: a native Mission appears in the product snapshot; no
    // Goal or Task graph is created as a side effect. Wave no longer owns
    // execution attempts (ADR 0051): the Host records judgment as a Mission
    // Log entry instead, and nothing populates the Wave ledger for a fresh
    // Mission anymore.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-alpha",
            "title": "Ship agent team retry semantics",
            "objective": "Prove TeamRun retry lineage survives the Mission Log cutover",
            "desired_outcome": "A completed retry attempt",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["id"].as_str(), Some("mission-alpha"));
    let host = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "agent-alpha-host",
        "Alpha Host",
        "host",
        "codex",
        &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    );
    assert!(
        host.status.success(),
        "canonical host create failed: {host:?}"
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
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-alpha",
            "--name",
            "Alpha Team",
            "--description",
            "Flat retry Team",
            "--mission-id",
            "mission-alpha",
            "--host-agent-id",
            "agent-alpha-host",
            "--node-id",
            &node_id,
            "--member",
            "agent-alpha-host",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "judgment",
            "--body",
            "Two lanes will run concurrently; integration follows the first completed attempt.",
            "--json",
        ],
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["missions"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["waves"].as_array().map(Vec::len),
        Some(0),
        "wave create is retired: nothing populates this ledger for a new Mission"
    );
    assert_eq!(snapshot["mission_log"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        snapshot["mission_log"][0]["kind"].as_str(),
        Some("judgment")
    );

    // CLI list returns native Mission rows; wave_ids stays empty -- there is
    // no live write path left to populate it.
    let missions = run_json(&home, &project_id, &["mission", "list"]);
    let native = missions
        .as_array()
        .expect("mission list")
        .iter()
        .find(|mission| mission["id"].as_str() == Some("mission-alpha"))
        .expect("native mission");
    assert_eq!(native["wave_ids"], serde_json::json!([]));

    // Historical Wave rows remain readable (ADR 0051): seeded directly
    // (never through `wave create`, which is retired), they still project
    // through `wave list`/the snapshot in index order, and the existing
    // cross-Mission / executor-kind validation on TeamRun creation still
    // runs against them -- only the *write* path retired, not reads or the
    // validation logic that resolves a Wave id.
    seed_historical_wave(
        &home,
        &project_id,
        "wave-alpha",
        "mission-alpha",
        1,
        "agent_team",
    );
    seed_historical_wave(
        &home,
        &project_id,
        "wave-alpha-later",
        "mission-alpha",
        2,
        "agent_team",
    );
    let waves = run_json(
        &home,
        &project_id,
        &["wave", "list", "--mission-id", "mission-alpha"],
    );
    assert_eq!(
        waves
            .as_array()
            .unwrap()
            .iter()
            .map(|wave| wave["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2],
        "wave list still orders historical rows by index"
    );

    // Reject a TeamRun that tries to bind a historical Wave from another
    // Mission. The request must be atomic: no run is recorded.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({"id": "mission-beta", "title": "Other", "objective": "isolation"}),
    );
    assert_eq!(status, 200, "body: {body}");
    seed_historical_wave(
        &home,
        &project_id,
        "wave-beta",
        "mission-beta",
        1,
        "agent_team",
    );
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "invalid cross join",
            "mission_id": "mission-alpha",
            "wave_id": "wave-beta",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert_eq!(snapshot["team_runs"].as_array().map(Vec::len), Some(0));

    // A non-AgentTeam historical Wave cannot be used as an AgentTeamRun
    // executor target -- the boundary check is unaffected by whether the row
    // is fresh or historical.
    seed_historical_wave(&home, &project_id, "wave-host", "mission-alpha", 3, "host");
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "invalid executor",
            "wave_id": "wave-host",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");

    // Attempt A is cancelled. Attempt B is a retry via `previous_run_id`.
    // Mission-only (no wave_id) is the primary TeamRun creation path now
    // that Wave no longer owns execution attempts (ADR 0034/0051).
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "first attempt",
            "agent_team_id": "team-alpha",
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi", "initial_work": "Integrate the first attempt and submit evidence for Host review."}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let attempt_a = body["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(body["result"]["team_run"]["agent_team_id"], "team-alpha");
    assert!(body["result"]["team_run"]["wave_id"].is_null());
    assert!(body["result"]["team_run"].get("task_ids").is_none());
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "agent-alpha-host",
            "sender_kind": "host",
            "sender_id": "agent-alpha-host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "Please execute the assigned Work.",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let request_id = body["result"]["id"]
        .as_str()
        .expect("request id")
        .to_string();
    let conversation_correlation = body["result"]["correlation_id"]
        .as_str()
        .expect("conversation correlation")
        .to_string();

    // Work is the ownership path. Conversation correlation remains useful for
    // replies, but it does not create or transfer responsibility.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "operator-test",
            "sender_kind": "operator",
            "sender_id": "operator-test",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "implementation handoff",
            "correlation_id": conversation_correlation,
            "causation_id": request_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(conversation_correlation.as_str())
    );
    assert_eq!(
        body["result"]["causation_id"].as_str(),
        Some(request_id.as_str())
    );
    let handoff_id = body["result"]["id"]
        .as_str()
        .expect("handoff id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "accepted",
            "causation_id": handoff_id,
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["result"]["correlation_id"].as_str(),
        Some(conversation_correlation.as_str()),
        "causation-only reply inherits its cause correlation"
    );

    // Provider/member failure settles at reviewing; it can be explicitly
    // cancelled so a truthful retry can be created without marking the
    // failed attempt completed. Unrelated to any Wave gate now -- there is
    // no gate left to race.
    force_team_run_reviewing(&home, &project_id, &attempt_a, "mission-alpha");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_a}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("cancelled"));

    // Log-before-act (ADR 0051): the Host records why it is retrying before
    // creating the replacement attempt, not as after-the-fact narration.
    let replan = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "replan",
            "--body",
            "First attempt failed in review; retry with a fresh ProviderRuntimeProjection.",
            "--json",
        ],
    );
    assert_eq!(replan["revision"].as_u64(), Some(2));

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "replacement attempt",
            "agent_team_id": "team-alpha",
            "previous_run_id": attempt_a,
            "members": [{"name": "lead", "role": "integrator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let attempt_b = body["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        body["result"]["team_run"]["previous_run_id"].as_str(),
        Some(attempt_a.as_str())
    );

    force_team_run_reviewing(&home, &project_id, &attempt_b, "mission-alpha");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{attempt_b}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));

    // The Host records closeout evidence in the Mission Log instead of a
    // Wave gate accepting the retry -- an append-only log has nothing
    // analogous to a gate to accept, revise, or block (ADR 0051).
    let closeout = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-alpha",
            "--kind",
            "closeout_evidence",
            "--body",
            "Retry attempt completed and reviewed.",
            "--json",
        ],
    );
    assert_eq!(closeout["revision"].as_u64(), Some(3));
    let entries = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-alpha",
            "--json",
        ],
    );
    assert_eq!(
        entries
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    // The Wave gate that used to accept a retry attempt is retired on every
    // surface, regardless of which attempt or Wave id is named.
    let (status, body) = serve.post_json(
        "/v1/waves/wave-alpha/gate",
        &serde_json::json!({"status": "accepted", "run_id": attempt_b}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("retired"),
        "body: {body}"
    );
    let cli_gate = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "wave",
            "gate",
            "--id",
            "wave-alpha",
            "--status",
            "accepted",
            "--run-id",
            &attempt_b,
        ],
    );
    assert!(!cli_gate.status.success());
    assert!(String::from_utf8_lossy(&cli_gate.stderr).contains("retired"));

    // Historical reasoning remains in JSONL, but the new snapshot must not
    // project it as product state or evidence.
    use std::io::Write as _;
    let action_path = home
        .projects_dir()
        .join(&project_id)
        .join("member_actions.jsonl");
    let mut actions = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&action_path)
        .expect("open action ledger");
    writeln!(
        actions,
        "{}",
        serde_json::json!({
            "id": "legacy-thinking",
            "seq": 999,
            "team_run_id": attempt_b,
            "member_run_id": "legacy-member",
            "action_type": "thinking",
            "status": "succeeded",
            "title": "legacy reasoning",
            "summary": "must stay historical",
            "started_at": "unix-ms:1",
        })
    )
    .expect("append legacy thinking");
    assert!(std::fs::read_to_string(&action_path)
        .unwrap()
        .contains("legacy reasoning"));
    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200);
    assert!(
        snapshot["member_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["action_type"].as_str() != Some("thinking")),
        "thinking leaked into snapshot: {:?}",
        snapshot["member_actions"]
    );
}

#[test]
fn http_console_delegates_native_team_run_to_node_daemon() {
    let home = TempHome::new("mission-wave-console-start");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );

    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-console",
            "title": "Console-native Agent Team",
            "objective": "Run the complete Mission/TeamRun journey",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let host = create_canonical_agent_member(
        &home,
        home.base(),
        &project_id,
        "agent-console-host",
        "Console Host",
        "host",
        "codex",
        &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
    );
    assert!(
        host.status.success(),
        "canonical host create failed: {host:?}"
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
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-console",
            "--name",
            "Console Team",
            "--description",
            "Flat Console Team",
            "--mission-id",
            "mission-console",
            "--host-agent-id",
            "agent-console-host",
            "--node-id",
            &node_id,
            "--member",
            "agent-console-host",
        ],
    );
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Complete through the Console start endpoint",
            "agent_team_id": "team-console",
            "members": [{"name": "worker", "role": "implementer", "provider": "kimi", "initial_work": "Run the fake provider and return the requested evidence."}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let run_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = body["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let agent_member_id = body["result"]["member_runs"][0]["agent_member_id"]
        .as_str()
        .expect("canonical AgentMember id")
        .to_string();
    let work_id = body["result"]["works"][0]["id"]
        .as_str()
        .expect("Work id")
        .to_string();

    let daemon = run_firm_with_env(
        &home,
        home.base(),
        &["--project", &project_id, "daemon", "start"],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_RESULT", "done"),
        ],
    );
    assert!(
        daemon.status.success(),
        "start NodeDaemon failed: {}",
        String::from_utf8_lossy(&daemon.stderr)
    );
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({"max_concurrency": 1, "idle_timeout_s": 10}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("running"));
    assert_eq!(body["result"]["node_daemon"]["node_id"], node_id);

    // Repeated adoption is idempotent at the NodeDaemon boundary.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start?project={project_id}"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    assert_eq!(
        body["result"]["node_daemon"]["daemon_response"]["reused"],
        true
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
        assert_eq!(status, 200);
        let idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        let completed_turn = snapshot["member_actions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|action| {
                action["member_run_id"].as_str() == Some(member_id.as_str())
                    && action["action_type"].as_str() == Some("turn_completed")
            });
        if idle && completed_turn {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "member did not return to persistent idle: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    let started = run_member_json(
        &home,
        &project_id,
        &run_id,
        &member_id,
        &[
            "team-run",
            "work",
            "start",
            "--team-run-id",
            &run_id,
            "--work-id",
            &work_id,
            "--expected-version",
            "1",
            "--member-run-id",
            &member_id,
            "--json",
        ],
    );
    let started_version = started["version"].as_u64().expect("started version");
    let submitted_version = started_version + 1;
    let report_id = "report-console-result";
    let candidate = serde_json::json!({
        "kind": "content_digest",
        "value": "console-result-v1"
    });
    let candidate_fingerprint = harness_store::canonical_json_fingerprint(&candidate);
    let report_command = serde_json::json!({
        "command": "create_work_report",
        "team_id": "team-console",
        "report": {
            "id": report_id,
            "work_id": work_id,
            "work_revision": submitted_version,
            "report_revision": 1,
            "kind": "result",
            "authored_by": {"kind": "agent_member", "id": agent_member_id},
            "summary": "Host accepted the fake provider evidence",
            "base_revision": null,
            "candidate": candidate,
            "candidate_fingerprint": candidate_fingerprint,
            "finding_refs": [],
            "failure_analysis_ref": null,
            "artifact_refs": [],
            "check_refs": [],
            "evidence_refs": ["fake-provider-round"],
            "known_risks": [],
            "confidence": "high",
            "recommended_next_action": "accept",
            "created_at": "unix-ms:1"
        }
    })
    .to_string();
    run_json(
        &home,
        &project_id,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "agent_member",
            "--actor-id",
            &agent_member_id,
            "--idempotency-key",
            "console-work-report",
            "--expected-version",
            "0",
            "--json",
            &report_command,
        ],
    );
    let accept_command = serde_json::json!({
        "command": "accept_work",
        "team_id": "team-console",
        "work_id": work_id,
        "work_report_id": report_id,
        "candidate_fingerprint": candidate_fingerprint,
        "updated_at": "unix-ms:2"
    })
    .to_string();
    let accepted = run_json(
        &home,
        &project_id,
        &[
            "member-trust",
            "mutate",
            "--actor-kind",
            "human",
            "--actor-id",
            "host-console",
            "--idempotency-key",
            "console-work-accept",
            "--expected-version",
            &submitted_version.to_string(),
            "--json",
            &accept_command,
        ],
    );
    assert_eq!(accepted["projection"]["phase"].as_str(), Some("closed"));
    assert_eq!(
        accepted["projection"]["resolution"].as_str(),
        Some("accepted")
    );

    let (status, completed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/transition?project={project_id}"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {completed}");
    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
    assert_eq!(status, 200);
    assert!(
        snapshot["member_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["action_type"].as_str() != Some("thinking")),
        "thinking became durable: {}",
        snapshot["member_actions"]
    );
    assert!(
        !snapshot.to_string().contains("hidden reasoning"),
        "thinking leaked into snapshot"
    );

    // The legacy unscoped provider ingress is retired for every payload. Live
    // activity now enters only through the exact-AgentSession daemon bridge.
    let (status, body) = serve.post_json(
        &format!("/v1/live/member-activity?project={project_id}"),
        &serde_json::json!({
            "team_run_id": run_id,
            "member_run_id": member_id,
            "preview": "too late",
        }),
    );
    assert_eq!(status, 410, "body: {body}");
    assert_eq!(body["error"].as_str(), Some("retired_live_member_activity"));
    let stopped = run_firm(
        &home,
        home.base(),
        &["--project", &project_id, "daemon", "stop"],
    );
    assert!(
        stopped.status.success(),
        "stop NodeDaemon failed: {stopped:?}"
    );

    // The Host records closeout evidence in the Mission Log instead of a
    // Wave gate accepting the run (ADR 0051: an append-only log has nothing
    // analogous to a gate). `/v1/waves/{id}/gate` is retired regardless.
    let closeout = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-console",
            "--kind",
            "closeout_evidence",
            "--body",
            "Deterministic provider completed; run accepted.",
            "--json",
        ],
    );
    assert_eq!(closeout["kind"].as_str(), Some("closeout_evidence"));
    assert_eq!(closeout["revision"].as_u64(), Some(1));

    let (status, body) = serve.post_json(
        &format!("/v1/waves/wave-console/gate?project={project_id}"),
        &serde_json::json!({
            "status": "accepted",
            "run_id": run_id,
            "accepted_by": "console-host",
            "outcome": "deterministic provider completed",
            "artifact_refs": ["check:http-console"],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("retired"),
        "body: {body}"
    );
}

/// `mission log append`/`show` CLI happy path, focused purely on the Mission
/// Log surface itself (revision monotonicity, --tail, plain-text vs --json,
/// the "no mission log yet" sentinel, and CLI-side empty-body rejection) --
/// the other tests in this file exercise it woven into larger scenarios,
/// this one is the dedicated coverage the ADR 0051 cutover asked for.
#[test]
fn mission_log_cli_append_and_show_happy_path() {
    let home = TempHome::new("mission-log-cli-happy-path");
    let project_id = init_project(&home, "alpha");

    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-log-happy",
            "--title",
            "Mission Log happy path",
            "--objective",
            "Prove append/show end to end",
            "--json",
        ],
    );

    // A fresh Mission with no entries shows the explicit sentinel in text
    // mode, not an empty line or an error.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
        ],
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "no mission log yet"
    );

    // Empty (whitespace-only) body is rejected before anything is appended.
    let empty_body = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "judgment",
            "--body",
            "   ",
        ],
    );
    assert!(!empty_body.status.success());
    assert!(String::from_utf8_lossy(&empty_body.stderr).contains("body must not be empty"));

    // Unknown --kind is rejected with the exact accepted set named.
    let bad_kind = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "narration",
            "--body",
            "not a real kind",
        ],
    );
    assert!(!bad_kind.status.success());
    assert!(String::from_utf8_lossy(&bad_kind.stderr)
        .contains("judgment|replan|recovery|closeout_evidence"));

    for (kind, body, actor) in [
        ("judgment", "First judgment.", None),
        ("replan", "Re-planned after review.", Some("operator-a")),
        ("recovery", "Recovered after a supervisor death.", None),
        (
            "closeout_evidence",
            "Everything verified; closing.",
            Some("host"),
        ),
    ] {
        let mut args = vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            kind,
            "--body",
            body,
        ];
        if let Some(actor) = actor {
            args.push("--actor");
            args.push(actor);
        }
        args.push("--json");
        run_json(&home, &project_id, &args);
    }

    // --json show: full ordered history, correct kinds, default actor "host"
    // when --actor was not supplied.
    let all_json = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
            "--json",
        ],
    );
    let all_json = all_json.as_array().expect("entries array");
    assert_eq!(all_json.len(), 4);
    assert_eq!(
        all_json
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        all_json
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["judgment", "replan", "recovery", "closeout_evidence"]
    );
    assert_eq!(all_json[0]["actor"].as_str(), Some("host"));
    assert_eq!(all_json[1]["actor"].as_str(), Some("operator-a"));

    // --tail 2 in --json mode: last two only, oldest-of-the-tail first.
    let tail_json = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
            "--tail",
            "2",
            "--json",
        ],
    );
    let tail_json = tail_json.as_array().expect("tail entries array");
    assert_eq!(
        tail_json
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![3, 4]
    );

    // Plain-text show (no --json): every body appears, in revision order.
    let text_out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
        ],
    );
    assert!(text_out.status.success());
    let text = String::from_utf8_lossy(&text_out.stdout).to_string();
    let first_pos = text.find("First judgment.").expect("revision 1 body");
    let replan_pos = text
        .find("Re-planned after review.")
        .expect("revision 2 body");
    let recovery_pos = text
        .find("Recovered after a supervisor death.")
        .expect("revision 3 body");
    let closeout_pos = text
        .find("Everything verified; closing.")
        .expect("revision 4 body");
    assert!(
        first_pos < replan_pos && replan_pos < recovery_pos && recovery_pos < closeout_pos,
        "plain-text show must render entries in revision order: {text}"
    );
    assert!(text.contains("[judgment]"), "text: {text}");
    assert!(text.contains("[closeout_evidence]"), "text: {text}");

    // A non-JSON append prints a terse summary line, not the full entry.
    let terse_append = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "judgment",
            "--body",
            "Fifth entry, terse output.",
        ],
    );
    assert!(terse_append.status.success());
    let terse_stdout = String::from_utf8_lossy(&terse_append.stdout);
    assert_eq!(terse_stdout.trim(), "mission-log-happy\t#5\tjudgment");

    // Appending against a Mission that does not exist fails clearly instead
    // of silently creating an orphaned Log row.
    let missing_mission = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-does-not-exist",
            "--kind",
            "judgment",
            "--body",
            "orphan",
        ],
    );
    assert!(!missing_mission.status.success());
    assert!(String::from_utf8_lossy(&missing_mission.stderr).contains("mission not found"));
}

/// The console must be able to record Host judgment without a terminal:
/// `POST /v1/missions/{id}/log` appends through the same path as
/// `harness mission log append`, assigns store revisions, and rejects
/// unknown kinds with a usage error.
#[test]
fn http_mission_log_append_route_appends_and_rejects_unknown_kind() {
    let home = TempHome::new("mission-wave-http-log");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-log-http",
            "title": "Mission Log HTTP",
            "objective": "Append entries through the console route",
        }),
    );
    assert_eq!(status, 200, "body: {body}");

    let (status, body) = serve.post_json(
        "/v1/missions/mission-log-http/log",
        &serde_json::json!({
            "kind": "judgment",
            "body": "Advance from the console.",
            "actor": "operator",
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let entry = &body["result"];
    assert_eq!(entry["mission_id"], "mission-log-http");
    assert_eq!(entry["kind"], "judgment");
    assert_eq!(entry["actor"], "operator");
    assert!(
        entry["revision"].as_u64().unwrap_or(0) >= 1,
        "revision must be store-assigned: {entry}"
    );

    // A second append defaults the actor to host and advances the revision.
    let (status, body) = serve.post_json(
        "/v1/missions/mission-log-http/log",
        &serde_json::json!({ "kind": "replan", "body": "Replan from the console." }),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["actor"], "host");
    assert!(
        body["result"]["revision"].as_u64().unwrap_or(0) > entry["revision"].as_u64().unwrap_or(0),
        "revisions must be monotonic: {body}"
    );

    // Unknown kinds are usage errors (HTTP 400), mirroring the CLI.
    let (status, body) = serve.post_json(
        "/v1/missions/mission-log-http/log",
        &serde_json::json!({ "kind": "advance", "body": "nope" }),
    );
    assert_eq!(status, 400, "body: {body}");

    // Missing missions are rejected like the CLI (`mission not found`).
    let (status, body) = serve.post_json(
        "/v1/missions/mission-log-does-not-exist/log",
        &serde_json::json!({ "kind": "judgment", "body": "orphan" }),
    );
    assert_eq!(status, 400, "body: {body}");
}
