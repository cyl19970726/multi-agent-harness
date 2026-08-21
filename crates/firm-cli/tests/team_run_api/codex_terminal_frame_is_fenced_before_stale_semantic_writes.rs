use super::*;

#[test]
fn codex_terminal_frame_is_fenced_before_stale_semantic_writes() {
    let home = TempHome::new("team-run-codex-terminal-fence");
    let project_id = init_project_selector_clean(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ready = home.base().join("codex-terminal-received");
    let release = home.base().join("codex-terminal-release");
    let ready_value = ready.display().to_string();
    let release_value = release.display().to_string();
    let mut serve_env = vec![
        ("PATH", path.as_str()),
        ("FAKE_CODEX_AUTO_COMPLETE", "1"),
        (
            "FIRM_TEST_CODEX_TERMINAL_RECEIVED_READY",
            ready_value.as_str(),
        ),
        (
            "FIRM_TEST_CODEX_TERMINAL_RECEIVED_RELEASE",
            release_value.as_str(),
        ),
        ("FIRM_TEAM_SUPERVISOR_LEASE_MS", "10000"),
        ("FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS", "10000"),
    ];
    serve_env.extend(NATIVE_SELECTOR_CLEAN_ENV.iter().copied());
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &serve_env);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Fence a Codex terminal frame",
            "members": [{
                "name": "codex-terminal-fence",
                "role": "runtime_reliability",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise terminal fencing"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {body}");
    wait_for_file(&ready, "Codex terminal receive barrier");

    let store = HarnessStore::new(home.spaces_dir().join(project_id));
    let before = member_semantic_row_counts(&store, &member_id);
    assert_eq!(before.2, 0, "terminal frame was processed before barrier");
    replace_supervisor_lease(&store, &run_id);
    std::fs::write(&release, b"release stale terminal").expect("release Codex terminal");
    std::thread::sleep(Duration::from_millis(300));

    assert_eq!(
        member_semantic_row_counts(&store, &member_id),
        before,
        "stale Codex terminal result wrote native-session/member/action/Handoff state"
    );
}
