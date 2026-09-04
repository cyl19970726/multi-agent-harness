//! Regression coverage for #749: a member joined to an already-running TeamRun
//! never reached the adoption seam that materializes AgentSessions.
//!
//! `team-run create` + adoption run `ensure_team_runtime_fabric` once, over the
//! roster the Supervisor was started with. `team-run add-member` admits a
//! MemberRun into a live run long after that pass, so the joined member owned
//! zero AgentSessions when the Supervisor first drove it and died on
//! `AGENT_SESSION_AMBIGUOUS: member <member-run> requires one current session
//! in Execution Space <space>, found 0` — surfaced to the Host as
//! `runtime_recovery_required` for a member that had never touched a provider.
//!
//! Every provider here is a deterministic PATH shim; no real provider is
//! invoked and no network call is made.

mod fake_provider;
mod firm_env;

use std::time::{Duration, Instant};

use firm_env::{
    create_canonical_agent_member, current_project_id, current_space_id, run_firm,
    run_firm_with_env, TempHome,
};
use harness_core::agentfirm_api::{AgentSession, AgentSessionStatus, RuntimeDriverRef};
use harness_store::HarnessStore;

const HOST_ID: &str = "agent-join-host";
const FOUNDING_ID: &str = "join-founding-worker";
const JOINING_ID: &str = "join-late-worker";
const TEAM_ID: &str = "team-live-join-fixture";

struct Fixture {
    project_id: String,
    space_id: String,
}

