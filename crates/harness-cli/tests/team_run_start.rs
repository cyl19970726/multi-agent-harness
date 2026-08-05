//! Integration coverage for `harness team-run start` (Agent Team v0
//! orchestration): a fake `kimi acp` shim on PATH answers the ACP handshake
//! and streams canned `session/update` frames, so the full loop — member
//! threads, the ACP driver, ledger journaling, queued-delivery rounds — runs
//! deterministically against a temp HOME. No real kimi binary is invoked.

use std::path::Path;

mod fake_provider;
mod harness_env;

use harness_env::{current_project_id, latest_works, run_harness, work_deliveries, TempHome};

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
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", fake_result)
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env(
            "FAKE_KIMI_ENV_MARKER",
            home.base().join("kimi-collaboration.env"),
        )
        .env(
            "FAKE_CODEX_ENV_MARKER",
            home.base().join("codex-collaboration.env"),
        )
        .env(
            "FAKE_CODEX_NAME_MARKER",
            home.base().join("codex-thread-name.jsonl"),
        )
        .env(
            "FAKE_CODEX_PLAN_MARKER",
            home.base().join("codex-execution-driver.log"),
        )
        .env("FAKE_CODEX_AUTO_COMPLETE", "1")
        .env(
            "FAKE_CLAUDE_ENV_MARKER",
            home.base().join("claude-collaboration.env"),
        )
        .env_remove("KIMI_CODE_BIN")
        .output()
        .expect("run harness")
}

/// Read one store JSONL file with latest-wins-per-id projection, in append
/// order (mirrors the harness's own projections).
fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
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

/// Seed one historical Wave row directly, bypassing the retired `wave
/// create` write path (ADR 0051), so a TeamRun can still explicitly cite an
/// existing Wave id via `--wave-id` (that citation path is unaffected --
/// only Wave *write* commands retired).
fn seed_historical_wave(home: &TempHome, project_id: &str, id: &str, mission_id: &str) {
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
            "index": 1,
            "title": "Historical Wave",
            "objective": "Seeded pre-cutover row for read/navigation coverage",
            "executor_kind": "agent_team",
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        })
    )
    .expect("append historical wave");
}

/// Read an append-only ledger when that object class is optional for the
/// scenario. A Work-only provider round correctly creates no TeamMessage
/// ledger at all.
fn optional_store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
    if !path.exists() {
        return Vec::new();
    }
    store_rows(home, project_id, file)
}

