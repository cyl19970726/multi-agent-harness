use super::*;

#[test]
fn sse_invalidates_team_run_projection_and_snapshot_converges() {
    let home = TempHome::new("team-run-sse");
    let project_id = init_project(&home, "alpha");

    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let mut sse = serve.open_sse("");

    // Create a run AFTER the stream is live. SSE carries freshness only; the
    // durable TeamRun row must be recovered from the authoritative snapshot.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--objective",
            "Stream me",
            "--member",
            "solo:worker:kimi",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // Collect the complete create burst and prove it contains only scoped
    // invalidations, never a durable TeamRun row that the browser could fold.
    let frames = collect_sse_data(&mut sse, Duration::from_secs(6), 6);
    assert!(
        frames.iter().any(|frame| {
            frame["scope"].as_str() == Some("execution_space")
                && frame["scope_id"].as_str() == Some(project_id.as_str())
                && matches!(
                    frame["ledger"].as_str(),
                    Some("team_runs.jsonl" | "team_run_events.jsonl")
                )
        }),
        "expected a scoped TeamRun invalidation for {run_id}; got: {frames:?}"
    );
    assert!(
        frames
            .iter()
            .all(|frame| frame.get("entity_type").is_none()),
        "SSE must not publish durable TeamRun row truth: {frames:?}"
    );
    let (status, snapshot) = serve.get_json(&format!("/v1/snapshot?project={project_id}"));
    assert_eq!(status, 200, "snapshot: {snapshot}");
    assert!(snapshot["team_runs"]
        .as_array()
        .is_some_and(|runs| runs.iter().any(|run| run["id"] == run_id)));
}
