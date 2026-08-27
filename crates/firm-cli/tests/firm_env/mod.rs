//! Shared test helper: an isolated harness HOME so integration tests never touch
//! the developer's real `~/.firm` (goal-multi-project "Test isolation" risk).
//!
//! `TempHome` creates a unique temp dir, points `HOME` and `FIRM_HOME` at it,
//! and exposes the registry/marker paths. It is passed to spawned `harness`
//! processes via `.envs(home.envs())`; we never mutate the test process's own env
//! (which would race across parallel tests).

#![allow(dead_code)]

mod provider_received_work;
mod work_owner;
pub use provider_received_work::record_provider_received_work;
#[allow(unused_imports)]
pub use work_owner::member_run_for_work_owner;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("fixture clock")
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub struct TempHome {
    base: PathBuf,
    home: PathBuf,
    firm_home: PathBuf,
}

pub fn current_space_id(home: &TempHome) -> String {
    let registry: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.space_registry_path()).expect("space registry"),
    )
    .expect("space registry JSON");
    registry["current_space_id"]
        .as_str()
        .expect("current_space_id")
        .to_string()
}

impl TempHome {
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("harness-it-{tag}-{pid}-{nanos}-{n}"));
        let home = base.join("home");
        let firm_home = home.join(".firm");
        std::fs::create_dir_all(&firm_home).expect("create temp harness home");
        // Canonicalize HOME so the binary's `project_id_for_path` (which
        // canonicalizes) derives slugs against the same root the tests assert on.
        let home = std::fs::canonicalize(&home).expect("canonicalize home");
        let firm_home = home.join(".firm");
        Self {
            base,
            home,
            firm_home,
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn firm_home(&self) -> &Path {
        &self.firm_home
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.firm_home.join("projects")
    }

    pub fn registry_path(&self) -> PathBuf {
        self.projects_dir().join("registry.json")
    }

    pub fn active_marker_path(&self) -> PathBuf {
        self.firm_home.join("ACTIVE_PROJECT")
    }

    pub fn spaces_dir(&self) -> PathBuf {
        self.firm_home.join("execution-spaces")
    }

    pub fn space_registry_path(&self) -> PathBuf {
        self.spaces_dir().join("registry.json")
    }

    pub fn active_space_marker_path(&self) -> PathBuf {
        self.firm_home.join("ACTIVE_SPACE")
    }

    /// Env pairs to pass to a spawned `harness` process.
    pub fn envs(&self) -> Vec<(String, String)> {
        let mut envs = vec![
            ("HOME".to_string(), self.home.display().to_string()),
            (
                "FIRM_HOME".to_string(),
                self.firm_home.display().to_string(),
            ),
        ];
        envs.extend(
            INHERITED_NATIVE_FIRM_ENV
                .iter()
                .filter(|key| **key != "FIRM_ROOT")
                .map(|key| ((*key).to_string(), String::new())),
        );
        envs
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        // Some focused integration fixtures start a real machine-scoped
        // NodeDaemon only to provide the parent execution authority. Stop it
        // before deleting its FIRM_HOME so no detached test process or socket
        // can outlive the isolated fixture.
        if self.firm_home.join("NODE_ID").is_file() {
            let mut stop = Command::new(env!("CARGO_BIN_EXE_firm"));
            stop.args(["daemon", "stop"])
                .current_dir(&self.base)
                .envs(self.envs())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            clear_inherited_native_firm_env(&mut stop);
            let _ = stop.output();
        }
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// ---------------------------------------------------------------------------
// Live-serve test harness: spawn the real `harness serve` binary on an ephemeral
// port against an isolated HOME, then drive it over raw HTTP/SSE. Used by the
// serve-api / sse-multiplex / project-convergence integration tests.
// ---------------------------------------------------------------------------

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const INHERITED_NATIVE_FIRM_ENV: &[&str] = &[
    "FIRM_ROOT",
    "FIRM_PROJECT",
    "FIRM_PROJECT_ID",
    "FIRM_SPACE",
    "FIRM_COMPANY",
    "FIRM_MISSION_ID",
    "FIRM_ORIGIN_WAVE_ID",
    "FIRM_TEAM_RUN_ID",
    "FIRM_MEMBER_RUN_ID",
    "FIRM_AGENT_MEMBER_ID",
    "FIRM_WORK_ID",
    // Backward compat: old HARNESS_ env vars
    "HARNESS_ROOT",
    "HARNESS_PROJECT",
    "HARNESS_PROJECT_ID",
    "HARNESS_SPACE",
    "HARNESS_COMPANY",
    "HARNESS_MISSION_ID",
    "HARNESS_ORIGIN_WAVE_ID",
    "HARNESS_TEAM_RUN_ID",
    "HARNESS_MEMBER_RUN_ID",
    "HARNESS_AGENT_MEMBER_ID",
    "HARNESS_WORK_ID",
    "HARNESS_HOME",
];

pub fn clear_inherited_native_firm_env(command: &mut Command) {
    for key in INHERITED_NATIVE_FIRM_ENV {
        command.env_remove(key);
    }
}

/// Seed one historical Mission row directly into the Execution Space ledger.
/// DOC-108 retired the `mission create` writer on every surface, so tests
/// that need pre-cutover Mission history (legacy reads, `legacy_mission_id`
/// provenance on a Team) write the row directly instead of calling the CLI.
pub fn seed_historical_mission(home: &TempHome, space_id: &str, id: &str, title: &str) {
    use std::io::Write as _;

    let path = home.spaces_dir().join(space_id).join("missions.jsonl");
    // Space stores are created lazily on first write; fixture seeding is that
    // first write for pre-cutover Mission history.
    std::fs::create_dir_all(path.parent().expect("mission ledger parent"))
        .expect("create space store dir");
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

pub fn latest_works(home: &TempHome, project_id: &str) -> Vec<serde_json::Value> {
    let operations = std::fs::read_to_string(
        home.spaces_dir()
            .join(project_id)
            .join("work_operations.jsonl"),
    )
    .expect("work operations");
    let mut order = Vec::<String>::new();
    let mut by_id = std::collections::HashMap::<String, serde_json::Value>::new();
    for line in operations.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).expect("work operation JSON");
        let work = row["work"].clone();
        let id = work["id"].as_str().expect("Work id").to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, work);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// A spawned `harness serve` child bound to `127.0.0.1:<port>`. Killed on drop.
pub struct ServeHandle {
    child: Child,
    node_daemon: Option<Child>,
    port: u16,
    fixture_store_root: PathBuf,
    fixture_execution_space_id: String,
    fixture_mutation_token: String,
}

impl ServeHandle {
    pub fn fixture_store_root(&self) -> &std::path::Path {
        &self.fixture_store_root
    }

    /// Spawn `harness serve` from `cwd` against `home`, on a free ephemeral port.
    /// Extra env can pin `--project`/`FIRM_PROJECT` via the args/env.
    pub fn spawn(home: &TempHome, cwd: &Path, extra_args: &[&str]) -> Self {
        Self::spawn_with_env(home, cwd, extra_args, &[])
    }

    /// Spawn serve with additional environment entries. Provider-execution
    /// tests use this to place deterministic adapter shims on PATH without
    /// mutating the parent test process.
    pub fn spawn_with_env(
        home: &TempHome,
        cwd: &Path,
        extra_args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> Self {
        let fixture_execution_space_id = extra_args
            .windows(2)
            .find_map(|pair| (pair[0] == "--space").then(|| pair[1].to_string()))
            .or_else(|| {
                extra_env.iter().find_map(|(key, value)| {
                    (*key == "FIRM_SPACE" && !value.is_empty()).then(|| (*value).to_string())
                })
            })
            .unwrap_or_else(|| current_space_id(home));
        let fixture_store_root = home.spaces_dir().join(&fixture_execution_space_id);
        let fixture_mutation_token = extra_env
            .iter()
            .find_map(|(key, value)| {
                if *key != "AGENTFIRM_HTTP_CREDENTIALS_JSON" || value.is_empty() {
                    return None;
                }
                serde_json::from_str::<serde_json::Value>(value)
                    .ok()?
                    .as_array()?
                    .first()?
                    .get("token")?
                    .as_str()
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "integration-test-http-token".to_string());
        let node_daemon = if home.firm_home().join("NODE_ID").exists() {
            let mut daemon = Command::new(env!("CARGO_BIN_EXE_firm"));
            daemon
                .args([
                    "daemon",
                    "serve",
                    "--max-concurrency",
                    "8",
                    "--idle-timeout-secs",
                    "30",
                    "--scan-interval-secs",
                    "1",
                ])
                .current_dir(cwd)
                .envs(home.envs())
                .stdin(Stdio::null())
                .stdout(Stdio::null());
            if std::env::var_os("FIRM_TEST_NODE_DAEMON_STDERR").is_some() {
                daemon.stderr(Stdio::inherit());
            } else {
                daemon.stderr(Stdio::null());
            }
            clear_inherited_native_firm_env(&mut daemon);
            for (key, value) in extra_env {
                daemon.env(key, value);
            }
            Some(daemon.spawn().expect("spawn NodeDaemon"))
        } else {
            None
        };
        if node_daemon.is_some() {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let mut status = Command::new(env!("CARGO_BIN_EXE_firm"));
                status
                    .args(["daemon", "status"])
                    .current_dir(cwd)
                    .envs(home.envs());
                clear_inherited_native_firm_env(&mut status);
                let ready = status.output().is_ok_and(|output| {
                    output.status.success()
                        && !String::from_utf8_lossy(&output.stdout).contains("absent")
                });
                if ready {
                    break;
                }
                assert!(Instant::now() < deadline, "NodeDaemon did not become ready");
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        let port = free_port();
        let addr = format!("127.0.0.1:{port}");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_firm"));
        cmd.arg("serve").arg("--addr").arg(&addr);
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.current_dir(cwd).envs(home.envs());
        clear_inherited_native_firm_env(&mut cmd);
        cmd.env(
            "AGENTFIRM_HTTP_CREDENTIALS_JSON",
            serde_json::json!([{
                "token": fixture_mutation_token.as_str(),
                "actor": {"kind": "service", "id": "integration-test-fixture"},
                "authority_actors": []
            }])
            .to_string(),
        );
        // Production supervisors never retire an idle Member implicitly.
        // Integration processes need a bounded escape after they have
        // asserted the idle state so test teardown can join cleanly.
        cmd.env("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "250");
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let child = cmd.spawn().expect("spawn harness serve");
        let handle = Self {
            child,
            node_daemon,
            port,
            fixture_store_root,
            fixture_execution_space_id,
            fixture_mutation_token,
        };
        handle.wait_until_ready();
        handle
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Poll the port until the server accepts and answers `/health`.
    fn wait_until_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok((status, body)) = self.try_get("/health") {
                if status == 200 && body.contains("\"status\"") {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("harness serve did not become ready on {}", self.addr());
    }

    /// GET a path, returning (status_code, body). Errors propagate (used by the
    /// readiness poll); production calls use `get`.
    fn try_get(&self, path: &str) -> std::io::Result<(u16, String)> {
        let mut stream = TcpStream::connect(self.addr())?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )?;
        let mut raw = String::new();
        read_http_to_string(&mut stream, &mut raw)?;
        Ok(split_status_body(&raw))
    }

    /// GET a path, returning (status_code, parsed JSON body).
    pub fn get_json(&self, path: &str) -> (u16, serde_json::Value) {
        let (status, body) = self.try_get(path).expect("GET request");
        let json = serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("GET {path} body not JSON ({e}): {body}"));
        (status, json)
    }

    /// GET JSON with explicit request headers (for authenticated read models).
    pub fn get_json_with_headers(
        &self,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (u16, serde_json::Value) {
        let mut stream = TcpStream::connect(self.addr()).expect("connect get");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n").expect("write get");
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").expect("write header");
        }
        write!(stream, "Connection: close\r\n\r\n").expect("finish get");
        let mut raw = String::new();
        read_http_to_string(&mut stream, &mut raw).expect("read get");
        let (status, body) = split_status_body(&raw);
        let json = serde_json::from_str(&body)
            .unwrap_or_else(|error| panic!("GET {path} body not JSON ({error}): {body}"));
        (status, json)
    }

    /// GET a path, returning (status_code, raw response INCLUDING headers) —
    /// for content-type assertions on non-JSON responses (e.g. HTML pages).
    pub fn get_raw(&self, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(self.addr()).expect("connect get");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .expect("write get");
        let mut raw = String::new();
        read_http_to_string(&mut stream, &mut raw).expect("read get");
        (split_status_body(&raw).0, raw)
    }

    /// POST a JSON body to a path, returning (status_code, parsed JSON body).
    pub fn post_json(&self, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
        if path == "/v1/team-runs" || path.starts_with("/v1/team-runs?") {
            let prepared = self.prepare_canonical_team_run_fixture(path, body);
            return self.post_json_with_header(path, &prepared, None);
        }
        if path.starts_with("/v1/team-runs/") && path.ends_with("/messages") {
            return self.post_canonical_team_message_fixture(path, body);
        }
        self.post_json_with_header(path, body, None)
    }

    fn post_canonical_team_message_fixture(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> (u16, serde_json::Value) {
        use harness_store::HarnessStore;

        let team_run_id = path
            .trim_matches('/')
            .split('/')
            .nth(2)
            .expect("team-run id");
        let store = HarnessStore::new(&self.fixture_store_root);
        let Some(run) = store
            .team_runs()
            .expect("read TeamRuns")
            .into_iter()
            .rev()
            .find(|run| run.id == team_run_id)
        else {
            return (
                409,
                serde_json::json!({"ok":false,"error":{"code":"TEAM_RUN_NOT_FOUND"}}),
            );
        };
        let host_identity_id = store
            .latest_teams()
            .expect("read AgentTeams")
            .remove(&run.agent_team_id)
            .expect("TeamRun AgentTeam")
            .host_agent_id;
        let runs = store
            .trust_member_runs(&self.fixture_execution_space_id)
            .expect("read canonical MemberRuns");
        let recipient_ids = body
            .get("recipient_runtime_ids")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let recipients = recipient_ids
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|recipient| {
                let stable_id = if recipient == "host" {
                    host_identity_id.as_str()
                } else {
                    runs.iter()
                        .find(|run| run.id == recipient)
                        .map(|run| run.agent_member_id.as_str())
                        .unwrap_or(recipient)
                };
                serde_json::json!({"kind": "agent_identity", "id": stable_id})
            })
            .collect::<Vec<_>>();
        let generation = COUNTER.fetch_add(1, Ordering::SeqCst);
        let message_id = format!("message-it-{generation}");
        let kind = match body
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("message")
        {
            "control" => "request_decision",
            other => {
                assert_eq!(other, "message", "unsupported legacy fixture message kind");
                "message"
            }
        };
        let correlation_id = body
            .get("correlation_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                let cause = body.get("causation_id")?.as_str()?;
                store
                    .fabric_messages(&self.fixture_execution_space_id)
                    .ok()?
                    .into_iter()
                    .find(|message| message.id == cause)
                    .map(|message| message.correlation_id)
            })
            .unwrap_or_else(|| message_id.clone());
        let target_ref = recipients
            .first()
            .cloned()
            .expect("canonical message fixture requires a recipient");
        let request = serde_json::json!({
            "target_node_id": run.execution_node_id,
            "command": "author_message",
            "expires_unix_ms": unix_ms() + 30_000,
            "payload": {"draft": {
                "address_kind": if recipients.len() == 1 { "direct_agent" } else { "authorized_broadcast" },
                "target_ref": target_ref,
                "recipients": recipients,
                "team_id": run.agent_team_id,
                "team_run_id": team_run_id,
                "work_id": body.get("work_id").cloned().unwrap_or(serde_json::Value::Null),
                "kind": kind,
                "body": body.get("body").and_then(serde_json::Value::as_str).unwrap_or("test message"),
                "correlation_id": correlation_id,
                "causation_id": body.get("causation_id").cloned().unwrap_or(serde_json::Value::Null),
                "response_intent": body
                    .get("response_intent")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| {
                        if body.get("sender_runtime_id").and_then(serde_json::Value::as_str) == Some("host") {
                            "response_required"
                        } else {
                            "informational"
                        }
                    }),
                "evidence_refs": body.get("evidence_refs").cloned().unwrap_or_else(|| serde_json::json!([])),
                "schema_version": 1
            }}
        });
        let expected = "0";
        let headers = [
            ("X-AgentFirm-Token", self.fixture_mutation_token.as_str()),
            ("Idempotency-Key", message_id.as_str()),
            ("If-Match", expected),
        ];
        let (status, response) =
            self.post_json_with_headers("/v1/agentfirm/runtime-commands", &request, &headers);
        if status == 200 && response["ok"].as_bool() == Some(true) {
            let mut result = response
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if let Some(result) = result.as_object_mut() {
                result.insert("status".into(), serde_json::Value::String("queued".into()));
                result.insert(
                    "deliveries".into(),
                    serde_json::json!([{"status": "queued", "attempt": 1}]),
                );
            }
            return (
                status,
                serde_json::json!({
                    "ok": true,
                    "result": result,
                    "canonical": response,
                }),
            );
        }
        (if status == 200 { 409 } else { status }, response)
    }

    /// Older runtime integration scenarios describe provider inputs inline.
    /// Production now requires every input to reference a pre-existing
    /// canonical AgentMember that belongs to the selected flat Team. Preserve
    /// those scenarios by materializing their prerequisites in the isolated
    /// test store before sending the unchanged production request shape.
    fn prepare_canonical_team_run_fixture(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> serde_json::Value {
        use harness_core::agentfirm_api::{
            ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, MutationContext,
            PermissionCeiling, TeamMembership, TeamMembershipRole, TeamMembershipStatus,
        };
        use harness_store::HarnessStore;

        let mut prepared = body.clone();
        let Some(object) = prepared.as_object_mut() else {
            return prepared;
        };
        let execution_space_id = path
            .split_once('?')
            .and_then(|(_, query)| {
                query.split('&').find_map(|pair| {
                    pair.split_once('=')
                        .and_then(|(key, value)| (key == "space").then(|| value.to_string()))
                })
            })
            .unwrap_or_else(|| self.fixture_execution_space_id.clone());
        let store_root = self
            .fixture_store_root
            .parent()
            .expect("execution-spaces root")
            .join(&execution_space_id);
        let store = HarnessStore::new(store_root);
        let teams = store.latest_teams().expect("read fixture AgentTeams");
        let team_id = object
            .get("agent_team_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| (teams.len() == 1).then(|| teams.keys().next().unwrap().clone()));
        let Some(team_id) = team_id else {
            return prepared;
        };
        object
            .entry("agent_team_id".to_string())
            .or_insert_with(|| serde_json::Value::String(team_id.clone()));
        let team = teams.get(&team_id).cloned().expect("fixture AgentTeam");
        let needs_host = !object
            .get("members")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|members| {
                members.iter().any(|member| {
                    member["agent_member_id"].as_str() == Some(team.host_agent_id.as_str())
                })
            });
        if needs_host {
            object.insert(
                "host_runtime_mode".into(),
                serde_json::Value::String("external_interactive".into()),
            );
        }
        let Some(members) = object
            .get_mut("members")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return prepared;
        };
        let creator = ActorRef {
            kind: ActorKind::Service,
            id: "integration-test-fixture".into(),
        };
        let fixture_generation = COUNTER.fetch_add(1, Ordering::SeqCst);
        for (index, member) in members.iter_mut().enumerate() {
            let Some(member_object) = member.as_object_mut() else {
                continue;
            };
            if member_object.get("agent_member_id").is_some() {
                continue;
            }
            let name = member_object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("member");
            let role = member_object
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("worker");
            let provider = member_object
                .get("provider")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("codex");
            let safe_name = name
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                .collect::<String>();
            let id = format!("agent-it-{fixture_generation}-{index}-{safe_name}");
            let now = format!("unix-ms:{}", unix_ms());
            store
                .create_trust_agent_member(
                    &MutationContext {
                        execution_space_id: execution_space_id.clone(),
                        authenticated_actor: creator.clone(),
                        authority_actor: None,
                        command_name: "integration_test.agent_member.create".into(),
                        idempotency_key: format!("integration-test-create-{id}"),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    AgentMember {
                        id: id.clone(),
                        name: name.to_string(),
                        description: "canonical integration-test AgentMember".into(),
                        role: role.to_string(),
                        capabilities: Vec::new(),
                        skill_refs: Vec::new(),
                        provider_profile_ref: Some(provider.to_string()),
                        model_preference: member_object
                            .get("model")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        workspace_policy: "trusted-development-explicit-cwd".into(),
                        permission_ceiling: PermissionCeiling::FullAccess,
                        organization_status: AgentMemberOrganizationStatus::Active,
                        version: 1,
                        created_by: creator.clone(),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    },
                )
                .expect("create canonical fixture AgentMember");
            store
                .join_team_membership(
                    &MutationContext {
                        execution_space_id: execution_space_id.clone(),
                        authenticated_actor: creator.clone(),
                        authority_actor: None,
                        command_name: "integration_test.team_membership.join".into(),
                        idempotency_key: format!("integration-test-membership-{team_id}-{id}"),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    TeamMembership {
                        id: format!("membership:{team_id}:{id}"),
                        team_id: team_id.clone(),
                        agent_member_id: id.clone(),
                        node_id: team.node_id.clone(),
                        role: TeamMembershipRole::Member,
                        state: TeamMembershipStatus::Active,
                        membership_generation: 1,
                        default_subscription_refs: Vec::new(),
                        created_by: creator.clone(),
                        revision: 1,
                        joined_at: now,
                        left_at: None,
                    },
                )
                .expect("create durable fixture TeamMembership");
            member_object.insert("agent_member_id".into(), serde_json::Value::String(id));
        }
        if !members
            .iter()
            .any(|member| member["agent_member_id"].as_str() == Some(team.host_agent_id.as_str()))
        {
            let host = store
                .trust_agent_members(&execution_space_id)
                .expect("read fixture AgentMembers")
                .into_iter()
                .find(|member| member.id == team.host_agent_id)
                .expect("fixture Team has exact Host AgentMember");
            let provider = host
                .provider_profile_ref
                .as_deref()
                .expect("fixture Host has provider profile");
            members.push(serde_json::json!({
                "agent_member_id": host.id,
                "name": host.name,
                "role": "host",
                "provider": provider,
                "execution_mode": "external_interactive",
            }));
        }
        prepared
    }

    /// POST JSON with the server-held Company OS capability token.
    pub fn post_json_with_token(
        &self,
        path: &str,
        body: &serde_json::Value,
        token: &str,
    ) -> (u16, serde_json::Value) {
        self.post_json_with_header(path, body, Some(token))
    }

    /// POST JSON with explicit test-owned headers. Values are restricted to
    /// single HTTP header lines so acceptance tests cannot smuggle a second
    /// request through this helper.
    pub fn post_json_with_headers(
        &self,
        path: &str,
        body: &serde_json::Value,
        headers: &[(&str, &str)],
    ) -> (u16, serde_json::Value) {
        let payload = body.to_string();
        let mut stream = TcpStream::connect(self.addr()).expect("connect post");
        stream
            // Authenticated Role Action requests can legitimately wait for a
            // serialized Store mutation under slower CI runners. Match the
            // ordinary POST helper's bounded response window; this never
            // retries or resends the mutation.
            .set_read_timeout(Some(Duration::from_secs(15)))
            .expect("timeout");
        let mut header_lines = String::new();
        for (name, value) in headers {
            assert!(
                !name.contains(['\r', '\n', ':']),
                "invalid test header name"
            );
            assert!(!value.contains(['\r', '\n']), "invalid test header value");
            header_lines.push_str(name);
            header_lines.push_str(": ");
            header_lines.push_str(value);
            header_lines.push_str("\r\n");
        }
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{header_lines}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .expect("write post");
        let mut raw = String::new();
        read_http_to_string(&mut stream, &mut raw).expect("read post");
        let (status, text) = split_status_body(&raw);
        let json = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("POST {path} body not JSON ({error}): {text}"));
        (status, json)
    }

    fn post_json_with_header(
        &self,
        path: &str,
        body: &serde_json::Value,
        token: Option<&str>,
    ) -> (u16, serde_json::Value) {
        // TeamRun regression suites can seed one explicit flat fixture Team.
        // If present, make legacy HTTP scenarios name it without introducing a
        // production fallback for missing AgentTeam identity.
        let mut normalized_body = body.clone();
        if path == "/v1/team-runs" && body.get("agent_team_id").is_none() {
            if let Ok((200, snapshot)) = self.try_get("/v1/snapshot") {
                if let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&snapshot) {
                    let has_fixture = snapshot["teams"].as_array().is_some_and(|teams| {
                        teams
                            .iter()
                            .any(|team| team["id"].as_str() == Some("team-runtime-fixture"))
                    });
                    if has_fixture {
                        normalized_body["agent_team_id"] =
                            serde_json::Value::String("team-runtime-fixture".to_string());
                        if let Some(object) = normalized_body.as_object_mut() {
                            object.remove("mission_id");
                            object.remove("wave_id");
                        }
                    }
                }
            }
        }
        let payload = normalized_body.to_string();
        let mut stream = TcpStream::connect(self.addr()).expect("connect post");
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .expect("timeout");
        let token_header = token
            .map(|value| format!("X-Harness-Company-OS-Token: {value}\r\n"))
            .unwrap_or_default();
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{token_header}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .expect("write post");
        let mut raw = String::new();
        read_http_to_string(&mut stream, &mut raw).expect("read post");
        let (status, text) = split_status_body(&raw);
        let json = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("POST {path} body not JSON ({e}): {text}"));
        (status, json)
    }

    /// Open an SSE stream to `/v1/events[?project=<id>]`, returning a reader the
    /// caller can pull `event:`/`data:` lines from. The connection stays open
    /// (no `Connection: close`) so live frames arrive as they are broadcast.
    pub fn open_sse(&self, query: &str) -> BufReader<TcpStream> {
        self.open_sse_with_token(query, None)
    }

    /// Open one authenticated SSE stream. Private provider activity is
    /// delivered only when this token resolves to the exact owning Member.
    pub fn open_sse_with_token(&self, query: &str, token: Option<&str>) -> BufReader<TcpStream> {
        let stream = TcpStream::connect(self.addr()).expect("connect sse");
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .expect("sse timeout");
        let mut writer = stream.try_clone().expect("clone sse");
        let token_header = token
            .map(|token| format!("X-AgentFirm-Token: {token}\r\n"))
            .unwrap_or_default();
        write!(
            writer,
            "GET /v1/events{query} HTTP/1.1\r\nHost: localhost\r\n{token_header}\r\n"
        )
        .expect("write sse req");
        let mut reader = BufReader::new(stream);
        // Drain through the initial `snapshot` frame so the caller starts reading
        // at live frames.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || Instant::now() > deadline {
                break;
            }
            if line.contains("event: snapshot") {
                // consume the following data line + blank line, then return.
                let mut data = String::new();
                let _ = reader.read_line(&mut data);
                if query.contains("agent_id=") {
                    let snapshot = data
                        .strip_prefix("data: ")
                        .and_then(|value| {
                            serde_json::from_str::<serde_json::Value>(value.trim()).ok()
                        })
                        .expect("authenticated SSE snapshot JSON");
                    assert_eq!(
                        snapshot["team_session_provider_activity"], true,
                        "Team-scoped SSE did not bind the selected provider stream"
                    );
                }
                let mut blank = String::new();
                let _ = reader.read_line(&mut blank);
                break;
            }
        }
        reader
    }
}

impl Drop for ServeHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(daemon) = self.node_daemon.as_mut() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
    }
}

