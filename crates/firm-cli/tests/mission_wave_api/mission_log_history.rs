use super::*;

/// `mission log show` is a read-only legacy read (DOC-108 retired the
/// append writer): revision order, --tail, plain-text vs --json, and the "no
/// mission log yet" sentinel are proven against directly-seeded pre-cutover
/// history — the only way Mission Log rows may exist now.
#[test]
fn mission_log_cli_show_reads_history_and_append_is_retired() {
    let home = TempHome::new("mission-log-cli-happy-path");
    let project_id = init_project(&home, "alpha");
    seed_historical_mission(&home, &project_id, "mission-log-happy", "Mission Log reads");

    // Append is retired, whatever the payload: empty body, unknown kind, and
    // well-formed rows all fail with the DOC-108 retired-write error and
    // write nothing.
    for args in [
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "judgment",
            "--body",
            "   ",
        ],
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "narration",
            "--body",
            "not a real kind",
        ],
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-happy",
            "--kind",
            "judgment",
            "--body",
            "must not persist",
        ],
        vec![
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-log-does-not-exist",
            "--kind",
            "judgment",
            "--body",
            "orphan",
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

    // A Mission with no entries shows the explicit sentinel in text mode, not
    // an empty line or an error.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
        ],
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "no mission log yet"
    );

    // Seed pre-cutover history directly, then prove the reads.
    for (revision, kind, body, actor) in [
        (1, "judgment", "First judgment.", "host"),
        (2, "replan", "Re-planned after review.", "operator-a"),
        (3, "recovery", "Recovered after a supervisor death.", "host"),
        (
            4,
            "closeout_evidence",
            "Everything verified; closing.",
            "host",
        ),
    ] {
        seed_historical_mission_log(
            &home,
            &project_id,
            "mission-log-happy",
            revision,
            kind,
            body,
            actor,
        );
    }

    // --json show: full ordered history with correct kinds and actors.
    let all_json = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
            "--json",
        ],
    );
    let all_json = all_json.as_array().expect("entries array");
    assert_eq!(all_json.len(), 4);
    assert_eq!(
        all_json
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        all_json
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["judgment", "replan", "recovery", "closeout_evidence"]
    );
    assert_eq!(all_json[0]["actor"].as_str(), Some("host"));
    assert_eq!(all_json[1]["actor"].as_str(), Some("operator-a"));

    // --tail 2 in --json mode: last two only, oldest-of-the-tail first.
    let tail_json = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
            "--tail",
            "2",
            "--json",
        ],
    );
    let tail_json = tail_json.as_array().expect("tail entries array");
    assert_eq!(
        tail_json
            .iter()
            .map(|entry| entry["revision"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![3, 4]
    );

    // Plain-text show (no --json): every body appears, in revision order.
    let text_out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "mission",
            "log",
            "show",
            "--mission-id",
            "mission-log-happy",
        ],
    );
    assert!(text_out.status.success());
    let text = String::from_utf8_lossy(&text_out.stdout).to_string();
    let first_pos = text.find("First judgment.").expect("revision 1 body");
    let replan_pos = text
        .find("Re-planned after review.")
        .expect("revision 2 body");
    let recovery_pos = text
        .find("Recovered after a supervisor death.")
        .expect("revision 3 body");
    let closeout_pos = text
        .find("Everything verified; closing.")
        .expect("revision 4 body");
    assert!(
        first_pos < replan_pos && replan_pos < recovery_pos && recovery_pos < closeout_pos,
        "plain-text show must render entries in revision order: {text}"
    );
    assert!(text.contains("[judgment]"), "text: {text}");
    assert!(text.contains("[closeout_evidence]"), "text: {text}");
}