fn assert_collaboration_env(
    home: &TempHome,
    provider: &str,
    project_id: &str,
    run_id: &str,
    member_id: &str,
) {
    let text = std::fs::read_to_string(home.base().join(format!("{provider}-collaboration.env")))
        .unwrap_or_else(|error| panic!("{provider} collaboration env missing: {error}"));
    let metadata_path = home.projects_dir().join(project_id).join("metadata.json");
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&metadata_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", metadata_path.display())),
    )
    .expect("project metadata JSON");
    let project_root = metadata["canonical_path"]
        .as_str()
        .expect("canonical_path in metadata");
    for expected in [
        format!("HARNESS_SPACE={project_id}"),
        format!("HARNESS_PROJECT_ID={project_id}"),
        format!("HARNESS_PROJECT={project_root}"),
        format!("HARNESS_TEAM_RUN_ID={run_id}"),
        format!("HARNESS_MEMBER_RUN_ID={member_id}"),
        "HARNESS_WORK_ID=".to_string(),
    ] {
        assert!(
            text.lines().any(|line| line.starts_with(&expected)),
            "{provider} missing {expected}: {text}"
        );
    }
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
            "lead:coordinator:kimi@docs#Coordinate the run and report focused checks",
            "--member",
            "worker-1:implementer:kimi@crates/a#Implement the scoped change and report focused checks",
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
fn team_run_start_leaves_kimi_members_idle_until_host_close() {
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
            "--max-concurrency",
            "1",
        ],
    );
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("team run {run_id}\trunning")),
        "summary line: {stdout}"
    );

    let expected_cwd = std::fs::canonicalize(home.base().join("alpha"))
        .expect("canonical project cwd")
        .display()
        .to_string();
    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(
        runs[0]["execution_root"].as_str(),
        Some(expected_cwd.as_str())
    );
    let store_root = home
        .projects_dir()
        .join(&project_id)
        .to_string_lossy()
        .to_string();
    assert_ne!(
        runs[0]["execution_root"].as_str(),
        Some(store_root.as_str()),
        "provider execution root must never be the centralized store root"
    );

    // A provider turn completed, but the persistent MemberRuns remain idle.
    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    assert_eq!(members.len(), 2, "members: {members:?}");
    for member in &members {
        assert_eq!(
            member["status"].as_str(),
            Some("idle"),
            "member: {member:?}"
        );
        let session = member["native_session"]["native_session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("native session written: {member:?}"));
        assert!(
            session.starts_with("session_fake_"),
            "shim session id: {session}"
        );
        assert!(
            member["finished_at"].is_null(),
            "idle runtime has no terminal finished_at: {member:?}"
        );
        assert!(
            member["last_event_at"].is_string(),
            "last_event_at set: {member:?}"
        );
        assert_eq!(
            member["workspace_snapshot"]["cwd"].as_str(),
            Some(expected_cwd.as_str()),
            "actual spawn cwd is durably snapshotted"
        );
        assert!(member["workspace_snapshot"]["instruction_roots"].is_array());
        assert!(member["workspace_snapshot"]["skill_roots"].is_array());
        for prohibited in [
            "config_contents",
            "credentials",
            "provider_transcript",
            "tool_stream",
            "thinking",
        ] {
            assert!(
                member["workspace_snapshot"].get(prohibited).is_none(),
                "snapshot must not persist {prohibited}: {member:?}"
            );
        }
    }

    // Work is the durable responsibility and WorkDelivery is the adapter
    // receipt. A terminal provider report does not fabricate a Handoff.
    let works = latest_works(&home, &project_id);
    assert_eq!(works.len(), 2, "initial Works: {works:?}");
    let deliveries = work_deliveries(&home, &project_id);
    assert_eq!(deliveries.len(), 2, "Work deliveries: {deliveries:?}");
    for delivery in &deliveries {
        assert_eq!(
            delivery["status"].as_str(),
            Some("provider_received"),
            "Work delivered: {delivery:?}"
        );
        assert!(member_ids.contains(
            &delivery["recipient_member_run_id"]
                .as_str()
                .expect("delivery recipient")
                .to_string()
        ));
    }
    let messages = optional_store_rows(&home, &project_id, "team_messages.jsonl");
    assert!(
        messages
            .iter()
            .all(|message| message["kind"].as_str() != Some("handoff")),
        "the adapter must not turn provider completion into a Handoff: {messages:?}"
    );

    // Harness keeps only the explicit round outcome. Provider progress, tool
    // activity, command details, and reasoning remain in Kimi's native session.
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    for member_id in &member_ids {
        let of_member: Vec<&str> = actions
            .iter()
            .filter(|a| a["member_run_id"].as_str() == Some(member_id))
            .filter_map(|a| a["action_type"].as_str())
            .collect();
        assert_eq!(
            of_member,
            vec!["turn_completed"],
            "coordination-only actions: {of_member:?}"
        );
    }
    assert!(
        !actions.iter().any(|action| {
            action["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("hidden reasoning"))
        }),
        "thinking text leaked into durable actions: {actions:?}"
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

    // Provider completion is not a TeamRun or Work acceptance decision.
    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"].as_str(), Some("running"));
    assert!(runs[0]["completed_at"].is_null(), "run: {:?}", runs[0]);
}

#[test]
fn kimi_can_send_work_linked_progress_after_first_acp_acceptance() {
    let home = TempHome::new("team-run-kimi-live-work-message");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let marker = home.base().join("kimi-live-work-message.txt");
    let (run_id, member_ids) = create_two_member_run(&home, &fake_bin, &project_id);
    let member_id = &member_ids[0];
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
            "--max-concurrency",
            "1",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_MESSAGE_DURING_TURN", "1")
        .env("FAKE_KIMI_MESSAGE_MARKER", &marker)
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .output()
        .expect("start fake kimi team");
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        std::fs::read_to_string(&marker)
            .expect("message command marker")
            .trim()
            .starts_with("tmsg-"),
        "member-authored Work-linked message must succeed during the ACP turn"
    );

    let deliveries = work_deliveries(&home, &project_id);
    let delivery = deliveries
        .iter()
        .find(|delivery| delivery["recipient_member_run_id"] == *member_id)
        .expect("member WorkDelivery");
    assert_eq!(delivery["status"], "provider_received");
    assert!(
        delivery["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.starts_with("kimi-acp-prompt:")),
        "WorkDelivery must be backed by the active ACP prompt: {delivery:?}"
    );

    let messages = optional_store_rows(&home, &project_id, "team_messages.jsonl");
    let progress = messages
        .iter()
        .filter(|message| message["kind"] == "message" && message["from_member_id"] == *member_id)
        .collect::<Vec<_>>();
    assert_eq!(
        progress.len(),
        1,
        "the member should author one explicit progress message: {progress:?}"
    );
    assert!(
        progress[0]["body"]
            .as_str()
            .is_some_and(|body| body.contains("explicit Work-linked update")),
        "the member-authored update is authoritative: {progress:?}"
    );
    assert!(
        progress[0]["work_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "conversation must link to the Work without owning it: {progress:?}"
    );
    assert!(progress[0]["causation_id"].is_null());
}

#[test]
fn kimi_concatenated_acp_report_persists_only_the_terminal_contract() {
    let home = TempHome::new("team-run-kimi-concatenated-report");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let (run_id, member_ids) = create_two_member_run(&home, &fake_bin, &project_id);
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
            "--max-concurrency",
            "2",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_CONCATENATED_REPORT", "1")
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .output()
        .expect("start fake kimi team");
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    for member_id in member_ids {
        let completed = actions
            .iter()
            .find(|action| {
                action["member_run_id"] == member_id && action["action_type"] == "turn_completed"
            })
            .expect("explicit member outcome");
        let body = completed["summary"].as_str().expect("outcome summary");
        assert!(
            body.contains("fake member finished round"),
            "the durable outcome must use the terminal report: {body:?}"
        );
        assert!(
            !body.contains("ordinary narration"),
            "interim ACP narration must remain provider-native: {body:?}"
        );
    }
}

#[test]
fn kimi_member_explicitly_resumes_provider_native_session() {
    let home = TempHome::new("team-run-kimi-native-resume");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Continue provider-owned work",
            "--member",
            "worker:implementer:kimi/acp:k2.5#Continue the provider-owned Work",
            "--resume-member",
            "worker:session_prior_native",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
    assert!(out.status.success(), "resume start failed: {out:?}");

    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    let member = members
        .iter()
        .find(|member| member["team_run_id"] == run_id)
        .unwrap();
    assert_eq!(
        member["native_session"]["native_session_id"],
        "session_prior_native"
    );
    assert_eq!(
        member["native_session"]["parent_native_session_id"],
        "session_prior_native"
    );
    assert_eq!(member["native_session"]["availability"], "available");
    assert_eq!(member["native_session"]["supports_resume"], true);
}

#[test]
#[ignore = "historical claude_cli Team path; new Team members require claude_agent_sdk"]
fn claude_member_uses_native_session_without_provider_activity_mirror() {
    let home = TempHome::new("team-run-claude-native");
    let project_id = init_project(&home, "alpha");
    let fake_bin =
        fake_provider::install_claude_team_shim(&home.base().join("fakebin-claude-team"));
    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Review native session contract",
            "--member",
            "reviewer:reviewer:claude/cli#Review the native session contract",
            "--resume-member",
            "reviewer:session_prior_claude",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
    assert!(out.status.success(), "Claude start failed: {out:?}");

    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    let member = members
        .iter()
        .find(|member| member["team_run_id"] == run_id)
        .unwrap();
    assert_eq!(member["status"], "completed");
    assert_eq!(
        member["native_session"]["native_session_id"],
        "session_prior_claude"
    );
    assert_eq!(
        member["native_session"]["parent_native_session_id"],
        "session_prior_claude"
    );
    assert_eq!(
        member["native_session"]["native_locator_kind"],
        "claude_project_session"
    );
    assert_collaboration_env(
        &home,
        "claude",
        &project_id,
        &run_id,
        member["id"].as_str().expect("claude id"),
    );
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    let member_actions: Vec<_> = actions
        .iter()
        .filter(|action| action["member_run_id"] == member["id"])
        .collect();
    assert_eq!(
        member_actions.len(),
        1,
        "only explicit outcome is durable: {member_actions:?}"
    );
    assert_eq!(member_actions[0]["action_type"], "completed");
    let store_root = home.spaces_dir().join(&project_id);
    for entry in std::fs::read_dir(store_root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("hidden claude reasoning"));
            assert!(!text.contains("provider-owned output"));
        }
    }
}

#[test]
#[ignore = "historical claude_cli Team path; new Team members require claude_agent_sdk"]
fn claude_failure_keeps_native_session_and_provider_error_without_mirroring_stream() {
    let home = TempHome::new("team-run-claude-native-failure");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_claude_failure_shim(
        &home.base().join("fakebin-claude-team-failure"),
    );
    let out = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Prove failed native session binding",
            "--member",
            "reviewer:reviewer:claude/cli#Prove the failed native session binding",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
        "failure is journaled, not a CLI crash: {out:?}"
    );

    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    let member = members
        .iter()
        .find(|member| member["team_run_id"] == run_id)
        .unwrap();
    assert_eq!(member["status"], "failed");
    assert_eq!(
        member["native_session"]["native_session_id"],
        "session_fake_claude_failed"
    );
    assert_eq!(member["native_session"]["availability"], "available");

    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    let failure = actions
        .iter()
        .find(|action| action["member_run_id"] == member["id"])
        .expect("failure action");
    assert_eq!(failure["action_type"], "error");
    assert!(
        failure["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("401 Invalid authentication credentials")),
        "provider failure is explicit: {failure:?}"
    );
    assert_eq!(
        store_rows(&home, &project_id, "team_runs.jsonl")[0]["status"],
        "reviewing"
    );
    assert!(!home
        .spaces_dir()
        .join(&project_id)
        .join("provider_sessions.jsonl")
        .exists());
}

#[test]
fn team_run_start_completes_mixed_codex_kimi_without_persisting_reasoning() {
    let home = TempHome::new("team-run-start-mixed-codex-kimi");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&fake_bin);

    let mission = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "create",
            "--title",
            "Mixed provider acceptance",
            "--objective",
            "Prove Codex and Kimi share one native TeamRun",
        ],
    );
    assert!(mission.status.success(), "mission: {mission:?}");
    let mission_id = String::from_utf8_lossy(&mission.stdout).trim().to_string();
    // `wave create` is retired (ADR 0051): seed a historical row directly so
    // TeamRun creation can still explicitly cite an existing Wave id.
    let wave_id = "wave-mixed-provider".to_string();
    seed_historical_wave(&home, &project_id, &wave_id, &mission_id);

    let create = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--mission-id",
            &mission_id,
            "--wave-id",
            &wave_id,
            "--objective",
            "Implement with Codex and perform a small Kimi review",
            "--member",
            "codex-worker:implementer:codex:gpt-5.6#Implement the scoped change and report checks",
            "--member",
            "kimi-reviewer:reviewer:kimi:k2.5#Review the scoped change and report findings",
        ],
    );
    assert!(
        create.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();
    let start = run_with_fake_kimi(
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
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );

    let members = store_rows(&home, &project_id, "member_runs.jsonl");
    assert_eq!(members.len(), 2, "members: {members:?}");
    assert!(members
        .iter()
        .all(|member| member["status"].as_str() == Some("idle")));
    let codex = members
        .iter()
        .find(|member| member["provider"].as_str() == Some("codex"))
        .expect("codex member");
    assert_eq!(codex["model"].as_str(), Some("gpt-5.6"));
    assert_eq!(
        codex["provider_profile"]["execution_mode"].as_str(),
        Some("codex_app_server")
    );
    assert_eq!(
        codex["provider_profile"]["execution_driver"].as_str(),
        Some("host_driven")
    );
    assert_eq!(
        codex["native_session"]["native_session_id"].as_str(),
        Some("thread_fake_codex_app_server")
    );
    let native_name = std::fs::read_to_string(home.base().join("codex-thread-name.jsonl"))
        .expect("Codex thread/name/set marker");
    assert!(
        native_name.contains("\"threadId\":\"thread_fake_codex_app_server\""),
        "thread/name/set targets the bound native thread: {native_name}"
    );
    assert!(
        native_name.contains("\"name\":\"Agent Team · codex-worker\""),
        "thread/name/set carries the Member identity: {native_name}"
    );
    let execution_driver = std::fs::read_to_string(home.base().join("codex-execution-driver.log"))
        .expect("Codex execution-driver marker");
    assert!(
        execution_driver
            .lines()
            .any(|line| line.starts_with("turn ")),
        "host-driven Codex must start an explicit mailbox turn: {execution_driver}"
    );
    assert!(
        !execution_driver.contains("goal_set"),
        "host-driven Codex must not activate an independent native Goal: {execution_driver}"
    );
    let kimi = members
        .iter()
        .find(|member| member["provider"].as_str() == Some("kimi"))
        .expect("kimi member");
    assert_eq!(kimi["model"].as_str(), Some("k2.5"));
    assert_eq!(
        kimi["provider_profile"]["execution_mode"].as_str(),
        Some("kimi_acp")
    );
    assert_eq!(
        kimi["provider_profile"]["execution_driver"].as_str(),
        Some("host_driven")
    );
    assert_eq!(
        kimi["provider_profile"]["interaction_mode"].as_str(),
        Some("pause_and_resume")
    );
    assert_eq!(
        kimi["provider_profile"]["provider_version"].as_str(),
        Some("0.0.0")
    );
    assert_collaboration_env(
        &home,
        "codex",
        &project_id,
        &run_id,
        codex["id"].as_str().expect("codex id"),
    );
    assert_collaboration_env(
        &home,
        "kimi",
        &project_id,
        &run_id,
        kimi["id"].as_str().expect("kimi id"),
    );

    let messages = optional_store_rows(&home, &project_id, "team_messages.jsonl");
    assert!(
        messages
            .iter()
            .all(|message| message["kind"].as_str() != Some("handoff")),
        "provider terminal reports must not become adapter-authored Handoffs: {messages:?}"
    );
    let works = latest_works(&home, &project_id);
    assert_eq!(works.len(), 2, "one initial Work per member: {works:?}");
    let deliveries = work_deliveries(&home, &project_id);
    assert_eq!(
        deliveries.len(),
        2,
        "one WorkDelivery per member: {deliveries:?}"
    );
    assert!(deliveries
        .iter()
        .all(|delivery| delivery["status"] == "provider_received"));

    // Neither provider's hidden reasoning may appear in any durable ledger.
    let store_root = home.spaces_dir().join(&project_id);
    for entry in std::fs::read_dir(&store_root).expect("read store") {
        let path = entry.expect("store entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read ledger");
        assert!(
            !text.contains("hidden codex reasoning") && !text.contains("hidden reasoning"),
            "reasoning leaked into {}",
            path.display()
        );
    }
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        actions.iter().all(|action| {
            action["provider_call_id"].is_null()
                && action["provider_status"].is_null()
                && action["semantic_status"].is_null()
                && action["action_type"].as_str() == Some("turn_completed")
        }),
        "provider activity must remain native while explicit outcomes stay durable: {actions:?}"
    );
    for member in [codex, kimi] {
        assert!(member["native_session"]["native_session_id"].is_string());
        assert_eq!(member["native_session"]["availability"], "available");
    }
}

