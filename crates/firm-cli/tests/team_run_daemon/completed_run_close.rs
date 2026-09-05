use super::*;
use harness_core::{MemberCoordinationStatus, MemberRunStatus, TeamRunStatus};

fn selected<'a>(fixture: &'a RuntimeFixture, command: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "--space",
        fixture.execution_space_id.as_str(),
        "--project",
        fixture.project_id.as_str(),
    ];
    args.extend_from_slice(command);
    args
}

fn wait_for_idle_managed_member(
    store: &HarnessStore,
    run_id: &str,
    previous_last_event_at: Option<&Option<String>>,
) -> harness_core::ProviderRuntimeProjection {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let member = store
            .member_runs()
            .expect("read managed MemberRun")
            .into_iter()
            .filter(|member| member.team_run_id == run_id && !member.is_external_interactive())
            .max_by_key(|member| member.runtime_generation)
            .expect("managed MemberRun");
        if member.status == MemberRunStatus::Idle
            && member.coordination_status == MemberCoordinationStatus::Active
            && member.native_session.is_some()
            && member.last_consumed_work_version.is_some()
            && previous_last_event_at.is_none_or(|previous| &member.last_event_at != previous)
        {
            return member;
        }
        assert!(
            Instant::now() < deadline,
            "managed member did not become idle: {member:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_completed_run_status(socket: &Path, run_id: &str, unclosed_members: usize) {
    let expected = format!("completed ({unclosed_members} unclosed member(s))");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = socket_request(socket, r#"{"cmd":"status"}"#);
        let completed_serving = status["runs"].as_array().is_some_and(|runs| {
            runs.iter()
                .any(|run| run["run_id"] == run_id && run["status"] == expected)
        });
        if completed_serving {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "completed run did not remain served with its unclosed count: {status}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// One completed TeamRun with one unclosed idle managed member, served by a
/// live NodeDaemon: the shared precondition for the pre-restart Close test
/// and the restart-then-Close test (#812).
struct CompletedRunFixture {
    home: TempHome,
    fixture: RuntimeFixture,
    provider_env: Vec<(String, String)>,
    run_id: String,
    socket: PathBuf,
    daemon: std::process::Child,
    store: HarnessStore,
    member: harness_core::ProviderRuntimeProjection,
}

fn completed_run_fixture() -> CompletedRunFixture {
    let home = TempHome::new("completed-run-close");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let kimi_bin = fake_bin.join("kimi").display().to_string();
    let provider_env = vec![
        ("PATH".to_string(), fake_path),
        ("KIMI_CODE_BIN".to_string(), kimi_bin),
        ("FAKE_KIMI_VERSION".to_string(), "0.36.1".to_string()),
    ];
    let env_refs: Vec<(&str, &str)> = provider_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let run_id = create_run(&home, &fixture, "close-after-complete", &env_refs);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &env_refs);
    wait_for_socket(&mut daemon, &socket);

    let start = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(&fixture, &["team-run", "start", "--id", &run_id]),
        &env_refs,
    );
    success(&start, "team-run start");
    wait_for_run(&socket, &fixture.execution_space_id, &run_id);

    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let member = wait_for_idle_managed_member(&store, &run_id, None);
    let work = store
        .latest_works()
        .expect("read Work")
        .into_iter()
        .find(|work| work.team_run_id == run_id)
        .expect("initial Work");
    let work_version = work.version.to_string();
    let cancelled = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(
            &fixture,
            &[
                "team-run",
                "work",
                "cancel",
                "--work-id",
                &work.id,
                "--expected-version",
                &work_version,
                "--reason",
                "fixture reached its terminal responsibility state",
            ],
        ),
        &env_refs,
    );
    success(&cancelled, "cancel initial Work");

    let completed = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(&fixture, &["team-run", "complete", "--id", &run_id]),
        &env_refs,
    );
    success(&completed, "team-run complete");
    let completion_output = String::from_utf8_lossy(&completed.stdout);
    assert!(
        completion_output.contains(&member.name),
        "{completion_output}"
    );
    assert!(
        completion_output.contains(&format!(
            "firm team-run close-member --id {run_id} --member-run-id {}",
            member.id
        )),
        "{completion_output}"
    );
    assert_eq!(
        store
            .team_runs()
            .expect("read TeamRun")
            .into_iter()
            .rfind(|run| run.id == run_id)
            .expect("completed TeamRun")
            .status,
        TeamRunStatus::Completed
    );

    wait_for_completed_run_status(&socket, &run_id, 1);
    let board = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(&fixture, &["team-run", "board-summary", "--id", &run_id]),
        &env_refs,
    );
    success(&board, "completed board-summary");
    assert!(
        String::from_utf8_lossy(&board.stdout).contains("run=completed (1 unclosed member(s))"),
        "{}",
        String::from_utf8_lossy(&board.stdout)
    );

    CompletedRunFixture {
        home,
        fixture,
        provider_env,
        run_id,
        socket,
        daemon,
        store,
        member,
    }
}

