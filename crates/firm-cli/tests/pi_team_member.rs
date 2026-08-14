//! Deterministic integration test: Pi Agent Team member orchestration.
//!
//! A fake `pi` shim on PATH answers the pi RPC handshake and streams canned
//! `agent_start` → `turn_end` → `agent_settled` frames. The full Harness loop
//! runs against a temp HOME. No real pi binary is invoked.
//!
//! Journey coverage (canonical Message/Delivery path):
//!   - initial Work completes with the first-round receipt; a canonical Host
//!     follow-up completes on the SAME native session with the second-round
//!     receipt; exactly two `turn_completed`, no disconnect;
//!   - the permission ceiling is compiled into the spawn argv (`--tools`),
//!     and thinking is forced off;
//!   - an ordinary Message stays in the Harness queue while the member is
//!     busy (never compiled to steer), and only an explicit Steer control
//!     command compiles into native current-cycle injection (DOC-89 §13.1).

use std::path::Path;
use std::time::Duration;

mod fake_provider;
mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, run_firm, run_firm_with_env, ServeHandle,
    TempHome,
};

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
        .output()
        .expect("run harness")
}

/// `harness init` a project and seed the mandatory flat AgentTeam runtime
/// relation (Host + Mission + Node registration + Team).
fn init_pi_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    let project_id = current_project_id(home);

    let run = |args: &[&str]| {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend_from_slice(args);
        let out = run_firm_with_env(home, home.base(), &full, &[]);
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
        &project_id,
        "agent-pi-host",
        "Pi Fixture Host",
        "host",
        "kimi",
        &[],
    );
    assert!(
        host.status.success(),
        "canonical fixture Host failed: {}",
        String::from_utf8_lossy(&host.stderr)
    );
    run(&[
        "mission",
        "create",
        "--id",
        "mission-pi-fixture",
        "--title",
        "Pi Journey Fixture",
        "--objective",
        "Preserve Pi member journey contracts",
        "--json",
    ]);
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
        &project_id,
        "--project-binding-id",
        &project_id,
    ]);
    run(&[
        "team",
        "create",
        "--id",
        "team-pi-fixture",
        "--name",
        "Pi Fixture Team",
        "--description",
        "Flat Team for Pi member integration scenarios",
        "--mission-id",
        "mission-pi-fixture",
        "--host-agent-id",
        "agent-pi-host",
    ]);
    project_id
}

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

/// Poll a predicate over the HTTP snapshot with a bounded deadline.
fn poll_snapshot<F>(serve: &ServeHandle, what: &str, mut predicate: F) -> serde_json::Value
where
    F: FnMut(&serde_json::Value) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn member_turn_count(snapshot: &serde_json::Value, member_id: &str) -> usize {
    snapshot["member_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|action| {
            action["member_run_id"].as_str() == Some(member_id)
                && action["action_type"].as_str() == Some("turn_completed")
        })
        .count()
}

fn member_snapshot<'a>(
    snapshot: &'a serde_json::Value,
    member_id: &str,
) -> Option<&'a serde_json::Value> {
    snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|member| member["id"].as_str() == Some(member_id))
}