#[test]
fn kimi_question_waits_for_lead_resolution_and_resumes_same_turn() {
    let home = TempHome::new("team-run-kimi-question");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let create = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Ask the Lead once",
            "--member",
            "kimi-worker:implementer:kimi:k2.5#Ask the Lead when the Work needs clarification",
        ],
    );
    assert!(
        create.status.success(),
        "create: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_ASK", "1")
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .spawn()
        .expect("spawn team run");

    let interaction_path = home
        .spaces_dir()
        .join(&project_id)
        .join("pending_interactions.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !interaction_path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        interaction_path.exists(),
        "Kimi request must create a pending interaction"
    );
    let interactions = store_rows(&home, &project_id, "pending_interactions.jsonl");
    let pending = interactions.first().expect("pending interaction");
    assert_eq!(pending["kind"].as_str(), Some("question"));
    assert_eq!(pending["route"].as_str(), Some("lead"));
    assert_eq!(pending["status"].as_str(), Some("pending"));
    let interaction_id = pending["id"].as_str().expect("interaction id");

    let unauthorized = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "resolve-interaction",
            "--id",
            &run_id,
            "--interaction-id",
            interaction_id,
            "--option-id",
            "q0_opt_0",
            "--resolved-by",
            "operator",
        ],
    );
    assert!(
        !unauthorized.status.success(),
        "operator must not impersonate Lead"
    );
    assert!(
        String::from_utf8_lossy(&unauthorized.stderr).contains("requires lead authority"),
        "unauthorized error: {}",
        String::from_utf8_lossy(&unauthorized.stderr)
    );

    let resolve = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "resolve-interaction",
            "--id",
            &run_id,
            "--interaction-id",
            interaction_id,
            "--option-id",
            "q0_opt_0",
            "--resolved-by",
            "lead",
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve: {}",
        String::from_utf8_lossy(&resolve.stderr)
    );
    let output = child.wait_with_output().expect("wait team run");
    assert!(
        output.status.success(),
        "start: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let interactions = store_rows(&home, &project_id, "pending_interactions.jsonl");
    assert_eq!(interactions[0]["status"].as_str(), Some("answered"));
    assert_eq!(
        interactions[0]["response_option_id"].as_str(),
        Some("q0_opt_0")
    );
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        actions.iter().any(|action| {
            action["action_type"].as_str() == Some("interaction_resolved")
                && action["summary"].as_str().is_some_and(|value| value.contains("answered"))
                && action["provider_call_id"].is_null()
        }),
        "PendingInteraction is authoritative; MemberAction records only the coordination resolution: {actions:?}"
    );
}

