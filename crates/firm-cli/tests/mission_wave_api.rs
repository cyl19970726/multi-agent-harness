//! End-to-end acceptance for the retired Mission/Wave legacy stack after the
//! DOC-108 legacy CompanyOS cutover: Mission and Wave writers are retired on
//! every surface (CLI, HTTP, MCP), while historical rows stay readable
//! through the read-only legacy CLI reads (`mission list|show|log show`,
//! `legacy wave list|show|history`) and the Stage A export/verify path.
//!
//! This deliberately exercises the public CLI and HTTP surfaces rather than
//! constructing core objects directly. Historical Mission/Wave rows are
//! seeded directly via `seed_historical_mission`, `seed_historical_mission_log`
//! and `seed_historical_wave` (the only way such rows can exist post-cutover)
//! so tests prove legacy reads and retired-write errors without making
//! Mission or Legacy Wave part of current TeamRun or Message identity.

use std::time::{Duration, Instant};

mod fake_provider;
mod firm_env;
use firm_env::{
    create_canonical_agent_member, current_project_id, member_run_for_work_owner, run_firm,
    run_firm_with_env, ServeHandle, TempHome,
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
/// without exercising a live write. Every field with `#[serde(default)]` is omitted;
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

/// Seed one historical Mission row directly, bypassing the retired `mission
/// create` write path (DOC-108): pre-cutover rows are the only Missions that
/// may exist, and the legacy reads must still serve them. Fields with
/// `#[serde(default)]` are omitted.
fn seed_historical_mission(home: &TempHome, project_id: &str, id: &str, title: &str) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(project_id).join("missions.jsonl");
    let mut ledger = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open mission ledger");
    writeln!(
        ledger,
        "{}",
        serde_json::json!({
            "id": id,
            "title": title,
            "objective": "Seeded pre-cutover row for legacy read coverage",
            "status": "planned",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical mission");
}

/// Seed one historical Mission Log row directly, bypassing the retired
/// `mission log append` write path (DOC-108), so the read-only `mission log
/// show` legacy read can be proven against pre-cutover history.
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

// Historical Mission/TeamRun/member-session umbrella. It seeds its world
// through the retired `mission create` and `company org actor` writers, whose
// authority DOC-108 closed; the retained-path contract it protected —
// TeamRun completion and Host close never tear down member runtimes — has
// executable coverage in team_run_api.rs (`...close...` tests) without any
// Mission or Company object.
#[path = "mission_wave_api/http_retired_routes.rs"]
mod http_retired_routes;
#[cfg(any())]
#[path = "mission_wave_api/mission_log_continuity.rs"]
mod mission_log_continuity;
#[path = "mission_wave_api/mission_log_history.rs"]
mod mission_log_history;
#[path = "mission_wave_api/node_daemon_delegation.rs"]
mod node_daemon_delegation;
#[path = "mission_wave_api/retired_writer_surfaces.rs"]
mod retired_writer_surfaces;
#[cfg(any())]
#[path = "mission_wave_api/retry_lineage_snapshot.rs"]
mod retry_lineage_snapshot;
