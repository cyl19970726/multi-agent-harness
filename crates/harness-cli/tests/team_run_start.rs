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
        let session = member["native_session"]["native_session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("native session written: {member:?}"));
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
            vec!["completed"],
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

    // Run: terminal completed with completed_at.
    let runs = store_rows(&home, &project_id, "team_runs.jsonl");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"].as_str(), Some("completed"));
    assert!(runs[0]["completed_at"].is_string(), "run: {:?}", runs[0]);
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
            "worker:implementer:kimi/acp:k2.5",
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
            "reviewer:reviewer:claude/cli",
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
    let store_root = home.projects_dir().join(&project_id);
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
    let wave = run_harness(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "wave",
            "create",
            "--mission-id",
            &mission_id,
            "--title",
            "Mixed team",
            "--objective",
            "Have Codex implement and Kimi review",
            "--executor-kind",
            "agent_team",
        ],
    );
    assert!(wave.status.success(), "wave: {wave:?}");
    let wave_id = String::from_utf8_lossy(&wave.stdout).trim().to_string();

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
            "codex-worker:implementer:codex:gpt-5.6",
            "--member",
            "kimi-reviewer:reviewer:kimi:k2.5",
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
        .all(|member| member["status"].as_str() == Some("completed")));
    let codex = members
        .iter()
        .find(|member| member["provider"].as_str() == Some("codex"))
        .expect("codex member");
    assert_eq!(codex["model"].as_str(), Some("gpt-5.6"));
    assert_eq!(
        codex["provider_profile"]["execution_mode"].as_str(),
        Some("codex_exec")
    );
    assert_eq!(
        codex["native_session"]["native_session_id"].as_str(),
        Some("thread_fake_codex_team")
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
        kimi["provider_profile"]["interaction_mode"].as_str(),
        Some("pause_and_resume")
    );
    assert_eq!(
        kimi["provider_profile"]["provider_version"].as_str(),
        Some("0.0.0")
    );

    let messages = store_rows(&home, &project_id, "team_messages.jsonl");
    let handoffs: Vec<_> = messages
        .iter()
        .filter(|message| message["kind"].as_str() == Some("handoff"))
        .collect();
    assert_eq!(handoffs.len(), 2, "handoffs: {handoffs:?}");
    assert!(handoffs.iter().any(|message| message["body"]
        .as_str()
        .is_some_and(|body| body.contains("fake codex member"))));

    // Neither provider's hidden reasoning may appear in any durable ledger.
    let store_root = home.projects_dir().join(&project_id);
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
                && matches!(
                    action["action_type"].as_str(),
                    Some("completed" | "blocked" | "error")
                )
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
            "kimi-worker:implementer:kimi:k2.5",
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
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .spawn()
        .expect("spawn team run");

    let interaction_path = home
        .projects_dir()
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
fn kimi_tool_approval_requires_policy_authority_and_resumes_same_turn() {
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
            "kimi-worker:implementer:kimi:k2.5",
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
        .env("FAKE_KIMI_ASK", "approval")
        .env_remove("KIMI_CODE_BIN")
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .spawn()
        .expect("spawn team run");

    let interaction_path = home
        .projects_dir()
        .join(&project_id)
        .join("pending_interactions.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !interaction_path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let pending = store_rows(&home, &project_id, "pending_interactions.jsonl")
        .into_iter()
        .next()
        .expect("pending interaction");
    assert_eq!(pending["kind"].as_str(), Some("tool_approval"));
    assert_eq!(pending["route"].as_str(), Some("policy"));
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
            "tool_allow_once",
            "--resolved-by",
            "lead",
        ],
    );
    assert!(
        !unauthorized.status.success(),
        "Lead must not bypass policy"
    );
    assert!(String::from_utf8_lossy(&unauthorized.stderr).contains("requires policy authority"));

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
            "tool_allow_once",
            "--resolved-by",
            "policy",
        ],
    );
    assert!(
        resolve.status.success(),
        "policy resolve failed: {resolve:?}"
    );
    let output = child.wait_with_output().expect("wait team run");
    assert!(output.status.success(), "start failed: {output:?}");

    let interactions = store_rows(&home, &project_id, "pending_interactions.jsonl");
    assert_eq!(interactions[0]["status"].as_str(), Some("approved"));
    assert_eq!(interactions[0]["resolved_by"].as_str(), Some("policy"));
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(actions.iter().any(|action| {
        action["action_type"].as_str() == Some("interaction_resolved")
            && action["summary"]
                .as_str()
                .is_some_and(|value| value.contains("approved"))
            && action["provider_call_id"].is_null()
    }));
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

    let retry = run_with_fake_kimi(
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
    assert!(!retry.status.success(), "reviewing attempt restarted");
    assert!(
        String::from_utf8_lossy(&retry.stderr).contains("create a new attempt to retry"),
        "stderr: {}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(
        store_rows(&home, &project_id, "team_runs.jsonl")[0]["status"].as_str(),
        Some("reviewing")
    );

    // Seqs stay continuous on the blocked path too.
    let events = store_rows(&home, &project_id, "team_run_events.jsonl");
    let mut seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    seqs.sort_unstable();
    let expected: Vec<u64> = (1..=events.len() as u64).collect();
    assert_eq!(seqs, expected, "event seqs continuous: {seqs:?}");
}
