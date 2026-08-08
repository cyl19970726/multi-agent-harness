//! Deterministic integration test: Pi Agent Team member orchestration.
//!
//! A fake `pi` shim on PATH answers the pi RPC handshake and streams canned
//! `agent_start` → `turn_end` → `agent_settled` frames. The full Harness loop
//! runs against a temp HOME. No real pi binary is invoked.

use std::path::Path;

mod fake_provider;
mod firm_env;

use firm_env::{current_project_id, run_firm, TempHome};

/// Init a project and return its id.
fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    current_project_id(home)
}

/// Run harness with the fake pi shim on PATH and PI_BIN set.
fn run_with_fake_pi(
    home: &TempHome,
    fake_bin: &Path,
    result_word: &str,
    args: &[&str],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env("PATH", path)
        .env("PI_BIN", fake_bin.join("pi").to_string_lossy().to_string())
        .env("FAKE_PI_RESULT", result_word)
        .env("FAKE_PI_SUBMIT_WORK", "1")
        .env(
            "FAKE_PI_ARGS_MARKER",
            home.base()
                .join("pi-args.json")
                .to_string_lossy()
                .to_string(),
        )
        .env(
            "FAKE_PI_SESSION_DIR",
            home.base()
                .join("pi-sessions")
                .to_string_lossy()
                .to_string(),
        )
        .env(
            "FAKE_PI_CWD_MARKER",
            home.base().join("pi-cwd.txt").to_string_lossy().to_string(),
        )
        .env("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .output()
        .expect("run harness")
}

/// Read store JSONL rows (latest-wins-per-id projection).
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
        let id = row["id"]
            .as_str()
            .or_else(|| row["delivery_id"].as_str())
            .expect("row id or delivery_id")
            .to_string();
        ids.retain(|known| known != &id);
        ids.push(id.clone());
        by_id.insert(id, row);
    }
    ids.into_iter()
        .map(|id| by_id.remove(&id).unwrap())
        .collect()
}

