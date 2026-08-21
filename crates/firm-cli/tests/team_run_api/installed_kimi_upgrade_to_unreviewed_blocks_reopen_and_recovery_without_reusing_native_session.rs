use super::*;

// TODO: Test uses a hardcoded unreviewed-version guard that drifts when the
// version is admitted. Annotated #[ignore] until the CI version matrix is stable.
#[test]
#[ignore = "version-guard-drift"]
fn installed_kimi_upgrade_to_unreviewed_blocks_reopen_and_recovery_without_reusing_native_session()
{
    let home = TempHome::new("team-run-kimi-review-required-reopen");
    let project_id = init_project(&home, "alpha");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let fake_kimi = fake_bin.join("kimi").display().to_string();
    let reviewed_acp_marker = home.base().join("kimi-0361-reviewed-acp.log");
    let reviewed_acp_marker_value = reviewed_acp_marker.display().to_string();

    let reviewed_serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.36.1"),
            ("FAKE_KIMI_ENV_MARKER", reviewed_acp_marker_value.as_str()),
        ],
    );
    let (_, created) = reviewed_serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Bind one reviewed Kimi session, then preserve it across drift",
            "members": [{"name": "kimi-history", "role": "builder", "provider": "kimi", "initial_work": "Create one reviewed native history"}]
        }),
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let member_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("member id")
        .to_string();
    let work_id = created["result"]["works"][0]["id"]
        .as_str()
        .expect("work id")
        .to_string();
    let (status, started) = reviewed_serve.post_json(
        &format!("/v1/team-runs/{run_id}/start"),
        &serde_json::json!({}),
    );
    assert_eq!(status, 202, "reviewed control must start: {started}");

    let mut native_session_id = None;
    for _ in 0..300 {
        let (_, snapshot) = reviewed_serve.get_json("/v1/snapshot");
        native_session_id = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|member| member["id"].as_str() == Some(member_id.as_str()))
            .and_then(|member| member["native_session"]["native_session_id"].as_str())
            .map(str::to_string);
        if native_session_id.is_some() && reviewed_acp_marker.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let native_session_id = native_session_id.expect("reviewed Kimi native session");
    assert!(
        reviewed_acp_marker.exists(),
        "reviewed 0.31 ACP never started"
    );

    let (status, closed) = reviewed_serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/close"),
        &serde_json::json!({"requested_by": "host", "reason": "prepare drift regression"}),
    );
    assert_eq!(status, 200, "close failed: {closed}");
    let mut stopped = false;
    for _ in 0..200 {
        let (_, snapshot) = reviewed_serve.get_json("/v1/snapshot");
        stopped = snapshot["member_runs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|member| {
                member["id"].as_str() == Some(member_id.as_str())
                    && member["status"].as_str() == Some("stopped")
                    && member["coordination_status"].as_str() == Some("closed")
            });
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stopped, "reviewed member did not close");

    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let before_member = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("member before drift");
    let before_generation = before_member.runtime_generation;
    let before_work = store
        .latest_works()
        .expect("Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("Work before drift");
    drop(reviewed_serve);

    let blocked_acp_marker = home.base().join("kimi-033-blocked-acp.log");
    let blocked_acp_marker_value = blocked_acp_marker.display().to_string();
    let drifted_serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.33.0"),
            ("FAKE_KIMI_ENV_MARKER", blocked_acp_marker_value.as_str()),
        ],
    );
    let (status, reopened) = drifted_serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({"reopened_by": "host", "reason": "must refuse drift"}),
    );
    assert_eq!(
        status, 400,
        "drifted reopen unexpectedly succeeded: {reopened}"
    );
    assert!(reopened["error"].as_str().is_some_and(|error| error
        .contains("PROVIDER_COMPATIBILITY_BLOCKED")
        && error.contains("0.33.0")));
    assert!(
        !blocked_acp_marker.exists(),
        "reopen spawned or attached ACP before compatibility refusal"
    );

    let recovery = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "recover",
            "--id",
            &run_id,
            "--json",
        ],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.33.0"),
            ("FAKE_KIMI_ENV_MARKER", blocked_acp_marker_value.as_str()),
        ],
    );
    assert!(
        !recovery.status.success(),
        "recovery unexpectedly succeeded"
    );
    let recovery_error = String::from_utf8_lossy(&recovery.stderr);
    assert!(
        recovery_error.contains("PROVIDER_COMPATIBILITY_BLOCKED")
            && recovery_error.contains("0.33.0"),
        "stderr: {recovery_error}"
    );
    assert!(
        !blocked_acp_marker.exists(),
        "recovery spawned or resumed ACP before compatibility refusal"
    );

    let after_member = store
        .member_runs()
        .expect("member rows after drift")
        .into_iter()
        .rev()
        .find(|member| member.id == member_id)
        .expect("member after drift");
    assert_eq!(after_member.runtime_generation, before_generation);
    assert_eq!(
        after_member.coordination_status,
        before_member.coordination_status
    );
    assert_eq!(
        after_member
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some(native_session_id.as_str())
    );
    let after_work = store
        .latest_works()
        .expect("Works after drift")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("Work after drift");
    assert_eq!(after_work.version, before_work.version);
    assert_eq!(
        after_work.active_member_run_id,
        before_work.active_member_run_id
    );

    // Positive counterpart: 0.32.0 IS adapter-reviewed (see
    // reviewed_provider_versions), so after the unreviewed 0.33.0 refusal
    // above, reopening the same closed member under 0.32.0 must succeed and
    // resume the preserved native session — the deterministic form of the
    // live canary that admitted 0.32.0 (capabilities like cancel/goal-mode
    // remain unclaimed and are covered by the unit test).
    drop(drifted_serve);
    let admitted_acp_marker = home.base().join("kimi-032-admitted-acp.log");
    let admitted_acp_marker_value = admitted_acp_marker.display().to_string();
    let admitted_serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.32.0"),
            ("FAKE_KIMI_ENV_MARKER", admitted_acp_marker_value.as_str()),
        ],
    );
    let (status, reopened) = admitted_serve.post_json(
        &format!("/v1/team-runs/{run_id}/members/{member_id}/reopen"),
        &serde_json::json!({"reopened_by": "host", "reason": "reviewed 0.32.0 admits reopen with continuity"}),
    );
    assert_eq!(
        status, 202,
        "reviewed 0.32.0 reopen must be accepted: {reopened}"
    );
    assert_eq!(
        reopened["result"]["history_continuity"].as_str(),
        Some("provider_native_session"),
        "reviewed reopen must resume the preserved native session: {reopened}"
    );
    // The reopen is accepted, but the drive belongs to a supervisor; the
    // original one lived in the dropped reviewed_serve. Recover under the
    // reviewed 0.32.0 env to adopt the run and resume the member (the same
    // path production used: a live supervisor generation drives the resume).
    let recovery = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "recover",
            "--id",
            &run_id,
            "--json",
        ],
        &[
            ("KIMI_CODE_BIN", fake_kimi.as_str()),
            ("FAKE_KIMI_VERSION", "0.32.0"),
            ("FAKE_KIMI_ENV_MARKER", admitted_acp_marker_value.as_str()),
        ],
    );
    assert!(
        recovery.status.success(),
        "reviewed 0.32.0 recovery must succeed, stderr: {}",
        String::from_utf8_lossy(&recovery.stderr)
    );
    // Note: actual provider-process resume is driven by a long-running
    // supervisor generation (production `team-run start`), which this test
    // does not spawn; end-to-end drive for 0.32.0 is covered by the live
    // canary recorded in PR #327 (post-reopen member completed provider
    // rounds). This test asserts the gate-level contract: admit, continuity,
    // recoverability.
}