fn ok(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The founding member must stay live long enough for the join to land while
/// the Supervisor is still driving. If it retired first, the daemon would
/// re-adopt the run through the ordinary start path, whose fabric pass would
/// mask the very defect under test.
fn provider_env(bin: &str) -> Vec<(&str, &str)> {
    vec![
        ("PATH", bin),
        ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        ("FAKE_KIMI_RESULT", "done"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "60000"),
    ]
}

fn bootstrap(home: &TempHome) -> Fixture {
    let root = home.base().to_path_buf();
    ok(&run_firm(home, &root, &["init"]), "firm init");
    let project_id = current_project_id(home);
    let space_id = current_space_id(home);
    let node = run_firm(home, &root, &["node", "init"]);
    ok(&node, "node init");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();
    ok(
        &run_firm(
            home,
            &root,
            &[
                "node",
                "project",
                "register",
                "--node-id",
                &node_id,
                "--project-binding-id",
                &project_id,
            ],
        ),
        "node project register",
    );
    for (id, role) in [
        (HOST_ID, "host"),
        (FOUNDING_ID, "implementer"),
        (JOINING_ID, "implementer"),
    ] {
        ok(
            &create_canonical_agent_member(home, &root, &project_id, id, id, role, "codex", &[]),
            "agent member create",
        );
    }
    // The joining AgentMember deliberately has no TeamMembership yet: it earns
    // one through `team add-member`, exactly as the dogfood run did.
    ok(
        &run_firm(
            home,
            &root,
            &[
                "team",
                "create",
                "--id",
                TEAM_ID,
                "--name",
                "Live join team",
                "--description",
                "Flat team that gains a member mid-run",
                "--host-agent-id",
                HOST_ID,
                "--node-id",
                &node_id,
                "--member",
                HOST_ID,
                "--member",
                FOUNDING_ID,
            ],
        ),
        "team create",
    );
    Fixture {
        project_id,
        space_id,
    }
}

/// Owns the NodeDaemon child for the whole test. Every exit path — including a
/// panic in the readiness loop or in any assertion below — reaps it, so a
/// failing run never leaves a daemon holding the machine lease.
struct DaemonGuard(std::process::Child);

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_daemon(home: &TempHome, env: &[(&str, &str)]) -> DaemonGuard {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args([
            "daemon",
            "serve",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "120",
        ])
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env_remove("KIMI_CODE_BIN")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    // Guarded from the first instant it exists: the readiness loop below can
    // panic, and an unguarded child would outlive the test.
    let mut guard = DaemonGuard(command.spawn().expect("spawn NodeDaemon"));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = run_firm(home, home.base(), &["daemon", "status"]);
        if status.status.success() && !String::from_utf8_lossy(&status.stdout).contains("absent") {
            return guard;
        }
        assert!(
            guard.0.try_wait().expect("inspect NodeDaemon").is_none(),
            "NodeDaemon exited before becoming ready"
        );
        assert!(Instant::now() < deadline, "NodeDaemon readiness timeout");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn store(home: &TempHome, space_id: &str) -> HarnessStore {
    HarnessStore::new(home.spaces_dir().join(space_id))
}

fn current_sessions(home: &TempHome, space_id: &str, agent_member_id: &str) -> Vec<AgentSession> {
    store(home, space_id)
        .fabric_agent_sessions(space_id)
        .expect("canonical AgentSession fabric")
        .into_iter()
        .filter(|session| {
            session.agent_member_id == agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
        })
        .collect()
}

fn wait_for_session(home: &TempHome, space_id: &str, agent_member_id: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if !current_sessions(home, space_id, agent_member_id).is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{agent_member_id} never got an AgentSession"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Everything this Supervisor generation recorded about the joined member, so
/// a failure names the actual refusal instead of only its status.
fn member_journal(home: &TempHome, space_id: &str, run_id: &str, member_run_id: &str) -> String {
    let store = store(home, space_id);
    let mut lines = store
        .current_team_run_events(run_id)
        .expect("team run events")
        .into_iter()
        .filter(|event| event.member_run_id.as_deref() == Some(member_run_id))
        .map(|event| format!("event {} {}: {}", event.seq, event.operation, event.summary))
        .collect::<Vec<_>>();
    lines.extend(
        store
            .member_actions()
            .expect("member actions")
            .into_iter()
            .filter(|action| action.member_run_id == member_run_id)
            .map(|action| {
                format!(
                    "action {} {:?}: {}",
                    action.action_type, action.status, action.summary
                )
            }),
    );
    lines.join("\n")
}

fn member_run_report(home: &TempHome, fixture: &Fixture, run_id: &str) -> serde_json::Value {
    let status = run_firm_with_env(
        home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "status",
            "--id",
            run_id,
            "--json",
        ],
        &[],
    );
    ok(&status, "team-run status");
    serde_json::from_slice(&status.stdout).expect("status JSON")
}

#[test]
fn member_joined_to_a_running_team_run_starts_with_its_own_agent_session() {
    let home = TempHome::new("live-run-member-join");
    let fixture = bootstrap(&home);
    let bin = fake_provider::install_kimi_acp_shim(home.base());
    fake_provider::install_codex_team_shim(&bin);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let env = provider_env(&path);

    let create = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "create",
            "--agent-team-id",
            TEAM_ID,
            "--objective",
            "Prove a member joined to a live run starts like a founding member",
            "--host-runtime-mode",
            "external_interactive",
            "--member",
            &format!("{HOST_ID}:host:codex/external_interactive"),
            "--member",
            &format!("{FOUNDING_ID}:implementer:codex/app-server#Hold the run open"),
        ],
        &env,
    );
    ok(&create, "team-run create");
    let run_id = String::from_utf8_lossy(&create.stdout).trim().to_string();

    // Reaped on every exit path, including a panic in the assertions below.
    let _daemon = spawn_daemon(&home, &env);
    let started = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &env,
    );
    ok(&started, "team-run start");

    // The join must land while this Supervisor generation is live.
    wait_for_session(
        &home,
        &fixture.space_id,
        FOUNDING_ID,
        Duration::from_secs(60),
    );

    let joined_team = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team",
            "add-member",
            "--id",
            TEAM_ID,
            "--member",
            JOINING_ID,
        ],
        &env,
    );
    ok(&joined_team, "team add-member");

    let joined = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--space",
            &fixture.space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "add-member",
            "--id",
            &run_id,
            "--member",
            &format!("{JOINING_ID}:implementer:codex/app-server"),
            "--initial-work",
            "Prove the joined member can be dispatched Work",
        ],
        &env,
    );
    ok(&joined, "team-run add-member");
    let joined: serde_json::Value = serde_json::from_slice(&joined.stdout).expect("joined JSON");
    let joined_member_run_id = joined["member_run"]["id"]
        .as_str()
        .expect("joined member run id")
        .to_string();
    let joined_work_id = joined["work"]["id"]
        .as_str()
        .expect("joined member initial Work id")
        .to_string();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut sessions;
    let mut dispatched = false;
    loop {
        sessions = current_sessions(&home, &fixture.space_id, JOINING_ID);
        dispatched = dispatched
            || store(&home, &fixture.space_id)
                .fabric_work_deliveries(&fixture.space_id)
                .expect("canonical WorkDelivery fabric")
                .iter()
                .any(|delivery| {
                    delivery.work_id == joined_work_id
                        && delivery.status
                            != harness_core::agentfirm_api::WorkDeliveryStatus::Queued
                });
        if (!sessions.is_empty() && dispatched) || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let report = member_run_report(&home, &fixture, &run_id);
    let journal = member_journal(&home, &fixture.space_id, &run_id, &joined_member_run_id);

    let joined_report = report["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["member_run"]["id"] == joined_member_run_id.as_str())
        .and_then(|member| member["member_run"]["status"].as_str())
        .map(|status| format!("joined member status={status}\n{journal}"))
        .expect("joined member in the run report");
    let [session] = sessions.as_slice() else {
        panic!(
            "a member joined to a live run must own exactly one current AgentSession, found {}:\n{joined_report}",
            sessions.len()
        );
    };
    assert_eq!(
        session.execution_space_id, fixture.space_id,
        "the joined member's session must live in the TeamRun's Execution Space"
    );
    assert_eq!(
        session.provider_kind, "codex",
        "the joined member's session must carry its own provider"
    );
    match &session.control_state.driver_ref {
        RuntimeDriverRef::TeamSupervisor { team_run_id, .. } => assert_eq!(
            team_run_id, &run_id,
            "the joined member's session must be bound to this TeamRun's Supervisor"
        ),
        other => panic!("the joined member's session is not Supervisor-bound: {other:?}"),
    }
    assert!(
        !joined_report.contains("status=failed"),
        "the joined member must not fail its first provider attempt:\n{joined_report}"
    );
    assert!(
        dispatched,
        "the joined member's assigned Work was never dispatched:\n{joined_report}"
    );
}
