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

#[test]
fn completed_run_is_readopted_after_restart_until_close_then_allows_deactivate() {
    let home = TempHome::new("completed-run-close");
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
    ];
    let run_id = create_run(&home, &fixture, "close-after-complete", &provider_env);
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &provider_env);
    wait_for_socket(&mut daemon, &socket);

    let start = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(&fixture, &["team-run", "start", "--id", &run_id]),
        &provider_env,
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
        &provider_env,
    );
    success(&cancelled, "cancel initial Work");

    let completed = run_firm_with_env(
        &home,
        &fixture.project_root,
        &selected(&fixture, &["team-run", "complete", "--id", &run_id]),
        &provider_env,
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
        &provider_env,
    );
    success(&board, "completed board-summary");
    assert!(
        String::from_utf8_lossy(&board.stdout).contains("run=completed (1 unclosed member(s))"),
        "{}",
        String::from_utf8_lossy(&board.stdout)
    );

    stop_daemon(&home, &fixture, &mut daemon, &socket);
    let stopped_member = store
        .member_runs()
        .expect("read stopped-daemon MemberRun")
        .into_iter()
        .filter(|candidate| candidate.id == member.id)
        .max_by_key(|candidate| candidate.last_event_at.clone())
        .expect("MemberRun after daemon stop");
    let mut daemon = spawn_daemon(&home, &fixture, &provider_env);
    wait_for_socket(&mut daemon, &socket);
    wait_for_completed_run_status(&socket, &run_id, 1);
    let readopted_member =
        wait_for_idle_managed_member(&store, &run_id, Some(&stopped_member.last_event_at));
    assert_eq!(readopted_member.id, member.id);

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
        &provider_env,
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
        &provider_env,
    );
    success(&deactivated, "deactivate member after Close");
    let deactivated: serde_json::Value =
        serde_json::from_slice(&deactivated.stdout).expect("deactivate JSON");
    assert_eq!(deactivated["coordination_status"], "retired");

    stop_daemon(&home, &fixture, &mut daemon, &socket);
}
