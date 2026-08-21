use super::*;

/// The Mission HTTP write routes are retired with the legacy CompanyOS
/// cutover (DOC-108): `POST /v1/missions`, `/{id}/close`, `/{id}/context`,
/// `/{id}/log`, and `/{id}/teams` all fail with the explicit retired-write
/// error and leave a byte-zero store delta.
#[test]
fn http_mission_write_routes_are_retired() {
    let home = TempHome::new("mission-wave-http-log");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    seed_historical_mission(&home, &project_id, "mission-log-http", "Mission Log HTTP");

    let ledger_dir = home.spaces_dir().join(&project_id);
    let before = if ledger_dir.exists() {
        std::fs::read_dir(&ledger_dir)
            .expect("read ledger dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                (
                    entry.file_name(),
                    std::fs::read(entry.path()).expect("read ledger file"),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    } else {
        std::collections::BTreeMap::new()
    };

    for (path, payload) in [
        (
            "/v1/missions",
            serde_json::json!({"id": "mission-new", "title": "x", "objective": "y"}),
        ),
        (
            "/v1/missions/mission-log-http/close",
            serde_json::json!({"outcome": "x"}),
        ),
        (
            "/v1/missions/mission-log-http/context",
            serde_json::json!({"context": "x"}),
        ),
        (
            "/v1/missions/mission-log-http/log",
            serde_json::json!({"kind": "judgment", "body": "Advance from the console."}),
        ),
        (
            "/v1/missions/mission-log-http/teams",
            serde_json::json!({"name": "x", "description": "y", "host_agent_id": "z"}),
        ),
        (
            "/v1/missions/mission-log-does-not-exist/log",
            serde_json::json!({"kind": "judgment", "body": "orphan"}),
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

    let after = std::fs::read_dir(&ledger_dir)
        .expect("read ledger dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .map(|entry| {
            (
                entry.file_name(),
                std::fs::read(entry.path()).expect("read ledger file"),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        before, after,
        "retired Mission HTTP writers must leave a byte-zero store delta"
    );
}