/// Close the fixture's one unclosed member, then prove the last Close
/// releases the Supervisor lease and removes the run from `daemon status`,
/// and that deactivate still works. Returns after the final daemon stop.
fn close_deactivate_and_stop(
    CompletedRunFixture {
        home,
        fixture,
        provider_env,
        run_id,
        socket,
        mut daemon,
        store,
        member,
    }: CompletedRunFixture,
) {
    let env_refs: Vec<(&str, &str)> = provider_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let closed = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(
            &fixture,
            &[
                "team-run",
                "close-member",
                "--id",
                &run_id,
                "--member-run-id",
                &member.id,
                "--reason",
                "completed TeamRun cleanup",
            ],
        ),
        &env_refs,
    );
    success(&closed, "close member after completion");

    let release_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let released = store
            .latest_team_supervisor_lease(&run_id)
            .expect("read Supervisor lease")
            .is_some_and(|lease| lease.status == TeamSupervisorLeaseStatus::Released);
        let status = socket_request(&socket, r#"{"cmd":"status"}"#);
        let absent = status["runs"]
            .as_array()
            .is_some_and(|runs| runs.iter().all(|run| run["run_id"] != run_id));
        if released && absent {
            break;
        }
        assert!(
            Instant::now() < release_deadline,
            "last Close did not release/remove completed Supervisor: {status}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    let deactivated = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(
            &fixture,
            &[
                "team-run",
                "deactivate-member",
                "--id",
                &run_id,
                "--member-run-id",
                &member.id,
                "--reason",
                "completed TeamRun cleanup finished",
            ],
        ),
        &env_refs,
    );
    success(&deactivated, "deactivate member after Close");
    let deactivated: serde_json::Value =
        serde_json::from_slice(&deactivated.stdout).expect("deactivate JSON");
    assert_eq!(deactivated["coordination_status"], "retired");

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}

#[test]
fn completed_run_stays_served_until_close_then_allows_deactivate() {
    close_deactivate_and_stop(completed_run_fixture());
}

#[test]
fn completed_run_is_readopted_after_restart_until_close_then_allows_deactivate() {
    let CompletedRunFixture {
        home,
        fixture,
        provider_env,
        run_id,
        socket,
        mut daemon,
        store,
        member,
    } = completed_run_fixture();
    let env_refs: Vec<(&str, &str)> = provider_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    stop_daemon(&home, &fixture, &mut daemon, &socket);
    let mut daemon = spawn_daemon(&home, &fixture, &env_refs);
    wait_for_socket(&mut daemon, &socket);
    // The readopted Supervisor serves the completed run without spawning a
    // member lane: the drained member's runtime is provably over, so Close
    // goes through the coordination path.
    wait_for_completed_run_status(&socket, &run_id, 1);

    close_deactivate_and_stop(CompletedRunFixture {
        home,
        fixture,
        provider_env,
        run_id,
        socket,
        daemon,
        store,
        member,
    });
}
