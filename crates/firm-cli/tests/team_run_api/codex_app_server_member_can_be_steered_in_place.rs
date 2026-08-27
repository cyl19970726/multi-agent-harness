use super::*;

#[test]
fn codex_app_server_member_can_be_steered_in_place() {
    let home = TempHome::new("team-run-codex-app-server");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_codex_team_shim(&home.base().join("fakebin-codex-app"));
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let serve = ServeHandle::spawn_with_env(&home, home.base(), &[], &[("PATH", path.as_str())]);
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise live Codex control",
            "members": [{
                "name": "codex-live",
                "role": "implementer",
                "provider": "codex",
                "execution_mode": "codex_app_server",
                "initial_work": "Exercise live Codex control"
            }]
        }),
    );
    assert_eq!(status, 200, "body: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = member_run_for_work_owner(&created["result"], 0)["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let (status, started) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "body: {started}");

    let mut live = false;
    for _ in 0..100 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        live = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("running")
                    && member["native_session"]["native_session_id"].as_str()
                        == Some("thread_fake_codex_app_server")
            });
        if live {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, diagnostic_snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        live,
        "app-server member never became live; snapshot: {diagnostic_snapshot}"
    );

    // Control the provider through a second Harness service process. The
    // durable lease routes this request to the Supervisor process that owns
    // the physical app-server connection; no process-local registry shortcut
    // is available to this client.
    let control_client = ServeHandle::spawn(&home, home.base(), &[]);
    let (status, steered) = control_client.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/steer"),
        &serde_json::json!({"content": "finish with the requested report", "requested_by": "operator"}),
    );
    assert_eq!(status, 200, "body: {steered}");
    assert_eq!(
        steered["result"]["control"]["delivery"].as_str(),
        Some("steered")
    );
    assert_eq!(
        steered["result"]["message"]["deliveries"][0]["policy"].as_str(),
        Some("queue"),
        "the audit Message remains conversation delivery; the separate RuntimeCommand owns current-turn injection"
    );

    let mut idle = false;
    // A complete fake app-server turn normally takes ~3s, but a clean CI
    // runner can be CPU-starved while the full workspace suite is draining.
    // Keep the assertion bounded without treating scheduler delay as a
    // provider-lifecycle failure.
    for _ in 0..1_000 {
        let (_, snapshot) = serve.get_json("/v1/snapshot");
        idle = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("idle")
            });
        if idle {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, diagnostic_snapshot) = serve.get_json("/v1/snapshot");
    assert!(
        idle,
        "steered member did not return to persistent idle; snapshot: {diagnostic_snapshot}"
    );

    // DEV-21: the fresh-start settle above wrote the native Session through
    // save_member_run; the trust selector + exact-binding layers must carry it.
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    assert_trust_native_binding_synced(&store, &member_id, "thread_fake_codex_app_server");
}
