//! Integration tests for the machine-scoped NodeDaemon.
//!
//! One stable local NodeDaemon discovers registered Execution Spaces and owns
//! every child TeamRun supervisor. A Team is placed on exactly one Node.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_core::TeamSupervisorLeaseStatus;
use harness_store::HarnessStore;

mod fake_provider;
mod firm_env;

#[path = "team_run_daemon/completed_run_close.rs"]
mod completed_run_close;

use firm_env::{
    create_canonical_agent_member, current_project_id, run_firm, run_firm_with_env, TempHome,
};

struct RuntimeFixture {
    project_root: PathBuf,
    project_id: String,
    execution_space_id: String,
    node_id: String,
    team_id: String,
    host_id: String,
}

fn success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn bootstrap_runtime(home: &TempHome, name: &str) -> RuntimeFixture {
    let project_root = home.base().join(name);
    std::fs::create_dir_all(&project_root).unwrap();
    success(&run_firm(home, &project_root, &["init"]), "firm init");
    let project_id = current_project_id(home);
    let execution_space_id = format!("{name}-space");
    success(
        &run_firm(
            home,
            &project_root,
            &[
                "space",
                "init",
                "--id",
                &execution_space_id,
                "--project-binding",
                &project_id,
            ],
        ),
        "space init",
    );
    let selected = |args: &[&str]| {
        let mut full = vec![
            "--space",
            execution_space_id.as_str(),
            "--project",
            project_id.as_str(),
        ];
        full.extend_from_slice(args);
        run_firm(home, &project_root, &full)
    };

    let node = selected(&["node", "init"]);
    success(&node, "node init");
    let node: serde_json::Value = serde_json::from_slice(&node.stdout).expect("node JSON");
    let node_id = node["id"].as_str().expect("node id").to_string();

    success(
        &selected(&[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--execution-space-id",
            &execution_space_id,
            "--project-binding-id",
            &project_id,
        ]),
        "node project register",
    );

    // DOC-108 retired the Mission writers; seed legacy provenance directly.
    let mission_id = format!("mission-daemon-{name}");
    firm_env::seed_historical_mission(
        home,
        &execution_space_id,
        &mission_id,
        &format!("Daemon mission {name}"),
    );

    let host_id = format!("agent-daemon-host-{name}");
    let host = create_canonical_agent_member(
        home,
        &project_root,
        &project_id,
        &host_id,
        &format!("host-{name}"),
        "host",
        "codex",
        &[("FIRM_SPACE", execution_space_id.as_str())],
    );
    success(&host, "canonical host create");

    let team = selected(&[
        "team",
        "create",
        "--name",
        &format!("Daemon team {name}"),
        "--description",
        "Flat AgentTeam placed on one Node",
        "--mission-id",
        &mission_id,
        "--host-agent-id",
        &host_id,
        "--node-id",
        &node_id,
        "--member",
        &host_id,
    ]);
    success(&team, "team create");
    let team: serde_json::Value = serde_json::from_slice(&team.stdout).expect("team JSON");
    let team_id = team["id"].as_str().expect("team id").to_string();

    RuntimeFixture {
        project_root,
        project_id,
        execution_space_id,
        node_id,
        team_id,
        host_id,
    }
}

fn node_daemon_socket_path(home: &TempHome, node_id: &str) -> PathBuf {
    let direct = home
        .firm_home()
        .join("nodes")
        .join(node_id)
        .join("daemon.sock");
    if direct.to_string_lossy().len() < 100 {
        return direct;
    }
    let mut hasher = DefaultHasher::new();
    home.firm_home().to_string_lossy().hash(&mut hasher);
    node_id.hash(&mut hasher);
    Path::new("/tmp").join(format!("firm-node-daemon-{:x}.sock", hasher.finish()))
}

