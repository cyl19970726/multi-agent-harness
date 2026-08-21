use super::*;

/// DOC-108 Stage B retirement contract: every Mission and Wave writer fails
/// with an explicit retired error on the CLI and HTTP surfaces, while the
/// historical rows stay readable through the legacy reads. History is seeded
/// directly into the ledgers — the only way Mission/Wave rows may exist
/// post-cutover.
#[test]
fn legacy_mission_and_wave_writes_are_retired_everywhere() {
    let home = TempHome::new("host-wave-gate");
    let project_id = init_project(&home, "host-wave");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    seed_historical_mission(&home, &project_id, "mission-host", "Direct host work");
    seed_historical_mission_log(
        &home,
        &project_id,
        "mission-host",
        1,
        "closeout_evidence",
        "Direct work verified without a fake executor run.",
        "host",
    );

    // Mission writers are retired on the CLI (DOC-108), whether or not the
    // referenced Mission exists.
    for args in [
        vec!["mission", "create", "--title", "x", "--objective", "y"],
        vec![
            "mission",
            "update-context",
            "--id",
            "mission-host",
            "--context",
            "x",
        ],
        vec!["mission", "close", "--id", "mission-host", "--outcome", "x"],
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host",
            "--kind",
            "judgment",
            "--body",
            "x",
        ],
    ] {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend(args.clone());
        let out = run_firm(&home, home.base(), &full);
        assert!(
            !out.status.success(),
            "harness {args:?} must fail as retired"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("retired") && stderr.contains("DOC-108"),
            "harness {args:?} stderr: {stderr}"
        );
    }

    // ...and over HTTP, the same five routes plus Mission-owned Team creation.
    for (path, payload) in [
        (
            "/v1/missions",
            serde_json::json!({"id": "mission-new", "title": "x", "objective": "y"}),
        ),
        (
            "/v1/missions/mission-host/close",
            serde_json::json!({"outcome": "x"}),
        ),
        (
            "/v1/missions/mission-host/context",
            serde_json::json!({"context": "x"}),
        ),
        (
            "/v1/missions/mission-host/log",
            serde_json::json!({"kind": "judgment", "body": "x"}),
        ),
        (
            "/v1/missions/mission-host/teams",
            serde_json::json!({"name": "x", "description": "y", "host_agent_id": "z"}),
        ),
    ] {
        let (status, body) = serve.post_json(path, &payload);
        assert_eq!(status, 400, "{path} body: {body}");
        let error = body["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("retired") && error.contains("DOC-108"),
            "{path} error: {error}"
        );
    }

    // Wave write commands stay retired on every surface (ADR 0051), regardless
    // of Mission state or whether the referenced Wave exists at all.
    for (command, extra) in [
        (
            "create",
            vec![
                "--mission-id",
                "mission-host",
                "--title",
                "Too late",
                "--objective",
                "Must be rejected",
            ],
        ),
        (
            "update",
            vec!["--id", "wave-does-not-exist", "--context", "x"],
        ),
        (
            "advance",
            vec!["--id", "wave-does-not-exist", "--outcome", "x"],
        ),
        (
            "gate",
            vec!["--id", "wave-does-not-exist", "--status", "accepted"],
        ),
    ] {
        let mut args = vec!["--project", project_id.as_str(), "wave", command];
        args.extend(extra);
        let out = run_firm(&home, home.base(), &args);
        assert!(!out.status.success(), "wave {command} must fail: {args:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("retired") && stderr.contains("legacy wave"),
            "wave {command} stderr: {stderr}"
        );
    }

    // ...and over HTTP, the same four routes.
    for (path, payload) in [
        (
            "/v1/waves",
            serde_json::json!({"mission_id": "mission-host", "title": "x", "objective": "y", "executor_kind": "host"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/context",
            serde_json::json!({"context": "x"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/advance",
            serde_json::json!({"outcome": "x"}),
        ),
        (
            "/v1/waves/wave-does-not-exist/gate",
            serde_json::json!({"status": "accepted"}),
        ),
    ] {
        let (status, body) = serve.post_json(path, &payload);
        assert_eq!(status, 400, "{path} body: {body}");
        let error = body["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("retired") && error.contains("legacy wave"),
            "{path} error: {error}"
        );
    }

    // Historical reads remain functional: the seeded pre-cutover Mission and
    // its Log stay readable through the read-only legacy CLI surface.
    let missions = run_json(&home, &project_id, &["mission", "list"]);
    assert_eq!(missions.as_array().map(Vec::len), Some(1));
    let shown = run_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-host"],
    );
    assert_eq!(shown["title"].as_str(), Some("Direct host work"));
    let log = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-host",
            "--json",
        ],
    );
    assert_eq!(log.as_array().map(Vec::len), Some(1));
    assert_eq!(log[0]["kind"].as_str(), Some("closeout_evidence"));

    // ...and so do Legacy Wave reads, seeded directly (the only way a Wave
    // can exist post-cutover).
    seed_historical_wave(
        &home,
        &project_id,
        "wave-host-historical",
        "mission-host",
        1,
        "host",
    );
    let waves = run_json(
        &home,
        &project_id,
        &["legacy", "wave", "list", "--mission-id", "mission-host"],
    );
    assert_eq!(waves.as_array().map(Vec::len), Some(1));
    let shown = run_json(
        &home,
        &project_id,
        &["legacy", "wave", "show", "--id", "wave-host-historical"],
    );
    assert_eq!(shown["id"].as_str(), Some("wave-host-historical"));
    let history = run_json(
        &home,
        &project_id,
        &["legacy", "wave", "history", "--id", "wave-host-historical"],
    );
    assert_eq!(history.as_array().map(Vec::len), Some(1));
}
