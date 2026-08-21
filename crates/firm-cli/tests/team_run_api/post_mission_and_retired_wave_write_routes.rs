use super::*;

#[test]
fn post_mission_and_retired_wave_write_routes() {
    let home = TempHome::new("mission-wave-http");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // `POST /v1/missions` is retired (DOC-108): Mission is historical
    // provenance, never new current authority.
    let (status, body) = serve.post_json(
        "/v1/missions",
        &serde_json::json!({
            "id": "mission-http",
            "title": "HTTP Mission",
            "objective": "Author via API"
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("retired") && error.contains("DOC-108"),
        "error: {error}"
    );

    // `POST /v1/waves` stays retired (ADR 0051).
    let (status, body) = serve.post_json(
        "/v1/waves",
        &serde_json::json!({
            "id": "wave-http",
            "mission_id": "mission-http",
            "title": "HTTP Wave",
            "objective": "Gate without accepting",
            "executor_kind": "host"
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("retired") && error.contains("legacy wave"),
        "error: {error}"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(
        snapshot["missions"].as_array().map(Vec::len),
        Some(1),
        "the rejected POST must not have appended a row; only the seeded fixture Mission remains"
    );
    assert_eq!(
        snapshot["legacy_waves"].as_array().map(Vec::len),
        Some(0),
        "the rejected POST must not have appended a row"
    );

    // `POST /v1/missions/{id}/log` is retired too (DOC-108); the historical
    // Log is seeded directly and stays readable in the snapshot projection.
    let (status, body) = serve.post_json(
        "/v1/missions/mission-runtime-fixture/log",
        &serde_json::json!({"kind": "judgment", "body": "must not persist"}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("retired"),
        "body: {body}"
    );
    seed_historical_mission_log(
        &home,
        &project_id,
        FIXTURE_MISSION_ID,
        1,
        "judgment",
        "pre-cutover history",
        "host",
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(snapshot["mission_log"].as_array().map(Vec::len), Some(1));

    // `POST /v1/waves/{id}/gate` is retired too, regardless of whether the
    // named Wave id exists.
    let (status, body) = serve.post_json(
        "/v1/waves/wave-http/gate",
        &serde_json::json!({"status": "revise", "note": "clarify scope"}),
    );
    assert_eq!(status, 400, "body: {body}");
    let error = body["error"].as_str().unwrap_or_default();
    assert!(error.contains("retired"), "error: {error}");
}