fn spawn_daemon(
    home: &TempHome,
    fixture: &RuntimeFixture,
    extra_env: &[(&str, &str)],
) -> std::process::Child {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args([
            "daemon",
            "serve",
            "--scan-interval-secs",
            "1",
            "--idle-timeout-secs",
            "30",
            "--max-concurrency",
            "8",
        ])
        .current_dir(&fixture.project_root)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.spawn().expect("spawn NodeDaemon")
}

fn wait_for_socket(child: &mut std::process::Child, socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect NodeDaemon") {
            panic!(
                "NodeDaemon exited before binding {}: {status}",
                socket.display()
            );
        }
        assert!(
            Instant::now() < deadline,
            "NodeDaemon did not bind {}",
            socket.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn socket_request(socket: &Path, request: &str) -> serde_json::Value {
    let mut stream = std::os::unix::net::UnixStream::connect(socket).expect("connect daemon");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(stream, "{request}").unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    std::io::BufReader::new(&mut stream)
        .read_line(&mut response)
        .expect("read daemon response");
    serde_json::from_str(response.trim()).expect("daemon response JSON")
}

fn stop_daemon(
    home: &TempHome,
    fixture: &RuntimeFixture,
    child: &mut std::process::Child,
    socket: &Path,
) {
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let generation = store
        .latest_node_daemon_lease(&fixture.node_id)
        .expect("NodeDaemon lease read")
        .expect("live NodeDaemon lease")
        .generation;
    let request = serde_json::json!({
        "cmd":"stop",
        "execution_space_id":fixture.execution_space_id,
        "daemon_generation":generation,
    });
    assert_eq!(socket_request(socket, &request.to_string())["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child.try_wait().expect("inspect daemon stop").is_some() {
            assert!(!socket.exists(), "daemon left socket behind");
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    panic!("NodeDaemon did not stop before timeout");
}

fn create_run(
    home: &TempHome,
    fixture: &RuntimeFixture,
    name: &str,
    extra_env: &[(&str, &str)],
) -> String {
    let mut identity_env = vec![("FIRM_SPACE", fixture.execution_space_id.as_str())];
    identity_env.extend(extra_env.iter().copied());
    let identity = create_canonical_agent_member(
        home,
        &fixture.project_root,
        &fixture.project_id,
        name,
        name,
        "implementer",
        "kimi",
        &identity_env,
    );
    success(&identity, "canonical runtime member create");
    let add_member = run_firm_with_env(
        home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team",
            "add-member",
            "--id",
            &fixture.team_id,
            "--member",
            name,
        ],
        extra_env,
    );
    success(&add_member, "canonical runtime member team placement");
    let output = run_firm_with_env(
        home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "create",
            "--agent-team-id",
            &fixture.team_id,
            "--objective",
            &format!("NodeDaemon run {name}"),
            "--host-runtime-mode",
            "external_interactive",
            "--member",
            &format!("{}:host:codex/external_interactive", fixture.host_id),
            "--member",
            &format!("{name}:implementer:kimi@crates/a#Wait for daemon inspection"),
        ],
        extra_env,
    );
    success(&output, "team-run create");
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    store
        .team_runs()
        .expect("team runs")
        .into_iter()
        .find(|run| run.objective == format!("NodeDaemon run {name}"))
        .expect("created TeamRun")
        .id
}

fn wait_for_run(socket: &Path, execution_space_id: &str, run_id: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = socket_request(socket, r#"{"cmd":"status"}"#);
        if status["runs"].as_array().is_some_and(|runs| {
            runs.iter().any(|run| {
                run["execution_space_id"] == execution_space_id && run["run_id"] == run_id
            })
        }) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "daemon did not adopt {execution_space_id}/{run_id}: {status}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn daemon_serve_uses_stable_node_socket_and_reports_identity() {
    let home = TempHome::new("node-daemon-bind");
    let fixture = bootstrap_runtime(&home, "project");
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &[]);
    wait_for_socket(&mut daemon, &socket);

    let status = socket_request(&socket, r#"{"cmd":"status"}"#);
    assert_eq!(status["ok"], true);
    assert_eq!(status["node_id"], fixture.node_id);
    assert_eq!(
        status["daemon_id"],
        format!("node-daemon:{}", fixture.node_id)
    );
    assert!(status["runs"].as_array().is_some_and(Vec::is_empty));

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}

#[test]
fn team_run_start_delegates_to_node_daemon_and_is_idempotent() {
    let home = TempHome::new("node-daemon-delegate");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run_id = create_run(&home, &fixture, "worker", &[]);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let provider_env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FAKE_KIMI_WAIT", "1"),
    ];
    let mut daemon = spawn_daemon(&home, &fixture, &provider_env);
    wait_for_socket(&mut daemon, &socket);

    let start_args = [
        "--space",
        fixture.execution_space_id.as_str(),
        "--project",
        fixture.project_id.as_str(),
        "team-run",
        "start",
        "--id",
        run_id.as_str(),
    ];
    let first = run_firm_with_env(&home, &fixture.project_root, &start_args, &provider_env);
    success(&first, "team-run start");
    assert!(String::from_utf8_lossy(&first.stdout).contains("delegated to NodeDaemon"));
    let status = wait_for_run(&socket, &fixture.execution_space_id, &run_id);
    let managed = status["runs"].as_array().unwrap();
    assert_eq!(managed.len(), 1);
    assert_eq!(managed[0]["project_binding_id"], fixture.project_id);
    let daemon_generation = managed[0]["daemon_generation"]
        .as_u64()
        .expect("managed daemon generation");
    let supervisor_id = managed[0]["supervisor_id"]
        .as_str()
        .expect("managed Supervisor id")
        .to_string();
    let supervisor_generation = managed[0]["supervisor_generation"]
        .as_u64()
        .expect("managed Supervisor generation");
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let member_runtime_generations = || {
        let mut sessions = store
            .fabric_agent_sessions(&fixture.execution_space_id)
            .expect("managed AgentSessions")
            .into_iter()
            .map(|session| {
                (
                    session.agent_member_id,
                    session.id,
                    session.node_daemon_generation,
                    session.runtime_generation,
                )
            })
            .collect::<Vec<_>>();
        sessions.sort();
        sessions
    };
    let session_deadline = Instant::now() + Duration::from_secs(5);
    let mut previous_sessions = member_runtime_generations();
    let sessions_before_duplicate = loop {
        std::thread::sleep(Duration::from_millis(25));
        let current_sessions = member_runtime_generations();
        if !current_sessions.is_empty() && current_sessions == previous_sessions {
            break current_sessions;
        }
        assert!(
            Instant::now() < session_deadline,
            "member runtime authority did not stabilize before duplicate start"
        );
        previous_sessions = current_sessions;
    };

    let duplicate = socket_request(
        &socket,
        &serde_json::json!({
            "cmd": "start",
            "execution_space_id": fixture.execution_space_id,
            "run_id": run_id,
        })
        .to_string(),
    );
    assert_eq!(duplicate["ok"], true, "duplicate start: {duplicate}");
    assert_eq!(duplicate["already_managed"], true);
    assert_eq!(duplicate["reused"], true);
    assert_eq!(
        duplicate["daemon_id"],
        format!("node-daemon:{}", fixture.node_id)
    );
    assert_eq!(duplicate["daemon_generation"], daemon_generation);
    assert_eq!(duplicate["supervisor_id"], supervisor_id);
    assert_eq!(duplicate["supervisor_generation"], supervisor_generation);

    let duplicate_status = wait_for_run(&socket, &fixture.execution_space_id, &run_id);
    assert_eq!(duplicate_status["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        duplicate_status["runs"][0]["daemon_generation"],
        daemon_generation
    );
    assert_eq!(duplicate_status["runs"][0]["supervisor_id"], supervisor_id);
    assert_eq!(
        duplicate_status["runs"][0]["supervisor_generation"],
        supervisor_generation
    );
    assert_eq!(
        member_runtime_generations(),
        sessions_before_duplicate,
        "an idempotent start must not restart any member runtime"
    );

    let replay = run_firm_with_env(&home, &fixture.project_root, &start_args, &provider_env);
    success(&replay, "idempotent CLI team-run start");
    assert!(
        String::from_utf8_lossy(&replay.stdout).contains(&format!(
            "already managed by NodeDaemon {} (gen {daemon_generation})",
            fixture.node_id
        )),
        "CLI omitted the idempotent start result: {}",
        String::from_utf8_lossy(&replay.stdout)
    );
    let replay_status = wait_for_run(&socket, &fixture.execution_space_id, &run_id);
    assert_eq!(replay_status["runs"].as_array().map(Vec::len), Some(1));

    stop_daemon(&home, &fixture, &mut daemon, &socket);
    let lease = store
        .latest_team_supervisor_lease(&run_id)
        .expect("lease read")
        .expect("supervisor lease");
    assert_eq!(lease.status, TeamSupervisorLeaseStatus::Released);
}

#[test]
fn node_daemon_honors_exact_project_scoped_provider_admission() {
    let home = TempHome::new("node-daemon-provider-admission-scope");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let provider_env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.31.2"),
        ("FAKE_KIMI_WAIT", "1"),
    ];
    let run_id = create_run(&home, &fixture, "admitted-worker", &provider_env);

    let admission = run_firm_with_env(
        &home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "provider",
            "admit",
            "--provider",
            "kimi",
            "--execution-mode",
            "kimi_acp",
            "--provider-version",
            "0.31.2",
            "--adapter-contract-version",
            "kimi-acp-v1",
            "--evidence",
            "test:exact-project-admission",
        ],
        &provider_env,
    );
    success(&admission, "provider admission");

    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &provider_env);
    wait_for_socket(&mut daemon, &socket);

    let started = run_firm_with_env(
        &home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &provider_env,
    );
    success(
        &started,
        "NodeDaemon must resolve the TeamRun Project Binding before provider preflight",
    );
    let status = wait_for_run(&socket, &fixture.execution_space_id, &run_id);
    assert_eq!(status["runs"][0]["project_binding_id"], fixture.project_id);

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}