#[test]
fn kimi_full_access_tool_permissions_acknowledge_without_pending_interactions() {
    let home = TempHome::new("team-run-kimi-policy-interaction");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let create = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Request governed tool permission",
            "--member",
            "kimi-worker:implementer:kimi:k2.5#Request governed tool permission for this Work",
        ],
    );
    assert!(create.status.success(), "create failed: {create:?}");
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_ASK", "approval_twice")
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .spawn()
        .expect("spawn team run");

    let output = child.wait_with_output().expect("wait team run");
    assert!(output.status.success(), "start failed: {output:?}");

    let interactions = optional_store_rows(&home, &project_id, "pending_interactions.jsonl");
    assert!(
        interactions.is_empty(),
        "trusted full-access permission receipts are not unresolved product interactions: {interactions:?}"
    );
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    let controls = actions
        .iter()
        .filter(|action| action["action_type"].as_str() == Some("provider_control"))
        .collect::<Vec<_>>();
    assert_eq!(
        controls.len(),
        1,
        "repeated safe approvals converge to one bounded receipt per MemberRun: {actions:?}"
    );
    assert!(controls.iter().all(|action| {
        action["status"].as_str() == Some("succeeded")
            && action["title"].as_str() == Some("Kimi full-access tool permission acknowledged")
            && action["summary"].as_str().is_some_and(|summary| {
                summary.contains("safe allow option")
                    && !summary.contains("Run the requested command")
                    && !summary.contains("another requested command")
            })
            && action["provider_call_id"].is_null()
    }));
    assert!(actions.iter().all(|action| {
        !matches!(
            action["action_type"].as_str(),
            Some("waiting_for_approval" | "interaction_resolved")
        )
    }));
    let events = store_rows(&home, &project_id, "team_run_events.jsonl");
    assert!(events
        .iter()
        .all(|event| event["entity_type"].as_str() != Some("pending_interaction")));
}

