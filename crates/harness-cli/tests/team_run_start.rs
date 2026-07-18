//! Integration coverage for `harness team-run start` (Agent Team v0
//! orchestration): a fake `kimi acp` shim on PATH answers the ACP handshake
//! and streams canned `session/update` frames, so the full loop — member
//! threads, the ACP driver, ledger journaling, queued-delivery rounds — runs
//! deterministically against a temp HOME. No real kimi binary is invoked.

use std::path::Path;

mod fake_provider;
mod harness_env;

use harness_env::{current_project_id, run_harness, TempHome};

/// `harness init` a project rooted at `<base>/<name>` and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    current_project_id(home)
}

/// Run `harness <args...>` with the fake kimi dir prepended to PATH (so
/// `resolve_kimi_bin` resolves the shim) and `FAKE_KIMI_RESULT` pinning the
/// shim's `## RESULT` word. KIMI_CODE_BIN is removed so the PATH branch of
/// the resolver is the one under test.
fn run_with_fake_kimi(
    home: &TempHome,
    fake_bin: &Path,
    fake_result: &str,
    args: &[&str],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", fake_result)
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .output()
        .expect("run harness")
}

/// Read one store JSONL file with latest-wins-per-id projection, in append
/// order (mirrors the harness's own projections).
fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.projects_dir().join(project_id).join(file);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut ids: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(trimmed).unwrap_or_else(|e| panic!("{file} row not JSON: {e}"));
        let id = row["id"].as_str().expect("row id").to_string();
        ids.retain(|known| known != &id);
        ids.push(id.clone());
        by_id.insert(id, row);
    }
    ids.into_iter()
        .map(|id| by_id.remove(&id).unwrap())
        .collect()
}

