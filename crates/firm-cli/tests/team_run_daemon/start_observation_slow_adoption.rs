use super::*;
use std::io::Read as _;

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
    let fake_path = format!(
        "{}:{}:{}",
        wrapper_dir.display(),
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let run_id = create_run(&home, &fixture, "worker", &[]);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let kimi_bin = wrapper_path.display().to_string();
    let provider_env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_bin.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("FAKE_KIMI_WAIT", "1"),
    ];
    let mut daemon = spawn_daemon(&home, &fixture, &provider_env);
    wait_for_socket(&mut daemon, &socket);

    // The explicit start competes with the blocked boot adoption. Spawn it as
    // a child so the test can release the probe only after the start lane's
    // read budget (3 x 5s same-socket reads) has provably expired.
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
    let mut start = command.spawn().expect("spawn team-run start");

    std::thread::sleep(Duration::from_secs(17));
    assert!(
        start.try_wait().expect("inspect team-run start").is_none(),
        "team-run start resolved before the adoption probe was released"
    );
    let mid_status = socket_request(&socket, &serde_json::json!({"cmd":"status"}).to_string());
    assert_eq!(
        mid_status["ok"], true,
        "status lane starved during adoption: {mid_status}"
    );

    std::fs::File::create(&release).expect("release provider probe");
    let deadline = Instant::now() + Duration::from_secs(45);
    let output = loop {
        if let Some(status) = start.try_wait().expect("inspect team-run start") {
            let mut stdout = String::new();
            start
                .stdout
                .take()
                .expect("start stdout")
                .read_to_string(&mut stdout)
                .expect("read start stdout");
            let mut stderr = String::new();
            start
                .stderr
                .take()
                .expect("start stderr")
                .read_to_string(&mut stderr)
                .expect("read start stderr");
            break std::process::Output {
                status,
                stdout: stdout.into_bytes(),
                stderr: stderr.into_bytes(),
            };
        }
        assert!(
            Instant::now() < deadline,
            "team-run start did not resolve after the adoption probe was released"
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

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}