fn assert_kimi_permission_request_fails_closed(
    test_name: &str,
    ask_mode: &str,
    expected_kind: &str,
    expected_route: &str,
    resolved_by: &str,
    option_id: Option<&str>,
) {
    let home = TempHome::new(test_name);
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let create = run_with_fake_kimi(
        &home,
        &fake_bin,
        "done",
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "Exercise a fail-closed Kimi permission request",
            "--member",
            "kimi-worker:implementer:kimi:k2.5#Wait for a real authority decision when policy cannot safely allow the request",
        ],
    );
    assert!(create.status.success(), "create failed: {create:?}");
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_ASK", ask_mode)
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .spawn()
        .expect("spawn team run");

    let interaction_path = home
        .spaces_dir()
        .join(&project_id)
        .join("pending_interactions.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !interaction_path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        interaction_path.exists(),
        "unsafe request must remain visibly pending"
    );
    let pending = store_rows(&home, &project_id, "pending_interactions.jsonl")
        .into_iter()
        .next()
        .expect("pending interaction");
    assert_eq!(pending["kind"].as_str(), Some(expected_kind));
    assert_eq!(pending["route"].as_str(), Some(expected_route));
    assert_eq!(pending["status"].as_str(), Some("pending"));
    let interaction_id = pending["id"].as_str().expect("interaction id");
    let waiting_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let members = store_rows(&home, &project_id, "member_runs.jsonl");
        if members[0]["status"].as_str() == Some("waiting") {
            break;
        }
        assert!(
            std::time::Instant::now() < waiting_deadline,
            "PendingInteraction must project MemberRun waiting; latest={members:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    let mut args = vec![
        "--project",
        &project_id,
        "team-run",
        "resolve-interaction",
        "--id",
        &run_id,
        "--interaction-id",
        interaction_id,
        "--resolved-by",
        resolved_by,
    ];
    if let Some(option_id) = option_id {
        args.extend(["--option-id", option_id]);
    } else {
        args.extend(["--response-text", "Human reviewed the unknown request"]);
    }
    let resolve = run_with_fake_kimi(&home, &fake_bin, "done", &args);
    assert!(
        resolve.status.success(),
        "resolve failed: {}",
        String::from_utf8_lossy(&resolve.stderr)
    );
    let output = child.wait_with_output().expect("wait team run");
    assert!(
        output.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resolved = store_rows(&home, &project_id, "pending_interactions.jsonl");
    assert_ne!(resolved[0]["status"].as_str(), Some("pending"));
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(actions
        .iter()
        .all(|action| action["action_type"].as_str() != Some("provider_control")));
}

