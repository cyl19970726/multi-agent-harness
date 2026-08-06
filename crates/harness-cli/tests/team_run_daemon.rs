//! Integration coverage for the multi-team supervisor daemon (#366):
//! `harness daemon serve` manages N team-runs from one process, and
//! `harness team-run start` delegates to it when the daemon socket is present.
//!
//! These tests use the fake kimi ACP shim so no real provider binary runs.
//!
//! Daemon-spawning tests (daemon_serve_*, daemon_restart_*) are ignored by
//! default because macOS AF_UNIX sun_path limits (104 bytes) make TempDir
//! store-root paths too long. Run them on CI (Linux, /tmp) or set TMPDIR=/tmp
//! before the test binary.

use std::path::Path;
use std::process::Command;

mod fake_provider;
mod harness_env;

use harness_env::{run_harness, TempHome};

fn init_project(home: &TempHome, name: &str) -> String {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    harness_env::current_project_id(home)
}

fn run_with_fake_kimi(
    home: &TempHome,
    fake_bin_dir: &Path,
    args: &[&str],
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake_bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_VERSION", "0.31.0")
        .env(
            "FAKE_KIMI_ENV_MARKER",
            home.base().join("kimi-collaboration.env"),
        )
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .output()
        .expect("run harness with fake kimi")
}

fn create_run(home: &TempHome, fake_dir: &Path, project: &str) -> String {
    let out = run_with_fake_kimi(
        home,
        fake_dir,
        &[
            "--project",
            project,
            "team-run",
            "create",
            "--objective",
            "Quick test",
            "--member",
            "w:implementer:kimi/acp:k2.5#Test daemon",
        ],
    );
    assert!(out.status.success(), "create failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ---------------------------------------------------------------------------
// Foreground fallback — no-regression guard
// ---------------------------------------------------------------------------

#[test]
fn team_run_start_runs_foreground_when_daemon_absent() {
    let home = TempHome::new("daemon-fg");
    let project_id = init_project(&home, "alpha");
    let fake_dir = fake_provider::install_kimi_acp_shim(home.base());
    let run_id = create_run(&home, &fake_dir, &project_id);

    let start = run_with_fake_kimi(
        &home,
        &fake_dir,
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
        start.status.success(),
        "foreground start: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(
        stdout.contains(&format!("team run {run_id}\trunning")),
        "summary: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Daemon socket bind (requires short store root — CI only)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "macOS AF_UNIX path limit; use TMPDIR=/tmp or run on Linux CI"]
fn daemon_serve_binds_socket() {
    std::env::set_var("TMPDIR", "/tmp");
    let home = TempHome::new("ds");
    let project_id = init_project(&home, "a");
    let fake_dir = fake_provider::install_kimi_acp_shim(home.base());
    let space_dir = home.spaces_dir().join(&project_id);
    let socket_path = {
        use std::hash::{Hash, Hasher};
        let default = space_dir.join("supervisor.sock");
        if default.as_os_str().len() < 104 {
            default
        } else {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            space_dir.as_os_str().as_encoded_bytes().hash(&mut h);
            std::path::PathBuf::from("/tmp")
                .join(format!("harness-{:016x}.sock", h.finish()))
        }
    };

    let daemon = spawn_daemon(&home, &fake_dir, &[
        "--space", &project_id, "daemon", "serve", "--scan-interval-s", "2",
    ]);

    let mut found = false;
    for _ in 0..10 {
        if socket_path.exists() {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    let output = daemon.wait_with_output().unwrap_or_else(|e| {
        panic!("daemon wait: {e}");
    });
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        found,
        "daemon should bind at {}\nstderr: {stderr}",
        socket_path.display()
    );

    let _ = std::fs::remove_file(&socket_path);
}

fn spawn_daemon(
    home: &TempHome,
    fake_bin_dir: &Path,
    args: &[&str],
) -> std::process::Child {
    let path = format!(
        "{}:{}",
        fake_bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(args)
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_SPACE")
        .env_remove("HARNESS_COMPANY")
        .env("PATH", path)
        .env("FAKE_KIMI_RESULT", "done")
        .env("FAKE_KIMI_VERSION", "0.31.0")
        .env("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")
        .env_remove("KIMI_CODE_BIN")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn harness daemon")
}