/// Two-round journey over the canonical Message/Delivery path: initial Work
/// settles with the round-1 receipt; a canonical Host follow-up completes on
/// the same native session with the round-2 receipt. This is the replacement
/// for the retired CLI-send journey.
#[test]
fn pi_rpc_member_two_round_journey_via_canonical_message() {
    let home = TempHome::new("pi-two-round-journey");
    let project_id = init_pi_project(&home, "pi-journey");
    let session_file = home.base().join("pi-sessions/fake-session.jsonl");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &session_file,
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let args_marker = home.base().join("pi-args.json");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_RESULT", "DONE"),
            ("FAKE_PI_SUBMIT_WORK", "1"),
            ("FAKE_PI_ARGS_MARKER", args_marker.to_str().unwrap()),
        ],
    );

    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi two-round canonical journey",
            "members": [{"name": "pi-worker", "role": "reviewer", "provider": "pi", "initial_work": "Review the work and report"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);

    // Round 1: the initial Work completes and the member goes idle on its
    // native session.
    let snapshot = poll_snapshot(&serve, "first Pi round", |snapshot| {
        member_turn_count(snapshot, &member_id) >= 1
            && member_snapshot(snapshot, &member_id).is_some_and(|member| {
                member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"]
                        .as_str()
                        .is_some()
            })
    });
    let first_session = member_snapshot(&snapshot, &member_id)
        .and_then(|member| member["native_session"]["native_session_id"].as_str())
        .map(str::to_string)
        .expect("first-round native session");

    // Canonical Host follow-up (response_required by sender-aware default).
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "Review the follow-up after the initial Work round",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let message_id = sent["result"]["id"].as_str().unwrap().to_string();

    // Round 2: same native session, message acknowledged after exactly one
    // attempt, two completed turns overall.
    poll_snapshot(
        &serve,
        "second Pi round on the same native session",
        |snapshot| {
            let delivered = snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|message| message["id"].as_str() == Some(message_id.as_str()))
                .is_some_and(|message| {
                    message["deliveries"][0]["status"].as_str() == Some("acknowledged")
                        && message["deliveries"][0]["attempt"].as_u64() == Some(1)
                });
            let same_session = member_snapshot(snapshot, &member_id).is_some_and(|member| {
                member["status"].as_str() == Some("idle")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some(first_session.as_str())
            });
            delivered && same_session && member_turn_count(snapshot, &member_id) >= 2
        },
    );

    // Exactly two completed rounds and never a disconnect.
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
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
        "a follow-up must not re-complete the original delivery: {member_actions:?}"
    );

    // The Work delivery carries exactly the round-1 receipt. Read the
    // canonical per-binding WorkDelivery — the legacy per-member-run
    // delivery of the same Work may read `invalidated` (WORK_REVISION_STALE)
    // by design once the member advanced the Work revision natively.
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let space_id = firm_env::current_space_id(&home);
    let canonical_deliveries = store
        .fabric_work_deliveries(&space_id)
        .expect("canonical work deliveries");
    let canonical = canonical_deliveries
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .expect("delivery json")
        .into_iter()
        .find(|delivery| {
            delivery["status"] == serde_json::Value::String("provider_received".into())
        })
        .expect("a provider_received canonical WorkDelivery");
    assert!(
        canonical["provider_receipt_id"]
            .as_str()
            .is_some_and(|receipt| receipt.ends_with(":round-1")),
        "initial Work must receive exactly the first-round receipt: {canonical:?}"
    );

    let canonical_messages = store
        .fabric_messages(&space_id)
        .expect("canonical messages");
    let canonical_message = canonical_messages
        .iter()
        .find(|message| message.body == "Review the follow-up after the initial Work round")
        .expect("canonical Host follow-up");
    let message_deliveries = store
        .fabric_message_deliveries(&space_id)
        .expect("canonical message deliveries");
    let follow_up_delivery = message_deliveries
        .iter()
        .find(|delivery| delivery.message_id == canonical_message.id)
        .expect("canonical follow-up delivery");
    assert!(
        follow_up_delivery
            .provider_receipt_id
            .as_deref()
            .is_some_and(|receipt| receipt.ends_with(":round-2")),
        "queued Host mail must receive the second-round provider receipt: {follow_up_delivery:?}"
    );

    // Launch contract: thinking forced off AND the workspace_write ceiling is
    // really compiled into the process argv (no bash under workspace-write).
    let args: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&args_marker).expect("read Pi args marker"))
            .expect("Pi args JSON");
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--thinking".to_string(), "off".to_string()]),
        "persistent Pi launch must force thinking off: {args:?}"
    );
    assert!(
        args.windows(2).any(|pair| pair
            == [
                "--tools".to_string(),
                "read,grep,find,ls,write,edit".to_string()
            ]),
        "workspace_write ceiling must compile to a --tools allowlist without bash: {args:?}"
    );

    // Native session binding.
    let member = store_rows(&home, &project_id, "member_runs.jsonl")
        .into_iter()
        .find(|member| member["id"] == member_id)
        .expect("latest Pi ProviderRuntimeProjection");
    assert_eq!(member["native_session"]["provider"], "pi");
    assert_eq!(member["native_session"]["execution_mode"], "pi_rpc");
    assert_eq!(
        member["native_session"]["native_session_id"],
        session_file.to_string_lossy().as_ref()
    );
}