/// Read SSE frames from a reader for up to `timeout`, returning every `data:` JSON
/// payload seen (one per `data:` line). Keepalive comments and event lines are
/// skipped. Stops early once `min` payloads are collected.
pub fn collect_sse_data(
    reader: &mut BufReader<TcpStream>,
    timeout: Duration,
    min: usize,
) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    while Instant::now() < deadline && out.len() < min {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(rest) = line.strip_prefix("data: ") {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest.trim()) {
                        out.push(v);
                    }
                }
            }
            Err(_) => break, // read timeout
        }
    }
    out
}

/// Collect one named SSE event family while unrelated durable projection
/// traffic continues on the same stream. A provider-live collection ends at
/// the explicit terminal clear rather than an arbitrary total frame count.
pub fn collect_named_sse_data(
    reader: &mut BufReader<TcpStream>,
    timeout: Duration,
    event_name: &str,
) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    let mut current_event = None::<String>;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(name) = line.strip_prefix("event: ") {
                    current_event = Some(name.trim().to_string());
                } else if current_event.as_deref() == Some(event_name) {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data.trim()) {
                            let terminal = value["reason"] == "terminal";
                            out.push(value);
                            if terminal {
                                break;
                            }
                        }
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    out
}

/// Find a free TCP port by binding to :0 and reading the assigned port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Linux may report `ECONNRESET`, `EAGAIN`, or a timeout after the server has
/// already written a complete `Connection: close` response. Accept that
/// transport ending only when the declared Content-Length is fully present;
/// never retry a mutation.
fn read_http_to_string(stream: &mut TcpStream, raw: &mut String) -> std::io::Result<()> {
    match stream.read_to_string(raw) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
            ) && complete_http_response(raw) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn complete_http_response(raw: &str) -> bool {
    let Some((headers, body)) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
    else {
        return false;
    };
    let Some(content_length) = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) else {
        return false;
    };
    body.len() >= content_length
}

