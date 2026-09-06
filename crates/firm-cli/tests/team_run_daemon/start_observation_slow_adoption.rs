use super::*;
use std::io::Read as _;

/// Reap every spawned process on any test outcome. The release file is
/// created first so a shim still parked in its probe loop can exit before
/// the kill, leaving no orphaned provider process behind.
struct ProcessGuard {
    release: PathBuf,
    children: Vec<std::process::Child>,
}

impl ProcessGuard {
    fn new(release: &Path) -> Self {
        Self {
            release: release.to_path_buf(),
            children: Vec::new(),
        }
    }

    fn watch(&mut self, child: std::process::Child) {
        self.children.push(child);
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = std::fs::File::create(&self.release);
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Read a finished child's piped output for diagnostics. Only call after the
/// child exited; the pipes block while it runs.
fn finished_child_output(child: &mut std::process::Child) -> (String, String) {
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    (stdout, stderr)
}

/// #773 expectation 2: an explicit `team-run start` competing with a slow
/// boot-time adoption must eventually return proved success instead of
/// TEAM_RUN_START_RESULT_UNKNOWN, with exactly one start request, no
/// duplicate admission, and the status lane reachable throughout.
#[test]
fn start_during_slow_adoption_observes_and_proves_exact_postcondition_once() {
    let home = TempHome::new("start-observation-slow-adoption");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());

    // A test-local wrapper whose `--version` probe blocks on a release file,
    // so boot adoption holds supervisor_start_gate until the test releases
    // it. Everything else delegates to the ordinary fake Kimi ACP shim.
    let release = home.base().join("kimi-probe-release");
    let mut processes = ProcessGuard::new(&release);
    let wrapper_dir = home.base().join("fakebin-slow-kimi");
    std::fs::create_dir_all(&wrapper_dir).expect("wrapper bin dir");
    let wrapper_path = wrapper_dir.join("kimi");
    std::fs::write(
        &wrapper_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  while [ ! -e \"{}\" ]; do sleep 0.05; done\nfi\nexec \"{}\" \"$@\"\n",
            release.display(),
            fake_bin.join("kimi").display()
        ),
    )
    .expect("write wrapper shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod wrapper shim");
    }
    let wrapper_path_string = wrapper_path.display().to_string();
    let daemon_path = format!(
        "{}:{}:{}",
        wrapper_dir.display(),
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let run_id = create_run(&home, &fixture, "worker", &[]);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let daemon_env = [
        ("PATH", daemon_path.as_str()),
        ("KIMI_CODE_BIN", wrapper_path_string.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FAKE_KIMI_WAIT", "1"),
    ];
    let daemon = spawn_daemon(&home, &fixture, &daemon_env);
    processes.watch(daemon);
    let socket_wait_child_index = processes.children.len() - 1;
    wait_for_socket(&mut processes.children[socket_wait_child_index], &socket);

    // The explicit start competes with the blocked boot adoption. The start
    // CLI itself probes the member's provider version in prepare before it
    // may send, so its environment gets the FAST shim (never the gated
    // wrapper): the start frame must reach the daemon while adoption still
    // holds the gate. Spawn it as a child so the test can release the probe
    // only after the start lane's read budget (3 x 5s same-socket reads) has
    // provably expired.
    let fast_kimi = fake_bin.join("kimi").display().to_string();
    let client_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args([
            "--space",
            fixture.execution_space_id.as_str(),
            "--project",
            fixture.project_id.as_str(),
            "team-run",
            "start",
            "--id",
            run_id.as_str(),
        ])
        .current_dir(&fixture.project_root)
        .envs(home.envs())
        .env("PATH", &client_path)
        .env("KIMI_CODE_BIN", &fast_kimi)
        .env("FAKE_KIMI_VERSION", "0.36.1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for key in [
        "FIRM_ROOT",
        "FIRM_PROJECT",
        "FIRM_PROJECT_ID",
        "FIRM_SPACE",
        "FIRM_COMPANY",
        "FIRM_MISSION_ID",
        "FIRM_ORIGIN_WAVE_ID",
        "FIRM_TEAM_RUN_ID",
        "FIRM_MEMBER_RUN_ID",
        "FIRM_WORK_ID",
        "FIRM_WORK_VERSION",
        "HARNESS_ROOT",
        "HARNESS_PROJECT",
        "HARNESS_PROJECT_ID",
        "HARNESS_SPACE",
        "HARNESS_COMPANY",
        "HARNESS_MISSION_ID",
        "HARNESS_ORIGIN_WAVE_ID",
        "HARNESS_TEAM_RUN_ID",
        "HARNESS_MEMBER_RUN_ID",
        "HARNESS_WORK_ID",
        "HARNESS_WORK_VERSION",
        "HARNESS_HOME",
    ] {
        command.env_remove(key);
    }
    let start = command.spawn().expect("spawn team-run start");
    processes.watch(start);
    let start_index = processes.children.len() - 1;

    std::thread::sleep(Duration::from_secs(17));
    if let Some(status) = processes.children[start_index]
        .try_wait()
        .expect("inspect team-run start")
    {
        let (stdout, stderr) = finished_child_output(&mut processes.children[start_index]);
        panic!(
            "team-run start resolved before the adoption probe was released (status {status})\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    let mid_status = socket_request(&socket, &serde_json::json!({"cmd":"status"}).to_string());
    assert_eq!(
        mid_status["ok"], true,
        "status lane starved during adoption: {mid_status}"
    );

    std::fs::File::create(&release).expect("release provider probe");
    let deadline = Instant::now() + Duration::from_secs(45);
    let output = loop {
        if let Some(status) = processes.children[start_index]
            .try_wait()
            .expect("inspect team-run start")
        {
            let (stdout, stderr) = finished_child_output(&mut processes.children[start_index]);
            break std::process::Output {
                status,
                stdout: stdout.into_bytes(),
                stderr: stderr.into_bytes(),
            };
        }
        assert!(
            Instant::now() < deadline,
            "team-run start did not resolve after the adoption probe was released; daemon evidence log: {}",
            home.base().join("daemon-stderr.log").display()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    success(&output, "team-run start during slow adoption");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("delegated to NodeDaemon"),
        "expected proved delegated success, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // Exactly one admission: one managed run at Supervisor generation 1, and
    // the already-managed replay stays prompt and duplicate-free.
    let managed = wait_for_run(&socket, &fixture.execution_space_id, &run_id);
    assert_eq!(managed["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(managed["runs"][0]["supervisor_generation"], 1);
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
    assert_eq!(duplicate["supervisor_generation"], 1);

    let mut daemon = processes.children.remove(socket_wait_child_index);
    stop_daemon(&home, &fixture, &mut daemon, &socket);
}