#[test]
fn kimi_reject_only_tool_permission_fails_closed_to_policy() {
    assert_kimi_permission_request_fails_closed(
        "team-run-kimi-reject-only-permission",
        "approval_reject_only",
        "tool_approval",
        "policy",
        "policy",
        Some("tool_reject_once"),
    );
}

#[test]
fn kimi_unknown_permission_request_fails_closed_to_human() {
    assert_kimi_permission_request_fails_closed(
        "team-run-kimi-unknown-permission",
        "unknown",
        "unknown",
        "human",
        "human",
        None,
    );
}

#[test]
fn blocked_provider_outcome_leaves_member_idle_and_supervisor_can_reattach() {
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
        members.iter().all(|m| m["status"].as_str() == Some("idle")),
        "members stay idle after reporting blocked: {members:?}"
    );

    // A provider-authored `RESULT blocked` is a failed provider turn summary,
    // not an implicit Work transition. The member must call `work block`
    // explicitly for the shared board to enter Blocked.
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        actions.iter().any(|action| {
            action["action_type"].as_str() == Some("turn_completed")
                && action["status"].as_str() == Some("failed")
                && action["summary"]
                    .as_str()
                    .is_some_and(|summary| summary.contains("fake member finished round"))
        }),
        "blocked report remains an explicit failed provider turn: {actions:?}"
    );

    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(
        runs[0]["status"].as_str(),
        Some("running"),
        "provider outcomes do not decide TeamRun status: {runs:?}"
    );

    let reattach = run_with_fake_kimi(
        &home,
        &fake_bin,
        "completed",
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
        reattach.status.success(),
        "service recovery should reattach the unclosed MemberRun: {reattach:?}"
    );
    assert_eq!(
        store_rows(&home, &project_id, "team_runs.jsonl")[0]["status"].as_str(),
        Some("running")
    );

    // Seqs stay continuous on the blocked path too.
    let events = store_rows(&home, &project_id, "team_run_events.jsonl");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    let expected: Vec<u64> = (1..=events.len() as u64).collect();
    assert_eq!(seqs, expected, "event seqs continuous: {seqs:?}");
}
