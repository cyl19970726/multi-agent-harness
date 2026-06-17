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

    /// Env pairs to pass to a spawned `harness` process.
    pub fn envs(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".to_string(), self.home.display().to_string()),
            (
                "HARNESS_HOME".to_string(),
                self.harness_home.display().to_string(),
            ),
        ]
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

/// A spawned `harness serve` child bound to `127.0.0.1:<port>`. Killed on drop.
pub struct ServeHandle {
    child: Child,
    port: u16,
}

impl ServeHandle {
    /// Spawn `harness serve` from `cwd` against `home`, on a free ephemeral port.
    /// `--no-truncate` preserves any pre-seeded `provider_turn_events.jsonl` rows.
    /// Extra env can pin `--project`/`HARNESS_PROJECT` via the args/env.
    pub fn spawn(home: &TempHome, cwd: &Path, extra_args: &[&str]) -> Self {
        let port = free_port();
        let addr = format!("127.0.0.1:{port}");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
        cmd.arg("serve")
            .arg("--addr")
            .arg(&addr)
            .arg("--no-truncate");
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.current_dir(cwd)
            .envs(home.envs())
            .env_remove("HARNESS_ROOT")
            .env_remove("HARNESS_PROJECT");
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
        stream.read_to_string(&mut raw)?;
        Ok(split_status_body(&raw))
    }

    /// GET a path, returning (status_code, parsed JSON body).
    pub fn get_json(&self, path: &str) -> (u16, serde_json::Value) {
        let (status, body) = self.try_get(path).expect("GET request");
        let json = serde_json::from_str(&body)
            .unwrap_or_else(|e| panic!("GET {path} body not JSON ({e}): {body}"));
        (status, json)
    }

    /// POST a JSON body to a path, returning (status_code, parsed JSON body).
    pub fn post_json(&self, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
        let payload = body.to_string();
        let mut stream = TcpStream::connect(self.addr()).expect("connect post");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .expect("write post");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read post");
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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .output()
        .expect("run harness")
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
