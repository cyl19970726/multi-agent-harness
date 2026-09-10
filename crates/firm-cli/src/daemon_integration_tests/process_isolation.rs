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
//!
//! Killing a child's own process group never reaches the independent groups
//! its fixtures lead (`process_group(0)`), so fixtures publish their group
//! ids into the child's isolation directory and every kill/failure path
//! releases exactly those groups by explicit signal with a bounded absence
//! proof — never by the fixture's own timer.

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

/// Set by `spawn_test_child` in every isolated child's environment so the
/// child's fixture spawns (`spawn_owned_fixture`) publish their process-group
/// ids where the parent's explicit cleanup paths can find them.
pub const ISOLATION_DIR_ENV: &str = "FIRM_PROCESS_ISOLATION_DIR";

/// Bound for proving a SIGKILLed child was actually reaped. An unproven reap
/// is reported as unresolved cleanup, never hidden by an unbounded wait.
pub const REAP_TIMEOUT: Duration = Duration::from_secs(5);

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `body` inside an isolated child process selected as `exact_test_name`.
///
/// In the ordinary test binary this spawns
/// `<current_exe> <exact_test_name> --exact --nocapture --test-threads=1`
/// with [`ISOLATED_CHILD_ENV`] set, waits with an explicit bound (SIGKILLing
/// and reaping the child's own process group on timeout), and fails unless
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
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let status = wait_child_bounded(&mut child, ISOLATED_CHILD_TIMEOUT, exact_test_name);
        let output = read_child_output(&dir, "child");
        assert_single_test_passed(status, exact_test_name, &output);
    }));
    if let Err(error) = outcome {
        // Every failure path — a timed-out scenario child included — releases
        // the exact fixture groups the child published, then kills and reaps
        // the child itself (a no-op when it already exited).
        kill_published_fixtures(&dir, REAP_TIMEOUT, exact_test_name);
        kill_child_group(&mut child);
        std::panic::resume_unwind(error);
    }
}

/// Spawn one isolated child copy of the test binary running exactly
/// `exact_test_name`, stdout/stderr captured to `<dir>/<label>.{stdout,stderr}`
/// (files, not pipes, so a verbose child can never deadlock a bounded wait).
/// The child leads its own process group so a timeout can SIGKILL that exact
/// group. Killing it never reaches independent process groups: a scenario
/// body that spawns a fixture with `process_group(0)` gives that fixture its
/// own group, which only an explicit `kill_independent_process_group` on the
/// published pid removes.
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
        .env(ISOLATION_DIR_ENV, dir)
        .envs(extra_env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);
    command.spawn()
}

/// Join `child` with a hard wall-clock bound. On timeout the child's own
/// process group is SIGKILLed and the child reaped before panicking, so a
/// hung scenario can never block the suite. Independent process groups the
/// child created (fixtures spawned with `process_group(0)`) survive this
/// kill by design; their lifetime is released explicitly, never by this
/// function — see `kill_independent_process_group`.
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

/// SIGKILL and reap `child`'s own process group if it is still running.
/// Used on a sibling's failure path. This never removes independent process
/// groups the child spawned with `process_group(0)` — those are released via
/// `kill_published_fixtures`. The kill result is checked (an already-gone
/// group is fine; any other errno is a failure, never ignored) and the reap
/// is bounded: an unproven reap panics with the unresolved cleanup named
/// instead of blocking the suite on an unbounded wait.
pub fn kill_child_group(child: &mut Child) {
    if child.try_wait().expect("poll child before kill").is_some() {
        return;
    }
    let pid = child.id();
    let kill_result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    if kill_result == -1 {
        let errno = std::io::Error::last_os_error().raw_os_error();
        assert!(
            errno == Some(libc::ESRCH),
            "SIGKILL of child process group {pid} failed with errno {errno:?}; cleanup is unproven"
        );
    }
    let deadline = Instant::now() + REAP_TIMEOUT;
    loop {
        if child.try_wait().expect("poll child after kill").is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "child {pid} was SIGKILLed but its reap is unproven after {REAP_TIMEOUT:?}; unresolved cleanup"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Probe whether process group `pid` still exists: signal 0 returns 0 while
/// any group member lives and fails with ESRCH once the group is gone.
pub fn process_group_alive(pid: u32) -> bool {
    let probe = unsafe { libc::kill(-(pid as libc::pid_t), 0) };
    probe == 0
}

/// Explicitly release an independent process group by its published leader
/// pid: SIGKILL the group, then prove absence (ESRCH) within `timeout`.
/// Independent groups are never removed by killing a child's own process
/// group; this explicit, bounded signal is the only honest release for them.
pub fn kill_independent_process_group(pid: u32, timeout: Duration, label: &str) {
    if !process_group_alive(pid) {
        return;
    }
    let kill_result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    if kill_result == -1 {
        let errno = std::io::Error::last_os_error().raw_os_error();
        assert!(
            errno == Some(libc::ESRCH),
            "SIGKILL of independent process group {pid} ({label}) failed with errno {errno:?}"
        );
    }
    let deadline = Instant::now() + timeout;
    while process_group_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "independent process group {pid} ({label}) did not exit within {timeout:?}"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Spawn a scenario fixture process: it leads its own (independent) process
/// group and is registered in this process's provider registry. Inside an
/// isolated child (`ISOLATION_DIR_ENV` is set) the group id is published to
/// `<dir>/owned-fixture-<pid>` so a parent that must kill this child can
/// still release exactly this fixture by explicit signal — killing the
/// child's own process group never reaches it.
pub fn spawn_owned_fixture(
    command: &mut Command,
) -> (Child, harness_runtime_host::OwnedProcessGroupRegistration) {
    let mut child = command.process_group(0).spawn().expect("spawn fixture");
    let registration = harness_runtime_host::OwnedProcessGroupRegistration::new(&mut child)
        .expect("register fixture process group");
    if let Ok(dir) = std::env::var(ISOLATION_DIR_ENV) {
        std::fs::write(
            Path::new(&dir).join(format!("owned-fixture-{}", child.id())),
            b"",
        )
        .expect("publish owned fixture process group");
    }
    (child, registration)
}

/// Remove a fixture's publication after its proven reap so parent cleanup
/// never double-handles it. Best-effort: a missing file just means someone
/// already cleaned up.
pub fn unpublish_owned_fixture(pid: u32) {
    if let Ok(dir) = std::env::var(ISOLATION_DIR_ENV) {
        let _ = std::fs::remove_file(Path::new(&dir).join(format!("owned-fixture-{pid}")));
    }
}

/// Explicitly release every fixture group published into `dir`: SIGKILL each
/// exact group and prove absence within `timeout` per group. This is what
/// actually cleans a killed or timed-out child's owned fixtures on the real
/// timeout/failure paths — not only in dedicated cleanup tests.
pub fn kill_published_fixtures(dir: &Path, timeout: Duration, label: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let pid = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_prefix("owned-fixture-"))
            .and_then(|pid| pid.parse::<u32>().ok());
        if let Some(pid) = pid {
            kill_independent_process_group(pid, timeout, label);
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
