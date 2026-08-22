use super::*;

#[test]
fn post_mutation_response_is_bounded_and_dashboard_can_refresh_from_get_snapshot() {
    let home = TempHome::new("bounded-mutation-response");
    let _project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    // DOC-108 retired `POST /v1/missions`; the multi-megabyte historical
    // projection is seeded directly into the ledger, and the bounded-mutation
    // proof uses the retained `POST /v1/team-runs` writer.
    let large_context = "x".repeat(20_000);
    {
        use std::io::Write as _;
        let path = home.spaces_dir().join(&_project_id).join("missions.jsonl");
        let mut ledger = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open mission ledger");
        for index in 0..80 {
            writeln!(
                ledger,
                "{}",
                serde_json::json!({
                    "id": format!("mission-large-{index}"),
                    "title": format!("Large mission {index}"),
                    "objective": "inflate the durable read projection",
                    "context": large_context,
                    "status": "planned",
                    "created_at": "unix-ms:1",
                    "updated_at": "unix-ms:1",
                })
            )
            .expect("seed large mission row");
        }
    }

    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "remain reachable from a deep link",
            "members": [{"name": "deep-link-member", "role": "auditor", "provider": "codex"}],
        }),
    );
    assert_eq!(status, 200, "created: {created}");
    assert!(
        created.get("snapshot").is_none(),
        "mutation response leaked a full snapshot"
    );
    assert!(
        serde_json::to_vec(&created).unwrap().len() < 64 * 1024,
        "mutation response exceeded the bounded envelope"
    );
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    let (status, snapshot) = serve.get_json("/v1/snapshot");
    assert_eq!(status, 200, "snapshot: {snapshot}");
    assert_eq!(
        snapshot["missions"].as_array().map(Vec::len),
        Some(81),
        "the Dashboard refresh GET must still expose every mutation"
    );
    assert!(
        serde_json::to_vec(&snapshot).unwrap().len() > 1_000_000,
        "fixture did not prove the POST response was bounded against a multi-megabyte projection"
    );
    let (status, scoped) = serve.get_json(&format!("/v1/team-runs/{run_id}/snapshot"));
    assert_eq!(status, 200, "scoped: {scoped}");
    assert_eq!(scoped["team_runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(scoped["member_runs"].as_array().map(Vec::len), Some(2));
    assert_eq!(scoped["missions"].as_array().map(Vec::len), Some(0));
    assert!(
        serde_json::to_vec(&scoped).unwrap().len() < 64 * 1024,
        "Team deep-link projection must remain bounded despite a large historical store"
    );
}