/// Split a raw HTTP response into (status_code, body). Tolerant of either CRLF or
/// LF header separators.
fn split_status_body(raw: &str) -> (u16, String) {
    let status = raw
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

/// Run `harness <args...>` from `cwd` against `home`; return its Output.
pub fn run_firm(home: &TempHome, cwd: &Path, args: &[&str]) -> std::process::Output {
    run_firm_with_env(home, cwd, args, &[])
}

/// Register the canonical durable AgentMember identity used by Team fixtures.
/// Provider execution details remain on the later TeamMember/MemberRun input;
/// this helper never recreates the retired runtime-heavy `agent create` row.
#[allow(clippy::too_many_arguments)]
pub fn create_canonical_agent_member(
    home: &TempHome,
    cwd: &Path,
    project_id: &str,
    id: &str,
    name: &str,
    role: &str,
    provider: &str,
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let payload = serde_json::json!({
        "command": "create_agent_member",
        "member": {
            "id": id,
            "name": name,
            "description": "canonical integration-test AgentMember",
            "role": role,
            "capabilities": [],
            "skill_refs": [],
            "provider_profile_ref": provider,
            "model_preference": null,
            "workspace_policy": "managed-worktree",
            "permission_ceiling": "full_access",
            "organization_status": "active",
            "version": 1,
            "created_by": { "kind": "service", "id": "integration-test" },
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }
    })
    .to_string();
    run_firm_with_env(
        home,
        cwd,
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
            &format!("integration-test-create-{id}"),
            "--expected-version",
            "0",
            "--json",
            &payload,
        ],
        extra_env,
    )
}

