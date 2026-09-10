//! Child-process isolation for process-global daemon scenarios (#928).
//!
//! The provider process-group registry (`harness_runtime_host`) and the
//! daemon graceful-shutdown sweep are process-global: any in-process test
//! that stops a daemon consumes and SIGKILLs every process group registered
//! in the same test binary, and `complete_registered_process_group_shutdown`
//! resets global spawn admission. A file-local mutex cannot protect that
//! state because not every actor participates in it (the old
//! `shutdown_test_lock` covered only `shutdown_tests`, never the stop/drain
//! or incidental `serve_loop` exits). Each process-global scenario therefore
//! runs its body in a fresh child copy of the test binary (`current_exe` +
//! exact test selection), so a scenario sweeps only the registry of its own
//! process and an unrelated registration anywhere else is unreachable by
//! construction — no lock, no suite serialization.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Marks the isolated child copy of the test binary; the value is the exact
/// test name that copy is allowed to run. A wrapped test executes its body
/// only inside such a child; everywhere else it spawns the child instead.
pub const ISOLATED_CHILD_ENV: &str = "FIRM_PROCESS_GLOBAL_TEST_CHILD";

/// Hard wall-clock bound for one isolated child. The slowest scenario drains
/// for ~7 s and harness startup is ~1 s; this bound only fires on a hung
/// child, never in a passing run.
pub const ISOLATED_CHILD_TIMEOUT: Duration = Duration::from_secs(120);

/// Bound for a coordinated child to publish its first milestone (harness
/// startup plus one fixture spawn). Fires only when the child died before
/// signaling, which the sibling-output dump then explains.
pub const CHILD_SIGNAL_TIMEOUT: Duration = Duration::from_secs(60);

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `body` inside an isolated child process selected as `exact_test_name`.
///
/// In the ordinary test binary this spawns
/// `<current_exe> <exact_test_name> --exact --nocapture --test-threads=1`
/// with [`ISOLATED_CHILD_ENV`] set, waits with an explicit bound (SIGKILLing
/// and reaping the whole child process group on timeout), and fails unless
/// the child ran exactly one test and passed it — a mistyped selection that
/// matches zero tests must never masquerade as a pass. Inside the child the
/// same wrapper observes the env and executes `body` in place.
pub fn run_in_isolated_child(exact_test_name: &str, body: fn()) {
    match std::env::var(ISOLATED_CHILD_ENV) {
        Ok(selected) if selected == exact_test_name => {
            body();
            return;
        }
        Ok(selected) => panic!(
            "isolated child for {selected} cannot also run {exact_test_name}: \
             nested process-global scenarios are forbidden"
        ),
        Err(_) => {}
    }
    let dir = isolation_dir(exact_test_name);
    let mut child = spawn_test_child(exact_test_name, &dir, "child", &[])
        .expect("spawn isolated scenario child");
    let status = wait_child_bounded(&mut child, ISOLATED_CHILD_TIMEOUT, exact_test_name);
    let output = read_child_output(&dir, "child");
    assert_single_test_passed(status, exact_test_name, &output);
}

/// Spawn one isolated child copy of the test binary running exactly
/// `exact_test_name`, stdout/stderr captured to `<dir>/<label>.{stdout,stderr}`
/// (files, not pipes, so a verbose child can never deadlock a bounded wait).
/// The child leads its own process group so a timeout can SIGKILL the whole
/// subtree — scenario bodies spawn `sleep` fixtures of their own.
pub fn spawn_test_child(
    exact_test_name: &str,
    dir: &Path,
    label: &str,
    extra_env: &[(&str, &str)],
) -> std::io::Result<Child> {
    let stdout = std::fs::File::create(dir.join(format!("{label}.stdout")))?;
    let stderr = std::fs::File::create(dir.join(format!("{label}.stderr")))?;
    let mut command = Command::new(std::env::current_exe().expect("test binary path"));
    command
        .arg(exact_test_name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(ISOLATED_CHILD_ENV, exact_test_name)
        .envs(extra_env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);
    command.spawn()
}

/// Join `child` with a hard wall-clock bound. On timeout the whole child
/// process group is SIGKILLed and the child reaped before panicking, so a
/// hung scenario can never leak a fixture process or block the suite.
pub fn wait_child_bounded(child: &mut Child, timeout: Duration, label: &str) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("poll isolated child") {
            return status;
        }
        if Instant::now() >= deadline {
            kill_child_group(child);
            panic!(
                "isolated child {label} exceeded its {timeout:?} wall-clock bound and was killed"
            );
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// SIGKILL and reap `child`'s whole process group if it is still running.
/// Used on a sibling's failure path so one hung or failed child never leaves
/// the other side of a coordinated regression behind.
pub fn kill_child_group(child: &mut Child) {
    if child.try_wait().expect("poll child before kill").is_some() {
        return;
    }
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Read everything an isolated child wrote to its capture files, for failure
/// diagnostics and for the zero-match guard below.
pub fn read_child_output(dir: &Path, label: &str) -> String {
    let mut output = String::new();
    for stream in ["stdout", "stderr"] {
        let mut contents = String::new();
        if let Ok(mut file) = std::fs::File::open(dir.join(format!("{label}.{stream}"))) {
            let _ = file.read_to_string(&mut contents);
        }
        output.push_str(&format!("--- {label} {stream} ---\n{contents}\n"));
    }
    output
}

/// A child must prove it ran exactly one test and that the test passed. Exit
/// status alone is not evidence: a mistyped `--exact` selection matches zero
/// tests and still exits successfully.
pub fn assert_single_test_passed(status: ExitStatus, exact_test_name: &str, output: &str) {
    assert!(
        status.success(),
        "isolated child for {exact_test_name} exited {status:?}\n{output}"
    );
    assert!(
        output.contains("running 1 test"),
        "isolated child for {exact_test_name} did not run exactly one test \
         (a zero-match selection is not a pass)\n{output}"
    );
    assert!(
        output.contains("test result: ok. 1 passed;"),
        "isolated child for {exact_test_name} did not report exactly one passed test\n{output}"
    );
}

/// Fresh per-process output directory for one isolation run. Stale contents
/// from an earlier run are removed first so coordination files always
/// describe this run, never a previous one.
pub fn isolation_dir(tag: &str) -> PathBuf {
    let sanitized: String = tag
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = std::env::temp_dir().join(format!(
        "firm-928-isolation-{sanitized}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create isolation output dir");
    dir
}

/// Wait for a coordination file with an explicit bound; the file's absence
/// after `timeout` means the publishing child died, which is a test failure,
/// not a reason to wait forever.
pub fn wait_for_file_bounded(path: &Path, timeout: Duration, label: &str) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {label} at {}",
            path.display()
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}