/// DOC-89 §13.1 both arms: while the member is busy, an ordinary Message
/// stays in the durable Harness queue (never compiled into native injection);
/// only an explicit Steer control command compiles into current-cycle
/// injection — and the queued ordinary message then arrives as the NEXT round.
#[test]
fn pi_member_busy_queue_and_explicit_steer_conformance() {
    let home = TempHome::new("pi-steer-conformance");
    init_pi_project(&home, "pi-steer");
    let session_file = home.base().join("pi-sessions/fake-session.jsonl");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-cwd.txt"),
        &session_file,
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let prompt_marker = home.base().join("pi-prompts.txt");
    let steer_marker = home.base().join("pi-steers.txt");
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_RESULT", "DONE"),
            ("FAKE_PI_WAIT_FOR_STEER", "1"),
            ("FAKE_PI_PROMPT_MARKER", prompt_marker.to_str().unwrap()),
            ("FAKE_PI_STEER_MARKER", steer_marker.to_str().unwrap()),
        ],
    );

    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi steer conformance",
            "members": [{"name": "pi-worker", "role": "reviewer", "provider": "pi", "initial_work": "Hold the first cycle open"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202);

    // Wait until the shim is inside the first cycle.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !prompt_marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "shim never entered the first cycle"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    // Arm 1: an ordinary Message while busy must NOT compile into native
    // injection — it stays in the Harness queue.
    let (status, sent) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_id],
            "kind": "message",
            "body": "ordinary busy-period note",
        }),
    );
    assert_eq!(status, 200, "body: {sent}");
    let message_id = sent["result"]["id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        !steer_marker.exists(),
        "an ordinary Message must never compile into a steer frame"
    );

    // Arm 2: the explicit Steer control command compiles into native
    // current-cycle injection and is acknowledged.
    let (status, steer) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({
            "content": "Steer: also check the tests before you settle",
            "requested_by": "host",
        }),
    );
    assert_eq!(status, 200, "steer dispatch failed: {steer}");
    assert!(
        steer.to_string().contains("steer_accepted"),
        "explicit Steer must be acknowledged as accepted for injection: {steer}"
    );

    // The steer reached the native runtime, and the queued ordinary message
    // then completed as the NEXT round on the same session.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !steer_marker.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "steer frame never reached the shim"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let steered = std::fs::read_to_string(&steer_marker).unwrap();
    assert!(
        steered.contains("also check the tests"),
        "steer body must reach the native runtime verbatim: {steered}"
    );

    poll_snapshot(
        &serve,
        "queued message completing as the next round",
        |snapshot| {
            snapshot["team_messages"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|message| message["id"].as_str() == Some(message_id.as_str()))
                .is_some_and(|message| {
                    message["deliveries"][0]["status"].as_str() == Some("acknowledged")
                })
                && member_turn_count(snapshot, &member_id) >= 2
        },
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
    // The executable capability report rides the same surface (DOC-89).
    let bindings = pi
        .get("runtime_capability_bindings")
        .and_then(|v| v.as_array())
        .expect("pi must publish executable capability bindings");
    let steer = bindings
        .iter()
        .find(|binding| binding["capability"] == "inject_current_cycle")
        .expect("inject_current_cycle binding");
    assert_eq!(steer["status"].as_str(), Some("supported"));
    let reconcile = bindings
        .iter()
        .find(|binding| binding["capability"] == "reconcile_effect")
        .expect("reconcile_effect binding");
    assert_eq!(
        reconcile["status"].as_str(),
        Some("unsupported"),
        "reconcile_effect must not be claimed for Pi"
    );
}