/// Run `harness <args...>` from `cwd` against `home` with explicit additional
/// environment variables.
pub fn run_firm_with_env(
    home: &TempHome,
    cwd: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_firm"));
    let fixture_team_exists = home.spaces_dir().exists()
        && std::fs::read_dir(home.spaces_dir())
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                ["agentfirm_trust_operations.jsonl", "teams.jsonl"]
                    .iter()
                    .any(|ledger| {
                        std::fs::read_to_string(entry.path().join(ledger))
                            .is_ok_and(|rows| rows.contains("team-runtime-fixture"))
                    })
            });
    let is_team_run_create = args
        .windows(2)
        .any(|window| window == ["team-run", "create"]);
    let has_team = args.contains(&"--agent-team-id");
    let mut normalized = Vec::with_capacity(args.len() + 6);
    let mut skip_legacy_value = false;
    for a in args {
        if skip_legacy_value {
            skip_legacy_value = false;
            continue;
        }
        if fixture_team_exists && is_team_run_create && (*a == "--mission-id" || *a == "--wave-id")
        {
            skip_legacy_value = true;
            continue;
        }
        normalized.push(*a);
    }
    if fixture_team_exists && is_team_run_create && !has_team {
        normalized.push("--agent-team-id");
        normalized.push("team-runtime-fixture");
    }
    if fixture_team_exists && is_team_run_create {
        let has_host = args
            .iter()
            .any(|arg| arg.starts_with("agent-runtime-host:"));
        if !has_host {
            normalized.push("--host-runtime-mode");
            normalized.push("external_interactive");
            normalized.push("--member");
            normalized.push("agent-runtime-host:host:kimi/external_interactive");
        }
    }
    for a in normalized {
        cmd.arg(a);
    }
    let command = cmd.current_dir(cwd).envs(home.envs());
    clear_inherited_native_firm_env(command);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run harness")
}