fn wait_for_member_turns(
    home: &TempHome,
    project_id: &str,
    member_run_id: &str,
    expected: usize,
) -> Vec<serde_json::Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let path = home
        .spaces_dir()
        .join(project_id)
        .join("member_actions.jsonl");
    loop {
        if path.is_file() {
            let actions = store_rows(home, project_id, "member_actions.jsonl");
            let completed = actions
                .iter()
                .filter(|action| {
                    action["member_run_id"] == member_run_id
                        && action["action_type"] == "turn_completed"
                })
                .count();
            if completed >= expected {
                return actions;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {expected} completed Pi member turns in {}",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

#[test]
fn pi_rpc_team_member_completes_work_then_host_follow_up_without_disconnect() {
    let home = TempHome::new("pi-team-member-round");
    let project_id = init_project(&home, "pi-test");

    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &home.base().join("pi-sessions/fake-session.jsonl"),
        "DONE",
    );

    // Create the team run with a pi member directly
    let create_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &[
            "team-run",
            "create",
            "--objective",
            "Verify pi integration",
            "--member",
            "pi-member:reviewer:pi/pi_rpc:any-model@#Review the work and report",
        ],
    );
    assert!(
        create_out.status.success(),
        "team-run create failed: stderr={}",
        String::from_utf8_lossy(&create_out.stderr),
    );
    let run_id = String::from_utf8_lossy(&create_out.stdout)
        .trim()
        .to_string();
    assert!(
        run_id.starts_with("team-run-"),
        "expected team-run-* id, got: {run_id}"
    );

    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .find(|member| member["team_run_id"] == run_id)
        .expect("Pi MemberRun");
    let member_id = member["id"].as_str().expect("member id").to_string();
    let work_list_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &["team-run", "work", "list", "--team-run-id", &run_id],
    );
    assert!(work_list_out.status.success(), "list initial Work");
    let works: Vec<serde_json::Value> =
        serde_json::from_slice(&work_list_out.stdout).expect("Work list JSON");
    let work_id = works[0]["id"].as_str().expect("Work id").to_string();
    let message_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &[
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            &member_id,
            "--kind",
            "message",
            "--response-required",
            "--body",
            "Review the follow-up after the initial Work round",
        ],
    );
    assert!(
        message_out.status.success(),
        "queue Host follow-up failed: {}",
        String::from_utf8_lossy(&message_out.stderr)
    );

    // Start the team run — this exercises the full orchestration loop
    let start_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &[
            "team-run",
            "start",
            "--id",
            &run_id,
            "--max-concurrency",
            "1",
        ],
    );
    assert!(
        start_out.status.success(),
        "team-run start failed: stdout={} stderr={}",
        String::from_utf8_lossy(&start_out.stdout),
        String::from_utf8_lossy(&start_out.stderr),
    );

    let stdout = String::from_utf8_lossy(&start_out.stdout);
    assert!(
        stdout.contains(&format!("team run {run_id}\trunning")),
        "summary line should show running: {stdout}"
    );

    // Verify the fake shim was actually called (cwd recorded)
    let cwd_marker = home.base().join("pi-cwd.txt");
    assert!(
        cwd_marker.exists(),
        "fake pi shim should have been called and recorded cwd"
    );
    let recorded_cwd = std::fs::read_to_string(&cwd_marker).expect("read cwd marker");
    assert!(
        !recorded_cwd.trim().is_empty(),
        "recorded cwd should not be empty"
    );

    let args: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(home.base().join("pi-args.json")).expect("read Pi args"),
    )
    .expect("Pi args JSON");
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--thinking".to_string(), "off".to_string()]),
        "persistent Pi launch must force thinking off: {args:?}"
    );

    let actions = wait_for_member_turns(&home, &project_id, &member_id, 2);
    let member_actions = actions
        .iter()
        .filter(|action| action["member_run_id"] == member_id)
        .collect::<Vec<_>>();
    assert_eq!(
        member_actions
            .iter()
            .filter(|action| action["action_type"] == "turn_completed")
            .count(),
        2,
        "initial Work and Host follow-up must complete in two rounds: {member_actions:?}"
    );
    assert!(
        member_actions
            .iter()
            .all(|action| action["action_type"] != "disconnected"),
        "a follow-up must not re-complete the original WorkDelivery: {member_actions:?}"
    );

    let work_show_out = run_with_fake_pi(
        &home,
        &fake_bin,
        "DONE",
        &["team-run", "work", "show", "--work-id", &work_id],
    );
    assert!(work_show_out.status.success(), "show initial Work");
    let work_show: serde_json::Value =
        serde_json::from_slice(&work_show_out.stdout).expect("Work show JSON");
    let delivery = &work_show["deliveries"][0];
    assert_eq!(delivery["status"], "provider_received");
    assert!(
        delivery["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.ends_with(":round-1")),
        "initial Work must receive exactly the first-round receipt: {delivery:?}"
    );

    let messages = store_rows(&home, &project_id, "team_messages.jsonl");
    let follow_up = messages
        .iter()
        .find(|message| message["body"] == "Review the follow-up after the initial Work round")
        .expect("Host follow-up");
    assert_eq!(follow_up["deliveries"][0]["status"], "delivered");
    assert!(
        follow_up["deliveries"][0]["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.ends_with(":round-2")),
        "queued Host mail must receive the second-round provider receipt: {follow_up:?}"
    );

    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .find(|member| member["id"] == member_id)
        .expect("latest Pi MemberRun");
    assert_eq!(member["native_session"]["provider"], "pi");
    assert_eq!(member["native_session"]["execution_mode"], "pi_rpc");
    assert_eq!(
        member["native_session"]["native_session_id"],
        home.base()
            .join("pi-sessions/fake-session.jsonl")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn pi_rpc_provider_profile_validation() {
    let home = TempHome::new("pi-profile-validation");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &home.base().join("pi-sessions/fake.jsonl"),
        "DONE",
    );

    // Verify that pi_rpc is a valid mode for Agent Team
    let out = run_with_fake_pi(&home, &fake_bin, "DONE", &["member", "providers"]);
    assert!(out.status.success(), "member providers failed");

    let providers: Vec<serde_json::Value> =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("member providers JSON");
    let pi = providers
        .iter()
        .find(|p| p.get("provider").and_then(|v| v.as_str()) == Some("pi"))
        .expect("pi should be in provider list");
    let team_profile = pi.get("team_member_profile").expect("team_member_profile");
    assert_eq!(
        team_profile.get("execution_mode").and_then(|v| v.as_str()),
        Some("pi_rpc"),
        "pi team member mode should be pi_rpc"
    );
    assert_eq!(
        team_profile
            .get("supports_cancel")
            .and_then(|v| v.as_bool()),
        Some(true),
        "pi should support cancel"
    );
    assert_eq!(
        team_profile
            .get("supports_resume")
            .and_then(|v| v.as_bool()),
        Some(true),
        "pi should support resume"
    );
    assert_eq!(
        team_profile
            .get("ordinary_message_boundary")
            .and_then(|v| v.as_str()),
        Some("next_round")
    );
    assert_eq!(
        team_profile
            .get("interaction_mode")
            .and_then(|v| v.as_str()),
        Some("end_round_and_follow_up")
    );
    assert_eq!(
        team_profile
            .get("thinking_transient_only")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        team_profile
            .get("provider_version")
            .and_then(|v| v.as_str()),
        Some("0.83.0"),
        "the fake Pi must satisfy the same reviewed-version gate as a real persistent member"
    );
    assert_eq!(
        team_profile
            .get("compatibility_status")
            .and_then(|v| v.as_str()),
        Some("current")
    );
}
