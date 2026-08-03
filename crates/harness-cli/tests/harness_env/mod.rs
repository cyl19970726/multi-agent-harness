//! Shared test helper: an isolated harness HOME so integration tests never touch
//! the developer's real `~/.harness` (goal-multi-project "Test isolation" risk).
//!
//! `TempHome` creates a unique temp dir, points `HOME` and `HARNESS_HOME` at it,
//! and exposes the registry/marker paths. It is passed to spawned `harness`
//! processes via `.envs(home.envs())`; we never mutate the test process's own env
//! (which would race across parallel tests).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempHome {
    base: PathBuf,
    home: PathBuf,
    harness_home: PathBuf,
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
        let harness_home = home.join(".harness");
        std::fs::create_dir_all(&harness_home).expect("create temp harness home");
        // Canonicalize HOME so the binary's `project_id_for_path` (which
        // canonicalizes) derives slugs against the same root the tests assert on.
        let home = std::fs::canonicalize(&home).expect("canonicalize home");
        let harness_home = home.join(".harness");
        Self {
            base,
            home,
            harness_home,
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn harness_home(&self) -> &Path {
        &self.harness_home
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.harness_home.join("projects")
    }

    pub fn registry_path(&self) -> PathBuf {
        self.projects_dir().join("registry.json")
    }

    pub fn active_marker_path(&self) -> PathBuf {
        self.harness_home.join("ACTIVE_PROJECT")
    }

    pub fn spaces_dir(&self) -> PathBuf {
        self.harness_home.join("execution-spaces")
    }

    pub fn space_registry_path(&self) -> PathBuf {
        self.spaces_dir().join("registry.json")
    }

    pub fn active_space_marker_path(&self) -> PathBuf {
        self.harness_home.join("ACTIVE_SPACE")
    }

    /// Env pairs to pass to a spawned `harness` process.
    pub fn envs(&self) -> Vec<(String, String)> {
        let mut envs = vec![
            ("HOME".to_string(), self.home.display().to_string()),
            (
                "HARNESS_HOME".to_string(),
                self.harness_home.display().to_string(),
            ),
        ];
        envs.extend(
            INHERITED_NATIVE_HARNESS_ENV
                .iter()
                .filter(|key| **key != "HARNESS_ROOT")
                .map(|key| ((*key).to_string(), String::new())),
        );
        envs
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
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
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub const INHERITED_NATIVE_HARNESS_ENV: &[&str] = &[
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
];

pub fn clear_inherited_native_harness_env(command: &mut Command) {
    for key in INHERITED_NATIVE_HARNESS_ENV {
        command.env_remove(key);
    }
}

/// Reconstruct the latest WorkDelivery projection from crash-atomic Work
/// operations plus later claim/receipt updates. Integration tests use this
/// instead of treating update rows as standalone deliveries.
pub fn work_deliveries(home: &TempHome, project_id: &str) -> Vec<serde_json::Value> {
    let store = home.spaces_dir().join(project_id);
    let mut order = Vec::<String>::new();
    let mut by_id = std::collections::HashMap::<String, serde_json::Value>::new();
    let operations =
        std::fs::read_to_string(store.join("work_operations.jsonl")).expect("work operations");
    for line in operations.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).expect("work operation JSON");
        for delivery in row["deliveries"].as_array().into_iter().flatten() {
            let id = delivery["id"]
                .as_str()
                .expect("WorkDelivery id")
                .to_string();
            if !by_id.contains_key(&id) {
                order.push(id.clone());
            }
            by_id.insert(id, delivery.clone());
        }
    }
    if let Ok(updates) = std::fs::read_to_string(store.join("work_delivery_updates.jsonl")) {
        for line in updates.lines().filter(|line| !line.trim().is_empty()) {
            let update: serde_json::Value =
                serde_json::from_str(line).expect("WorkDelivery update JSON");
            let id = update["delivery_id"]
                .as_str()
                .expect("WorkDelivery update id");
            if let Some(delivery) = by_id.get_mut(id) {
                let object = delivery.as_object_mut().expect("WorkDelivery object");
                for key in [
                    "status",
                    "attempt",
                    "claim_id",
                    "claimed_by_supervisor_id",
                    "claimed_generation",
                    "provider_receipt_id",
                    "updated_at",
                ] {
                    object.insert(key.to_string(), update[key].clone());
                }
            }
        }
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
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
    port: u16,
}

impl ServeHandle {
    /// Spawn `harness serve` from `cwd` against `home`, on a free ephemeral port.
    /// Extra env can pin `--project`/`HARNESS_PROJECT` via the args/env.
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
        let port = free_port();
        let addr = format!("127.0.0.1:{port}");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
        cmd.arg("serve").arg("--addr").arg(&addr);
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.current_dir(cwd).envs(home.envs());
        clear_inherited_native_harness_env(&mut cmd);
        // Production supervisors never retire an idle Member implicitly.
        // Integration processes need a bounded escape after they have
        // asserted the idle state so test teardown can join cleanly.
        cmd.env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "250");
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let child = cmd.spawn().expect("spawn harness serve");
        let handle = Self { child, port };
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
        self.post_json_with_header(path, body, None)
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

    fn post_json_with_header(
        &self,
        path: &str,
        body: &serde_json::Value,
        token: Option<&str>,
    ) -> (u16, serde_json::Value) {
        let payload = body.to_string();
        let mut stream = TcpStream::connect(self.addr()).expect("connect post");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
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
        let stream = TcpStream::connect(self.addr()).expect("connect sse");
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .expect("sse timeout");
        let mut writer = stream.try_clone().expect("clone sse");
        write!(
            writer,
            "GET /v1/events{query} HTTP/1.1\r\nHost: localhost\r\n\r\n"
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

/// Find a free TCP port by binding to :0 and reading the assigned port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Linux may report `ECONNRESET` after the server has already written a
/// complete `Connection: close` response. Accept that transport ending only
/// when the declared Content-Length is fully present; never retry a mutation.
fn read_http_to_string(stream: &mut TcpStream, raw: &mut String) -> std::io::Result<()> {
    match stream.read_to_string(raw) {
        Ok(_) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::ConnectionReset
                && complete_http_response(raw) =>
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
pub fn run_harness(home: &TempHome, cwd: &Path, args: &[&str]) -> std::process::Output {
    run_harness_with_env(home, cwd, args, &[])
}

/// Run `harness <args...>` from `cwd` against `home` with explicit additional
/// environment variables.
pub fn run_harness_with_env(
    home: &TempHome,
    cwd: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    for a in args {
        cmd.arg(a);
    }
    let command = cmd.current_dir(cwd).envs(home.envs());
    clear_inherited_native_harness_env(command);
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