/// Read the current project id from the registry written under `home`.
pub fn current_project_id(home: &TempHome) -> String {
    let registry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.registry_path()).expect("registry"))
            .expect("parse registry");
    registry["current_project_id"]
        .as_str()
        .expect("current_project_id")
        .to_string()
}

pub fn membership_id_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    member_run_id: &str,
) -> String {
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let member_run = store
        .trust_member_runs(execution_space_id)
        .expect("read fixture MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("fixture MemberRun");
    let team_run = store
        .team_runs()
        .expect("read fixture TeamRuns")
        .into_iter()
        .rev()
        .find(|run| run.id == member_run.team_run_id)
        .expect("fixture TeamRun");
    let matches = store
        .fabric_team_memberships(execution_space_id)
        .expect("read fixture TeamMemberships")
        .into_iter()
        .filter(|membership| {
            membership.team_id == team_run.agent_team_id
                && membership.agent_member_id == member_run.agent_member_id
                && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
        })
        .collect::<Vec<_>>();
    let [membership] = matches.as_slice() else {
        panic!("fixture MemberRun must resolve one active TeamMembership");
    };
    membership.id.clone()
}

pub fn assign_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    bind_execution: bool,
) -> harness_core::Work {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus,
        MutationContext, PermissionCeiling, RuntimeActivity, RuntimeCommandBinding,
        RuntimeDriverRef, RuntimeResidency, WorkExecutionBinding, WorkExecutionBindingStatus,
    };
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let member = store
        .trust_member_runs(execution_space_id)
        .expect("read fixture MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("fixture MemberRun");
    let team_run = store
        .team_runs()
        .expect("read fixture TeamRuns")
        .into_iter()
        .rev()
        .find(|run| run.id == member.team_run_id)
        .expect("fixture TeamRun");
    let work = store
        .latest_works()
        .expect("read fixture Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("fixture Work");
    let membership_id = membership_id_for_member_run(home, execution_space_id, member_run_id);
    let work = if work.assignee_membership_id.as_deref() == Some(membership_id.as_str()) {
        work
    } else {
        store
            .assign_work_to_membership(
                &work.id,
                work.version,
                &membership_id,
                execution_space_id,
                harness_core::WorkCommandContext {
                    event_id: format!("test-assign-{work_id}"),
                    performed_by_actor: team_run.host_actor.clone().expect("exact fixture Host"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("test-assign-{work_id}"),
                    created_at: "unix-ms:test-assign".into(),
                    duplicate_ok: false,
                },
            )
            .expect("assign fixture Work responsibility")
    };
    if !bind_execution {
        return work;
    }
    let now = unix_ms();
    let daemon = store
        .latest_node_daemon_lease(&team_run.execution_node_id)
        .expect("read fixture NodeDaemon lease")
        .unwrap_or_else(|| {
            store
                .acquire_node_daemon_lease(
                    &team_run.execution_node_id,
                    "test-node-daemon",
                    "test-node-daemon-instance",
                    now,
                    60_000,
                )
                .expect("acquire fixture NodeDaemon lease")
        });
    let session = store
        .fabric_agent_sessions(execution_space_id)
        .expect("read fixture AgentSessions")
        .into_iter()
        .find(|session| {
            session.agent_member_id == member.agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
        })
        .unwrap_or_else(|| {
            let session = AgentSession {
                id: format!("test-session:{}", member.agent_member_id),
                agent_member_id: member.agent_member_id.clone(),
                node_id: team_run.execution_node_id.clone(),
                execution_space_id: execution_space_id.into(),
                node_daemon_id: daemon.daemon_id.clone(),
                node_daemon_generation: daemon.generation,
                provider_kind: member
                    .provider_profile_snapshot
                    .clone()
                    .unwrap_or_else(|| "codex".into()),
                provider_profile_ref: "test".into(),
                permission_envelope_ref: format!("test-permission:{}", member.agent_member_id),
                effective_permission_ceiling: PermissionCeiling::FullAccess,
                workspace_cwd: Some(
                    std::fs::canonicalize(home.base())
                        .expect("canonical fixture workspace")
                        .to_string_lossy()
                        .into_owned(),
                ),
                lifecycle: AgentSessionStatus::Idle,
                runtime_generation: 1,
                control_state: AgentSessionControlState {
                    driver_generation: 1,
                    driver_ref: RuntimeDriverRef::NodeDaemon {
                        node_daemon_id: daemon.daemon_id.clone(),
                        node_daemon_generation: daemon.generation,
                    },
                    composition_fingerprint: Some("test-composition-v1".into()),
                    capability_fingerprint: Some("test-capability-v1".into()),
                    runtime_residency: RuntimeResidency::Detached,
                    activity: RuntimeActivity::Idle,
                    ..Default::default()
                },
                native_session_ref: None,
                current_turn_id: None,
                queued_input_count: 0,
                version: 1,
                opened_at: "unix-ms:test-session".into(),
                last_active_at: "unix-ms:test-session".into(),
                closed_at: None,
            };
            store
                .create_agent_session(
                    &MutationContext {
                        execution_space_id: execution_space_id.into(),
                        authenticated_actor: ActorRef {
                            kind: ActorKind::Service,
                            id: daemon.daemon_id.clone(),
                        },
                        authority_actor: None,
                        command_name: "test.session.create".into(),
                        idempotency_key: session.id.clone(),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    session.clone(),
                )
                .expect("create fixture AgentSession");
            session
        });
    let binding_id = format!("work-binding:{work_id}:1");
    store
        .bind_responsible_work_execution(
            &MutationContext {
                execution_space_id: execution_space_id.into(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: daemon.daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "test.work.bind".into(),
                idempotency_key: binding_id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &RuntimeCommandBinding {
                target_member_run_id: Some(member.id.clone()),
                target_member_run_generation: Some(member.runtime_generation),
                target_session_id: Some(session.id.clone()),
                target_runtime_generation: Some(session.runtime_generation),
                target_driver_generation: Some(session.control_state.driver_generation),
                target_driver: session.control_state.driver_ref.clone(),
                native_session_ref: session.native_session_ref.clone(),
                composition_fingerprint: session.control_state.composition_fingerprint.clone(),
                capability_fingerprint: session.control_state.capability_fingerprint.clone(),
                permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
                ..Default::default()
            },
            WorkExecutionBinding {
                id: binding_id,
                work_id: work.id.clone(),
                work_revision: work.version,
                team_id: team_run.agent_team_id,
                team_membership_id: membership_id,
                agent_member_id: member.agent_member_id,
                agent_session_id: session.id,
                agent_session_generation: session.runtime_generation,
                delivery_id: format!("work-delivery:{work_id}:1"),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: ActorRef {
                    kind: ActorKind::Service,
                    id: daemon.daemon_id,
                },
                bound_at: "unix-ms:test-bind".into(),
                ended_at: None,
            },
        )
        .expect("bind fixture Work execution");
    work
}
