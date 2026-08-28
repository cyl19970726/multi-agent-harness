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
//!   - trusted full access is launched without a false enforcement claim,
//!     uncontained workspace-write fails before spawn, and thinking is forced
//!     off;
//!   - an ordinary Message stays in the Harness queue while the member is
//!     busy (never compiled to steer), and only an explicit Steer control
//!     command compiles into native current-cycle injection (DOC-89 §13.1).

use std::path::Path;
use std::time::Duration;

mod fake_provider;
mod firm_env;

use firm_env::{
    create_canonical_agent_member, current_project_id, member_run_for_work_owner, run_firm,
    run_firm_with_env, ServeHandle, TempHome,
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
    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    firm_env::seed_historical_mission(
        home,
        &project_id,
        "mission-pi-fixture",
        "Pi Journey Fixture",
    );
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

fn create_pi_identity_with_ceiling(home: &TempHome, project_id: &str, id: &str, ceiling: &str) {
    let payload = serde_json::json!({
        "command": "create_agent_member",
        "member": {
            "id": id,
            "name": id,
            "description": "Pi integration-test AgentMember",
            "role": "reviewer",
            "capabilities": [],
            "skill_refs": [],
            "provider_profile_ref": "pi",
            "model_preference": null,
            "workspace_policy": "managed-worktree",
            "permission_ceiling": ceiling,
            "organization_status": "active",
            "version": 1,
            "created_by": { "kind": "service", "id": "integration-test" },
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
    .to_string();
    let output = run_firm_with_env(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "member-trust",
            "mutate",
            "--actor-kind",
            "service",
            "--actor-id",
            "integration-test",
            "--idempotency-key",
            &format!("pi-test-member:{id}"),
            "--expected-version",
            "0",
            "--json",
            &payload,
        ],
        &[],
    );
    assert!(
        output.status.success(),
        "create Pi identity failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run_firm_with_env(
        home,
        home.base(),
        &[
            "--project",
            project_id,
            "team",
            "add-member",
            "--id",
            "team-pi-fixture",
            "--member",
            id,
        ],
        &[],
    );
    assert!(
        output.status.success(),
        "add Pi identity to fixture team failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn store_rows(home: &TempHome, project_id: &str, file: &str) -> Vec<serde_json::Value> {
    let path = home.spaces_dir().join(project_id).join(file);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("read {}: {error}", path.display()),
    };
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
            "timed out waiting for {what}; last snapshot: {snapshot}"
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
    create_pi_identity_with_ceiling(&home, &project_id, "agent-pi-worker-full", "full_access");
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
            "members": [{"agent_member_id": "agent-pi-worker-full", "name": "pi-worker", "role": "reviewer", "provider": "pi", "initial_work": "Review the work and report"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _start_body) = serve.post_json(
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
    assert!(
        member_snapshot(&snapshot, &member_id)
            .and_then(|member| member["last_consumed_work_version"].as_u64())
            .is_some(),
        "the first await_next_cycle Work revision must be persisted when round 1 settles"
    );

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
    let round_one_receipt = canonical["provider_receipt_id"]
        .as_str()
        .filter(|receipt| receipt.starts_with("pi-rpc-"))
        .expect("initial Work must carry Pi's real prompt response id")
        .to_string();

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
    let round_two_receipt = follow_up_delivery
        .provider_receipt_id
        .as_deref()
        .filter(|receipt| receipt.starts_with("pi-rpc-"))
        .expect("queued Host mail must carry Pi's real prompt response id");
    assert_ne!(
        round_two_receipt, round_one_receipt,
        "each accepted input must retain its own provider receipt"
    );

    // Launch contract: thinking is forced off. This journey deliberately uses
    // trusted-development full_access, so Pi receives no --tools flag and the
    // profile must remain explicit that no adapter enforcement was verified.
    let args: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&args_marker).expect("read Pi args marker"))
            .expect("Pi args JSON");
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--thinking".to_string(), "off".to_string()]),
        "persistent Pi launch must force thinking off: {args:?}"
    );
    assert!(
        !args.iter().any(|arg| arg == "--tools"),
        "trusted full_access must not be mislabeled as a restricted --tools policy: {args:?}"
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
    assert_eq!(
        member["provider_profile"]["security_enforcement_locus"]["kind"],
        "none_verified"
    );

    let commands = store
        .runtime_commands(&space_id)
        .expect("Pi runtime command evidence");
    let applied = |kind| {
        commands.iter().filter(move |command| {
            command.command == kind
                && command.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                && command.postcondition_status
                    == harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied
        })
    };
    assert_eq!(
        applied(harness_core::agentfirm_api::RuntimeCommandKind::OpenRuntime).count(),
        1,
        "one persistent Pi handle must have one durable OpenRuntime effect: {commands:?}"
    );
    assert_eq!(
        applied(harness_core::agentfirm_api::RuntimeCommandKind::StartCycle).count(),
        2,
        "the two accepted Pi inputs must be two distinct StartCycle effects: {commands:?}"
    );
}

/// DOC-89 §13.1 both arms: while the member is busy, an ordinary Message
/// stays in the durable Harness queue (never compiled into native injection);
/// only an explicit Steer control command compiles into current-cycle
/// injection — and the queued ordinary message then arrives as the NEXT round.
#[test]
fn pi_member_busy_queue_and_explicit_steer_conformance() {
    let home = TempHome::new("pi-steer-conformance");
    let project_id = init_pi_project(&home, "pi-steer");
    create_pi_identity_with_ceiling(&home, &project_id, "agent-pi-steer-full", "full_access");
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
            ("FAKE_PI_STEER_RESPONSE_DELAY_MS", "350"),
        ],
    );

    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi steer conformance",
            "members": [{"agent_member_id": "agent-pi-steer-full", "name": "pi-worker", "role": "reviewer", "provider": "pi", "initial_work": "Hold the first cycle open"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
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
    let steer_started = std::time::Instant::now();
    let (status, steer) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({
            "content": "Steer: also check the tests before you settle",
            "requested_by": "host",
        }),
    );
    assert!(
        steer_started.elapsed() >= Duration::from_millis(300),
        "HTTP steer must wait for Pi's matching command response, not acknowledge a local append"
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
fn pi_prompt_receipt_survives_disconnect_without_redelivery() {
    let home = TempHome::new("pi-accepted-then-disconnected");
    let project_id = init_pi_project(&home, "pi-accepted-then-disconnected");
    create_pi_identity_with_ceiling(
        &home,
        &project_id,
        "agent-pi-disconnect-full",
        "full_access",
    );
    let prompt_marker = home.base().join("pi-disconnect-prompts.txt");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-disconnect-cwd.txt"),
        &home.base().join("pi-sessions/disconnect-session.jsonl"),
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_DISCONNECT_AFTER_PROMPT_ACCEPT", "1"),
            (
                "FAKE_PI_PROMPT_COUNT_MARKER",
                prompt_marker.to_str().unwrap(),
            ),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi accepted input survives later transport loss",
            "members": [{
                "agent_member_id": "agent-pi-disconnect-full",
                "name": "pi-worker",
                "role": "reviewer",
                "provider": "pi",
                "initial_work": "Accept this input then disconnect"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start scheduling failed: {body}");

    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let space_id = firm_env::current_space_id(&home);
    poll_snapshot(&serve, "durable input receipt after Pi disconnect", |_| {
        let commands = store.runtime_commands(&space_id).unwrap_or_default();
        let deliveries = store.fabric_work_deliveries(&space_id).unwrap_or_default();
        commands.iter().any(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::StartCycle
                && command.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                && command.effect_certainty
                    == harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
        }) && deliveries.iter().any(|delivery| {
            delivery.status == harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
                && delivery
                    .provider_receipt_id
                    .as_deref()
                    .is_some_and(|receipt| receipt.starts_with("pi-rpc-"))
        })
    });

    let prompts = std::fs::read_to_string(&prompt_marker).expect("prompt marker");
    assert_eq!(
        prompts.lines().count(),
        1,
        "accepted input must not be blindly replayed after transport loss: {prompts:?}"
    );
    let actions = store_rows(&home, &project_id, "member_actions.jsonl");
    assert!(
        actions
            .iter()
            .all(|action| action["action_type"] != "turn_completed"),
        "provider input acceptance must not be mistaken for cycle settlement: {actions:?}"
    );
}

#[test]
fn pi_workspace_write_managed_member_is_rejected_before_spawn() {
    let home = TempHome::new("pi-workspace-write-admission");
    let project_id = init_pi_project(&home, "pi-workspace-write-admission");
    create_pi_identity_with_ceiling(
        &home,
        &project_id,
        "agent-pi-workspace-write",
        "workspace_write",
    );
    let cwd_marker = home.base().join("pi-workspace-write-spawned.txt");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &cwd_marker,
        &home.base().join("pi-sessions/workspace-write.jsonl"),
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("PI_BIN", fake_pi.as_str()), ("FAKE_PI_RESULT", "DONE")],
    );
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi workspace_write fail-closed admission",
            "members": [{
                "agent_member_id": "agent-pi-workspace-write",
                "name": "pi-worker",
                "role": "reviewer",
                "provider": "pi",
                "initial_work": "Must not reach provider"
            }]
        }),
    );
    assert_eq!(
        status, 400,
        "workspace_write must fail admission: {created}"
    );
    assert!(
        created
            .to_string()
            .contains("TRUSTED_DEVELOPMENT_FULL_ACCESS_REQUIRED"),
        "admission failure must identify the frozen lower ceiling: {created}"
    );
    assert!(
        !cwd_marker.exists(),
        "workspace_write admission must fail before the Pi process starts"
    );

    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let space_id = firm_env::current_space_id(&home);
    let commands = store
        .runtime_commands(&space_id)
        .expect("runtime command evidence");
    assert!(
        commands.iter().all(|command| {
            command.effect_certainty != harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
        }),
        "permission refusal must cause zero provider/process effects: {commands:?}"
    );
}