/// Create a run with two kimi members and return (run_id, member ids).
fn create_two_member_run(
    home: &TempHome,
    fake_bin: &Path,
    project_id: &str,
) -> (String, Vec<String>) {
    let out = run_with_fake_kimi(
        home,
        fake_bin,
        "done",
        &[
            "--project",
            project_id,
            "team-run",
            "create",
            "--objective",
            "Ship v0",
            "--member",
            "lead:coordinator:kimi@docs",
            "--member",
            "worker-1:implementer:kimi@crates/a",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(run_id.starts_with("team-run-"), "run id: {run_id}");
    let members = store_rows(home, project_id, "member_runs.jsonl");
    let member_ids: Vec<String> = members
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(member_ids.len(), 2, "members: {member_ids:?}");
    (run_id, member_ids)
}

#[test]
fn team_run_start_completes_kimi_members() {
    let home = TempHome::new("team-run-start-done");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let (run_id, member_ids) = create_two_member_run(&home, &fake_bin, &project_id);

    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("team run {run_id}\tcompleted")),
        "summary line: {stdout}"
    );

    // Member runs: terminal completed, ACP session id written back, finished.
    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    assert_eq!(members.len(), 2, "members: {members:?}");
    for member in &members {
        assert_eq!(
            member["status"].as_str(),
            Some("completed"),
            "member: {member:?}"
        );
        let session = member["acp_session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("acp_session_id written: {member:?}"));
        assert!(
            session.starts_with("session_fake_"),
            "shim session id: {session}"
        );
        assert!(
            member["finished_at"].is_string(),
            "finished_at set: {member:?}"
        );
        assert!(
            member["last_event_at"].is_string(),
            "last_event_at set: {member:?}"
        );
    }

    // Messages: the two assignments are delivered, and each member handed off
    // to the host with a manual_ack/delivered delivery.
    let messages = store_rows(&home, &project_id, "team_messages.jsonl");
    let assignments: Vec<_> = messages
        .iter()
        .filter(|m| m["kind"].as_str() == Some("assignment"))
        .collect();
    assert_eq!(assignments.len(), 2, "assignments: {messages:?}");
    for assignment in &assignments {
        assert_eq!(
            assignment["deliveries"][0]["status"].as_str(),
            Some("delivered"),
            "assignment delivered: {assignment:?}"
        );
    }
    let handoffs: Vec<_> = messages
        .iter()
        .filter(|m| m["kind"].as_str() == Some("handoff"))
        .collect();
    assert_eq!(handoffs.len(), 2, "handoffs: {messages:?}");
    for handoff in &handoffs {
        assert!(
            member_ids.contains(&handoff["from_member_id"].as_str().unwrap().to_string()),
            "handoff from a member: {handoff:?}"
        );
        assert_eq!(
            handoff["to_member_ids"],
            serde_json::json!(["host"]),
            "handoff to host: {handoff:?}"
        );
        let delivery = &handoff["deliveries"][0];
        assert_eq!(delivery["member_id"].as_str(), Some("host"));
        assert_eq!(delivery["policy"].as_str(), Some("manual_ack"));
        assert_eq!(delivery["status"].as_str(), Some("delivered"));
        assert_eq!(delivery["attempt"].as_u64(), Some(1));
        let body = handoff["body"].as_str().unwrap_or_default();
        assert!(body.contains("## RESULT"), "handoff carries report: {body}");
    }

    // Actions: per member a tool_started + tool_completed + completed, plus
    // throttled progress; hidden reasoning never journaled.
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    for member_id in &member_ids {
        let of_member: Vec<&str> = actions
            .iter()
            .filter(|a| a["member_run_id"].as_str() == Some(member_id))
            .filter_map(|a| a["action_type"].as_str())
            .collect();
        for expected in ["progress", "tool_started", "tool_completed", "completed"] {
            assert!(
                of_member.contains(&expected),
                "member {member_id} missing action {expected}: {of_member:?}"
            );
        }
        // Reasoning streams are journaled as `thinking` actions (not dropped).
        assert!(
            of_member.contains(&"thinking"),
            "member {member_id} missing thinking action: {of_member:?}"
        );
    }
    // The round-end thinking action carries the full reasoning text.
    let thinking_full = actions.iter().any(|action| {
        action["action_type"].as_str() == Some("thinking")
            && action["summary"]
                .as_str()
                .is_some_and(|s| s.contains("hidden reasoning"))
    });
    assert!(
        thinking_full,
        "expected a thinking action carrying the reasoning text: {actions:?}"
    );

    // Events: seq strictly continuous 1..=N for the run.
    let events = store_rows(&home, &project_id, "team_run_events.jsonl");
    assert!(events.len() > 10, "orchestration folded events: {events:?}");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    let expected: Vec<u64> = (1..=events.len() as u64).collect();
    assert_eq!(seqs, expected, "event seqs continuous: {seqs:?}");
    assert!(
        events
            .iter()
            .all(|e| e["team_run_id"].as_str() == Some(run_id.as_str())),
        "all events belong to the run"
    );

    // Run: terminal completed with completed_at.
    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"].as_str(), Some("completed"));
    assert!(runs[0]["completed_at"].is_string(), "run: {:?}", runs[0]);
}

#[test]
fn team_run_start_blocked_member_sends_run_to_reviewing() {
    let home = TempHome::new("team-run-start-blocked");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let (run_id, _member_ids) = create_two_member_run(&home, &fake_bin, &project_id);

    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "blocked",
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
    );
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    assert!(
        members
            .iter()
            .all(|m| m["status"].as_str() == Some("blocked")),
        "members blocked: {members:?}"
    );

    // A blocked member journals a blocked action (the review signal).
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        actions
            .iter()
            .any(|a| a["action_type"].as_str() == Some("blocked")),
        "blocked action journaled: {actions:?}"
    );

    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(
        runs[0]["status"].as_str(),
        Some("reviewing"),
        "run reviewing: {runs:?}"
    );

    // Seqs stay continuous on the blocked path too.
    let events = store_rows(&home, &project_id, "team_run_events.jsonl");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    let expected: Vec<u64> = (1..=events.len() as u64).collect();
    assert_eq!(seqs, expected, "event seqs continuous: {seqs:?}");
}
