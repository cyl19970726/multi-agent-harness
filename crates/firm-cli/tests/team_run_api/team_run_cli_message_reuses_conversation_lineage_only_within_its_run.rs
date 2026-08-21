use super::*;

#[test]
#[cfg(any())] // Historical conversation projection over retired `team-run send` authority.
fn team_run_cli_message_reuses_conversation_lineage_only_within_its_run() {
    let home = TempHome::new("team-run-cli-lineage");
    let project_id = init_project(&home, "alpha");
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Correlate work",
            "--member",
            "lead:coordinator:kimi",
            "--member",
            "worker:implementer:kimi",
            "--json",
        ],
    );
    let run_id = created["team_run"]["id"].as_str().unwrap().to_string();
    let members = created["member_runs"].as_array().unwrap();
    let root = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "Please coordinate this conversation",
            "--informational",
            "--json",
        ],
    );
    let root_id = root["id"].as_str().unwrap();
    let correlation_id = root["correlation_id"].as_str().unwrap();

    let handoff = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "handoff linked to the conversation",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
            "--json",
        ],
    );
    assert_eq!(handoff["correlation_id"].as_str(), Some(correlation_id));
    assert_eq!(handoff["causation_id"].as_str(), Some(root_id));

    // A causation-only reply inherits its direct cause's correlation rather
    // than fabricating a fresh one.
    let reply = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            members[1]["id"].as_str().unwrap(),
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "acknowledged",
            "--causation-id",
            handoff["id"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(reply["correlation_id"].as_str(), Some(correlation_id));
    assert_eq!(reply["causation_id"].as_str(), handoff["id"].as_str());

    let foreign = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Separate team boundary",
            "--member",
            "outsider:implementer:kimi",
            "--json",
        ],
    );
    let foreign_member_id = foreign["member_runs"][0]["id"].as_str().unwrap();
    let messages_before_invalid = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read messages before invalid sends")
    .lines()
    .count();

    // A member from another TeamRun cannot impersonate a sender in this run,
    // even when it presents valid conversation lineage from the target run.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            foreign_member_id,
            "--to",
            members[0]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "cross-run impersonation",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not belong to team run"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Recipient membership is checked before any message or event is written.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            "member-run-unknown",
            "--kind",
            "message",
            "--body",
            "unknown recipient",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            root_id,
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not belong to team run"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let messages_after_invalid = std::fs::read_to_string(
        home.spaces_dir()
            .join(&project_id)
            .join("team_messages.jsonl"),
    )
    .expect("read messages after invalid sends")
    .lines()
    .count();
    assert_eq!(messages_after_invalid, messages_before_invalid);

    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "unproven correlation",
            "--correlation-id",
            "corr-not-a-conversation",
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not identify a conversation"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let second_root = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "A separate conversation",
            "--json",
        ],
    );

    // A causation id from the same run must still agree with an explicitly
    // supplied correlation; the rejected send leaves the event stream intact.
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            members[0]["id"].as_str().unwrap(),
            "--to",
            members[1]["id"].as_str().unwrap(),
            "--kind",
            "message",
            "--body",
            "mismatched lineage",
            "--correlation-id",
            correlation_id,
            "--causation-id",
            second_root["id"].as_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "unexpected success: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("has correlation_id"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = team_run_json(&home, &project_id, &["events", "--id", &run_id, "--json"]);
    assert_eq!(events.as_array().map(Vec::len), Some(7));
}