#[test]
fn pi_full_access_busy_close_reopens_same_session_without_overclaiming_quiesce() {
    let home = TempHome::new("pi-busy-close");
    let project_id = init_pi_project(&home, "pi-busy-close");
    create_pi_identity_with_ceiling(&home, &project_id, "agent-pi-close-full", "full_access");
    let session_file = home.base().join("pi-sessions/close-session.jsonl");
    let prompt_marker = home.base().join("pi-close-prompt.txt");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-close-cwd.txt"),
        &session_file,
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_RESULT", "DONE"),
            ("FAKE_PI_WAIT_FOR_STEER", "1"),
            ("FAKE_PI_PROMPT_MARKER", prompt_marker.to_str().unwrap()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi busy close conformance",
            "members": [{
                "agent_member_id": "agent-pi-close-full",
                "name": "pi-worker",
                "role": "reviewer",
                "provider": "pi",
                "initial_work": "Wait for explicit close"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start scheduling failed: {body}");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !prompt_marker.exists() {
        assert!(std::time::Instant::now() < deadline, "Pi never became busy");
        std::thread::sleep(Duration::from_millis(25));
    }

    let (status, closed) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({
            "reason": "terminal close conformance",
            "requested_by": "host"
        }),
    );
    assert_eq!(
        status, 200,
        "narrow FullAccess Close must succeed: {closed}"
    );
    assert_eq!(closed["result"]["status"].as_str(), Some("closed"));
    assert_eq!(
        closed["result"]["provider_terminal_evidence"]["member_runtime_close"]
            ["managed_runtime_released"]
            .as_str(),
        Some("satisfied"),
        "Close must prove only the owned Pi runtime was released: {closed}"
    );
    assert!(
        session_file.is_file(),
        "native Session truth must remain readable after refusal"
    );

    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let space_id = firm_env::current_space_id(&home);
    let commands = store
        .runtime_commands(&space_id)
        .expect("Pi Close runtime command evidence");
    assert!(
        commands.iter().any(|command| {
            command.command == harness_core::agentfirm_api::RuntimeCommandKind::CloseMember
                && command.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
                && command.postcondition_status
                    == harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied
        }),
        "narrow Close must settle its exact provider effect: {commands:?}"
    );
    assert!(
        commands.iter().all(|command| {
            !matches!(
                command.command,
                harness_core::agentfirm_api::RuntimeCommandKind::QuiesceExecutionLane
                    | harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime
            )
        }),
        "reversible Team Close must not overclaim strong Quiesce/Release: {commands:?}"
    );
    assert!(
        commands.iter().all(|command| {
            command.command != harness_core::agentfirm_api::RuntimeCommandKind::CancelProviderTurn
        }),
        "Pi Close must not fall back to the legacy overloaded cancel command: {commands:?}"
    );

    let (status, reopened) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({
            "reason": "prove Pi native-session continuity",
            "reopened_by": "host"
        }),
    );
    assert_eq!(status, 202, "Pi Reopen must be accepted: {reopened}");
    assert_eq!(
        reopened["result"]["reopen"]["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["result"]["reopen"]["member_run"]["native_session"]["native_session_id"].as_str(),
        Some(session_file.to_string_lossy().as_ref()),
        "Reopen must retain the exact Pi native session: {reopened}"
    );
}

#[test]
fn pi_full_access_close_reaps_owned_group_without_claiming_strong_quiesce() {
    let home = TempHome::new("pi-full-access-quiesce");
    let project_id = init_pi_project(&home, "pi-full-access-quiesce");
    create_pi_identity_with_ceiling(&home, &project_id, "agent-pi-quiesce-full", "full_access");
    let session_file = home.base().join("pi-sessions/full-access-session.jsonl");
    let prompt_marker = home.base().join("pi-full-access-prompt.txt");
    let writer_marker = home.base().join("pi-full-access-background-writer.txt");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-full-access-cwd.txt"),
        &session_file,
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_RESULT", "DONE"),
            ("FAKE_PI_WAIT_FOR_STEER", "1"),
            ("FAKE_PI_PROMPT_MARKER", prompt_marker.to_str().unwrap()),
            ("FAKE_PI_BACKGROUND_WRITER", writer_marker.to_str().unwrap()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi FullAccess quiesce must fail closed",
            "members": [{
                "agent_member_id": "agent-pi-quiesce-full",
                "name": "pi-worker",
                "role": "reviewer",
                "provider": "pi",
                "initial_work": "Wait while a writable child remains alive"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start scheduling failed: {body}");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !prompt_marker.exists() || !writer_marker.exists() {
        if std::time::Instant::now() >= deadline {
            let (_, snapshot) = serve.get_json("/v1/snapshot");
            let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
            let space_id = firm_env::current_space_id(&home);
            panic!(
                "Pi or its background writer never became active; snapshot={snapshot}; commands={:?}",
                store.runtime_commands(&space_id).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let (status, close) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({
            "reason": "close only the owned Pi runtime",
            "requested_by": "host"
        }),
    );
    assert_eq!(status, 200, "narrow FullAccess Close must succeed: {close}");

    let store = harness_store::HarnessStore::new(home.spaces_dir().join(&project_id));
    let space_id = firm_env::current_space_id(&home);
    let commands = store
        .runtime_commands(&space_id)
        .expect("Pi quiesce RuntimeCommand evidence");
    assert!(commands.iter().any(|command| {
        command.command == harness_core::agentfirm_api::RuntimeCommandKind::CloseMember
            && command.status == harness_core::agentfirm_api::RuntimeCommandStatus::Applied
            && command.effect_certainty
                == harness_core::agentfirm_api::RuntimeEffectCertainty::Applied
            && command.postcondition_status
                == harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied
    }));
    assert!(commands.iter().all(|command| {
        !matches!(
            command.command,
            harness_core::agentfirm_api::RuntimeCommandKind::QuiesceExecutionLane
                | harness_core::agentfirm_api::RuntimeCommandKind::ReleaseRuntime
        )
    }));

    let size_after_close = std::fs::metadata(&writer_marker)
        .expect("background writer marker after Close")
        .len();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(
        std::fs::metadata(&writer_marker)
            .expect("background writer marker remains readable")
            .len(),
        size_after_close,
        "the child in Pi's owned process group must stop with the managed runtime"
    );
}

#[test]
fn pi_busy_interrupt_waits_for_abort_receipt_and_agent_settled() {
    let home = TempHome::new("pi-busy-interrupt");
    let project_id = init_pi_project(&home, "pi-busy-interrupt");
    create_pi_identity_with_ceiling(&home, &project_id, "agent-pi-interrupt-full", "full_access");
    let prompt_marker = home.base().join("pi-interrupt-prompt.txt");
    let fake_bin = fake_provider::install_pi_rpc_shim(
        home.base(),
        &home.base().join("pi-interrupt-cwd.txt"),
        &home.base().join("pi-sessions/interrupt-session.jsonl"),
        "DONE",
    );
    let fake_pi = fake_bin.join("pi").display().to_string();
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("PI_BIN", fake_pi.as_str()),
            ("FAKE_PI_RESULT", "DONE"),
            ("FAKE_PI_WAIT_FOR_STEER", "1"),
            ("FAKE_PI_PROMPT_MARKER", prompt_marker.to_str().unwrap()),
        ],
    );
    let (_, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Pi busy interrupt conformance",
            "members": [{
                "agent_member_id": "agent-pi-interrupt-full",
                "name": "pi-worker",
                "role": "reviewer",
                "provider": "pi",
                "initial_work": "Wait for explicit interrupt"
            }]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "start scheduling failed: {body}");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !prompt_marker.exists() {
        assert!(std::time::Instant::now() < deadline, "Pi never became busy");
        std::thread::sleep(Duration::from_millis(25));
    }

    let (status, interrupted) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/interrupt"),
        &serde_json::json!({
            "reason": "terminal interrupt conformance",
            "requested_by": "host"
        }),
    );
    assert_eq!(status, 200, "busy interrupt failed: {interrupted}");
    assert_eq!(interrupted["result"]["status"], "interrupted");
    let evidence = &interrupted["result"]["provider_terminal_evidence"];
    assert_eq!(evidence["provider_terminal_event"], "agent_settled");
    assert_eq!(evidence["post_abort_observation"]["is_streaming"], false);
    assert_eq!(evidence["post_abort_observation"]["process_alive"], true);
    poll_snapshot(&serve, "Pi member idle after interrupt", |snapshot| {
        member_snapshot(snapshot, &member_id)
            .is_some_and(|member| member["status"].as_str() == Some("idle"))
    });
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
        "Pi 0.84.2 cancel is executable only after the exact live abort canary"
    );
    assert_eq!(
        team_profile
            .get("supports_resume")
            .and_then(|v| v.as_bool()),
        Some(true),
        "Pi 0.84.2 resume has exact retained-session live evidence"
    );
    assert_eq!(
        team_profile
            .get("ordinary_message_boundary")
            .and_then(|v| v.as_str()),
        Some("next_round_batched")
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
    assert_eq!(
        pi["core_runtime_capability_admission"], "active",
        "Pi 0.84.2 core open/start/observe bindings have exact live evidence"
    );
    assert_eq!(
        team_profile["binding_admission"], "degraded",
        "optional Pi capabilities without proportional live evidence must remain pending"
    );
    let admitted_bindings = team_profile["capability_bindings"]
        .as_array()
        .expect("versioned Pi capability bindings");
    for capability in ["interrupt_current_cycle", "close_runtime"] {
        let binding = admitted_bindings
            .iter()
            .find(|binding| binding["capability"] == capability)
            .unwrap_or_else(|| panic!("missing {capability} binding"));
        assert_eq!(binding["status"], "verified");
        assert_eq!(binding["admission"], "active");
    }
    let steer_admission = admitted_bindings
        .iter()
        .find(|binding| binding["capability"] == "inject_current_cycle")
        .expect("inject_current_cycle binding");
    assert_eq!(steer_admission["status"], "review_required");
    assert_eq!(steer_admission["admission"], "pending_dependency");
}
