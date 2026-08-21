use super::*;

#[test]
fn mission_log_cli_and_legacy_wave_read_are_independent_of_team_run() {
    let home = TempHome::new("mission-wave-cli");
    let project_id = init_project(&home, "alpha");
    // DOC-108 retired the Mission writers; this pre-cutover Mission and its
    // Log rows are seeded directly as history, and the legacy reads must
    // still serve them.
    firm_env::seed_historical_mission(&home, &project_id, "mission-cli", "CLI Mission");
    // `wave create` is retired (ADR 0051): seed a row solely to prove the
    // explicit Legacy read surface. Current TeamRun identity does not cite it.
    seed_historical_wave(
        &home,
        &project_id,
        "wave-cli",
        "mission-cli",
        1,
        "agent_team",
    );

    let run = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "empty completion",
            "--agent-team-id",
            FIXTURE_TEAM_ID,
            "--member",
            "worker:implementer:kimi",
            "--json",
        ],
    );
    let run_id = run["team_run"]["id"].as_str().unwrap().to_string();
    let mut reviewing = run["team_run"].clone();
    reviewing["status"] = serde_json::json!("reviewing");
    reviewing["updated_at"] = serde_json::json!("unix-ms:review-ready");
    use std::io::Write as _;
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(home.spaces_dir().join(&project_id).join("team_runs.jsonl"))
        .expect("open team run ledger");
    writeln!(ledger, "{reviewing}").expect("append reviewing row");
    let completed = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "complete",
            "--id",
            &run_id,
        ],
    );
    assert!(
        completed.status.success(),
        "team completion failed: {}",
        String::from_utf8_lossy(&completed.stderr)
    );
    let waiting_wave = command_json(
        &home,
        &project_id,
        &["legacy", "wave", "show", "--id", "wave-cli", "--json"],
    );
    assert_eq!(waiting_wave["status"].as_str(), Some("planned"));
    let running_mission = command_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-cli", "--json"],
    );
    assert_eq!(running_mission["status"].as_str(), Some("planned"));

    // `wave gate` is retired (ADR 0051) and the Mission writers are retired
    // (DOC-108): there is nothing left to accept, close, or append through
    // the legacy surfaces.
    for args in [
        vec![
            "wave",
            "gate",
            "--id",
            "wave-cli",
            "--status",
            "accepted",
            "--run-id",
            run_id.as_str(),
        ],
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-cli",
            "--kind",
            "closeout_evidence",
            "--body",
            "must not persist",
        ],
        vec![
            "mission",
            "close",
            "--id",
            "mission-cli",
            "--outcome",
            "must not persist",
        ],
    ] {
        let mut full = vec!["--project", project_id.as_str()];
        full.extend(args.clone());
        let out = run_firm(&home, home.base(), &full);
        assert!(
            !out.status.success(),
            "harness {args:?} must fail as retired"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("retired"),
            "harness {args:?} stderr must name the retirement"
        );
    }
    // The Mission stays exactly as seeded: no closeout row, no status flip.
    let still_planned = command_json(
        &home,
        &project_id,
        &["mission", "show", "--id", "mission-cli", "--json"],
    );
    assert_eq!(still_planned["status"].as_str(), Some("planned"));
    assert!(still_planned["completed_at"].is_null());
    let log = command_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-cli",
            "--json",
        ],
    );
    assert_eq!(log.as_array().map(Vec::len), Some(0));
}
