use super::*;

#[test]
fn post_team_run_transition_and_compatibility_lineage() {
    let home = TempHome::new("team-run-transition");
    let project_id = init_project(&home, "alpha");
    let serve = ServeHandle::spawn(&home, home.base(), &[]);

    // Unlinked compatibility attempt 1 (planning).
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt one",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let wave1_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();

    // Unlinked legacy runs retain previous_run_id as compatibility lineage.
    // Native Mission/Wave attempts are covered separately and only link retries
    // inside one Wave.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt two",
            "previous_run_id": wave1_id,
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let runs = snapshot["team_runs"].as_array().expect("team_runs");
    assert_eq!(
        runs.iter()
            .find(|run| run["objective"].as_str() == Some("Compatibility attempt two"))
            .and_then(|run| run["previous_run_id"].as_str()),
        Some(wave1_id.as_str()),
        "compatibility attempt lineage: {runs:?}"
    );

    // An unknown previous run id is rejected, nothing journaled.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Dangling compatibility attempt",
            "previous_run_id": "team-run-nope",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["ok"].as_bool(), Some(false), "body: {body}");

    // Illegal attempt move: planning → completed; an attempt must reach
    // reviewing before it can become completion-eligible.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("invalid team-run transition"),
        "body: {body}"
    );

    // Legal: planning → cancelled, folded into the run row + event log.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("cancelled"));
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let runs = snapshot["team_runs"].as_array().expect("team_runs");
    assert_eq!(
        runs.iter()
            .find(|run| run["id"].as_str() == Some(wave1_id.as_str()))
            .and_then(|run| run["status"].as_str()),
        Some("cancelled"),
        "latest-wins projection shows the cancellation: {runs:?}"
    );
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert!(
        events.iter().any(|event| {
            event["entity_id"].as_str() == Some(wave1_id.as_str())
                && event["operation"].as_str() == Some("updated")
                && event["summary"]
                    .as_str()
                    .unwrap_or("")
                    .contains("cancelled")
        }),
        "a cancellation event was folded: {events:?}"
    );

    // A terminal run cannot transition anywhere.
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave1_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 400, "body: {body}");

    // Flip compatibility attempt 2 to reviewing by appending the row directly
    // (the store is an append-only latest-wins ledger), then complete it.
    let wave2_id = runs
        .iter()
        .find(|run| run["objective"].as_str() == Some("Compatibility attempt two"))
        .and_then(|run| run["id"].as_str())
        .expect("wave 2 id")
        .to_string();
    let store_root = home.spaces_dir().join(&project_id);
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(store_root.join("team_runs.jsonl"))
        .expect("open team_runs.jsonl");
    use std::io::Write as _;
    let mut reviewing = runs
        .iter()
        .find(|run| run["id"].as_str() == Some(wave2_id.as_str()))
        .expect("attempt two row")
        .clone();
    reviewing["status"] = serde_json::json!("reviewing");
    reviewing["updated_at"] = serde_json::json!("unix-ms:2");
    writeln!(ledger, "{}", reviewing).expect("append reviewing row");

    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{wave2_id}/transition"),
        &serde_json::json!({"status": "completed"}),
    );
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["result"]["status"].as_str(), Some("completed"));
    assert!(
        body["result"]["completed_at"].as_str().is_some(),
        "completed_at stamped on attempt completion: {body:?}"
    );
    let (_, snapshot) = serve.get_json("/v1/snapshot");
    let events = snapshot["team_run_events"]
        .as_array()
        .expect("team_run_events");
    assert!(
        events.iter().any(|event| {
            event["entity_id"].as_str() == Some(wave2_id.as_str())
                && event["operation"].as_str() == Some("completed")
                && event["summary"]
                    .as_str()
                    .unwrap_or("")
                    .contains("team-run attempt completed")
        }),
        "the attempt-completion event was folded: {events:?}"
    );

    // The CLI arms share the same lifecycle: completing an already-completed run is
    // a usage error, and cancelling a planning run succeeds.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "complete",
            "--id",
            &wave2_id,
        ],
    );
    assert!(
        !out.status.success(),
        "completing a completed run must fail: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid team-run transition"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Compatibility attempt three",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let wave3_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("run id")
        .to_string();
    let cancelled = team_run_json(&home, &project_id, &["cancel", "--id", &wave3_id, "--json"]);
    assert_eq!(cancelled["status"].as_str(), Some("cancelled"));

    // A status-only cancellation must not lie about stopping active provider
    // work. Until cooperative interruption exists, running -> cancelled is
    // rejected by the shared CLI/HTTP transition contract.
    let (status, body) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Active compatibility attempt",
            "members": [{"name": "lead", "role": "coordinator", "provider": "kimi"}],
        }),
    );
    assert_eq!(status, 200, "body: {body}");
    let running_id = body["result"]["team_run"]["id"]
        .as_str()
        .expect("running attempt id")
        .to_string();
    let mut running_row = body["result"]["team_run"].clone();
    running_row["status"] = serde_json::json!("running");
    running_row["updated_at"] = serde_json::json!("unix-ms:4");
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(store_root.join("team_runs.jsonl"))
        .expect("open team_runs.jsonl");
    writeln!(ledger, "{}", running_row).expect("append running row");
    let (status, body) = serve.post_json(
        &format!("/v1/team-runs/{running_id}/transition"),
        &serde_json::json!({"status": "cancelled"}),
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("running cancellation requires provider interruption"),
        "body: {body}"
    );

    // Once an operator has independently stopped every provider process, the
    // explicit recovery path terminates the stale attempt and its members with
    // an auditable reason. It is deliberately separate from status-only cancel.
    let recovered = team_run_json(
        &home,
        &project_id,
        &[
            "cancel",
            "--id",
            &running_id,
            "--confirm-provider-stopped",
            "--reason",
            "foreground orchestrator was interrupted",
            "--cancelled-by",
            "test-operator",
            "--json",
        ],
    );
    assert_eq!(recovered["status"].as_str(), Some("cancelled"));
    let recovered_status = team_run_json(
        &home,
        &project_id,
        &["status", "--id", &running_id, "--json"],
    );
    assert_eq!(
        recovered_status["members"][0]["member_run"]["status"].as_str(),
        Some("stopped")
    );
    assert_eq!(
        recovered_status["members"][0]["latest_action"]["action_type"].as_str(),
        Some("interrupted")
    );
    assert_eq!(
        recovered_status["members"][0]["latest_action"]["status"].as_str(),
        Some("cancelled")
    );
}