#[test]
fn public_team_run_start_cannot_use_test_idle_env_to_bypass_node_daemon() {
    let home = TempHome::new("node-daemon-no-test-bypass");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let provider_env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100"),
    ];
    let run_id = create_run(&home, &fixture, "worker", &provider_env);
    let output = run_firm_with_env(
        &home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &provider_env,
    );
    assert!(
        !output.status.success(),
        "test-only provider timing must not become a public start bypass"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("NODE_DAEMON_UNAVAILABLE"),
        "unexpected missing-daemon error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    assert!(
        store
            .latest_team_supervisor_lease(&run_id)
            .expect("supervisor lease read")
            .is_none(),
        "public start spawned a hidden in-process supervisor"
    );
}

#[test]
fn daemon_rejects_second_machine_owner_and_bad_commands() {
    let home = TempHome::new("node-daemon-negative");
    let fixture = bootstrap_runtime(&home, "project");
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &[]);
    wait_for_socket(&mut daemon, &socket);

    let malformed = socket_request(&socket, "{not-json");
    assert_eq!(malformed["ok"], false);
    assert!(malformed["error"]
        .as_str()
        .is_some_and(|error| error.starts_with("invalid json:")));
    let missing = socket_request(&socket, r#"{"cmd":"start"}"#);
    assert_eq!(
        missing["error"],
        "execution_space_id and run_id are required"
    );
    let unknown = socket_request(&socket, r#"{"cmd":"unknown"}"#);
    assert_eq!(unknown["error"], "unknown command: unknown");

    let second = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["daemon", "serve"])
        .current_dir(&fixture.project_root)
        .envs(home.envs())
        .output()
        .expect("second daemon");
    assert!(
        !second.status.success(),
        "second machine daemon unexpectedly started"
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("NODE_DAEMON_ALREADY_RUNNING"),
        "unexpected second-owner error: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}

#[test]
fn stale_socket_with_live_lease_is_not_reclaimed() {
    let home = TempHome::new("node-daemon-live-lease-fence");
    let fixture = bootstrap_runtime(&home, "project");
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    std::fs::create_dir_all(socket.parent().expect("socket parent")).unwrap();
    let stale_listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    drop(stale_listener);
    assert!(socket.exists(), "fixture did not leave a stale socket");

    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    store
        .acquire_node_daemon_lease(
            &fixture.node_id,
            "node-daemon:previous-owner",
            "previous-instance",
            now_ms,
            60_000,
        )
        .expect("seed live NodeDaemon lease");

    let attempted_takeover = std::process::Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["daemon", "serve"])
        .current_dir(&fixture.project_root)
        .envs(home.envs())
        .output()
        .expect("attempt NodeDaemon takeover");
    assert!(
        !attempted_takeover.status.success(),
        "daemon unexpectedly reclaimed a socket protected by a live lease"
    );
    assert!(
        String::from_utf8_lossy(&attempted_takeover.stderr).contains("NODE_DAEMON_LEASE_HELD"),
        "unexpected takeover error: {}",
        String::from_utf8_lossy(&attempted_takeover.stderr)
    );
    assert!(socket.exists(), "live-lease fence removed the stale socket");
}

#[test]
fn one_node_daemon_adopts_runs_from_two_execution_spaces() {
    let home = TempHome::new("node-daemon-two-spaces");
    let fixture_a = bootstrap_runtime(&home, "project-a");
    let fixture_b = bootstrap_runtime(&home, "project-b");
    assert_eq!(
        fixture_a.node_id, fixture_b.node_id,
        "local Node identity changed"
    );
    assert_ne!(fixture_a.execution_space_id, fixture_b.execution_space_id);

    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let run_a = create_run(&home, &fixture_a, "worker-a", &[]);
    let run_b = create_run(&home, &fixture_b, "worker-b", &[]);
    let socket = node_daemon_socket_path(&home, &fixture_a.node_id);
    let provider_env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FAKE_KIMI_WAIT", "1"),
    ];
    let mut daemon = spawn_daemon(&home, &fixture_a, &provider_env);
    wait_for_socket(&mut daemon, &socket);

    for (fixture, run_id) in [(&fixture_a, &run_a), (&fixture_b, &run_b)] {
        let start = run_firm_with_env(
            &home,
            &fixture.project_root,
            &[
                "--space",
                &fixture.execution_space_id,
                "--project",
                &fixture.project_id,
                "team-run",
                "start",
                "--id",
                run_id,
            ],
            &provider_env,
        );
        success(&start, "cross-space team-run start");
    }

    wait_for_run(&socket, &fixture_a.execution_space_id, &run_a);
    let status = wait_for_run(&socket, &fixture_b.execution_space_id, &run_b);
    let runs = status["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert!(runs
        .iter()
        .any(|run| run["execution_space_id"] == fixture_a.execution_space_id));
    assert!(runs
        .iter()
        .any(|run| run["execution_space_id"] == fixture_b.execution_space_id));
    assert_eq!(status["process_id"].as_u64(), Some(daemon.id() as u64));

    let store_a = HarnessStore::new(home.spaces_dir().join(&fixture_a.execution_space_id));
    let store_b = HarnessStore::new(home.spaces_dir().join(&fixture_b.execution_space_id));
    let lease_a = store_a
        .latest_team_supervisor_lease(&run_a)
        .unwrap()
        .unwrap();
    let lease_b = store_b
        .latest_team_supervisor_lease(&run_b)
        .unwrap()
        .unwrap();
    assert_eq!(lease_a.owner_process_id, lease_b.owner_process_id);
    assert_eq!(lease_a.node_daemon_id, lease_b.node_daemon_id);
    assert_eq!(
        lease_a.node_daemon_generation,
        lease_b.node_daemon_generation
    );

    stop_daemon(&home, &fixture_a, &mut daemon, &socket);
}
