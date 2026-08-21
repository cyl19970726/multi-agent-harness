use super::*;

#[test]
#[cfg(any())] // Historical Wave4A CLI-mail contract; canonical fabric coverage replaces it.
fn team_run_cli_create_list_status_send_events() {
    let home = TempHome::new("team-run-cli");
    let project_id = init_project(&home, "alpha");
    seed_mission_with_legacy_wave(&home, &project_id);
    let project_root = std::fs::canonicalize(home.base().join("alpha"))
        .expect("canonical project root")
        .display()
        .to_string();

    // create (plain output): bare run id on stdout.
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
            "Ship v0",
            "--budget-usd",
            "5.5",
            "--execution-root",
            &project_root,
            "--member",
            "lead:coordinator:kimi#Coordinate the delivery",
            "--member",
            "worker-1:implementer:codex:gpt-5@crates/a,docs#Implement and verify the change",
            "--member-effort",
            "worker-1:max",
            "--member-service-tier",
            "worker-1:priority",
            "--member-worktree",
            &format!("worker-1:{project_root}"),
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(run_id.starts_with("team-run-"), "run id: {run_id}");

    // list --json: one run, durable Team identity/budget/member ids carried through.
    let runs = team_run_json(&home, &project_id, &["list", "--json"]);
    let runs = runs.as_array().expect("runs array");
    assert_eq!(runs.len(), 1, "runs: {runs:?}");
    assert_eq!(runs[0]["id"].as_str(), Some(run_id.as_str()));
    assert_eq!(runs[0]["status"].as_str(), Some("planning"));
    assert_eq!(runs[0]["agent_team_id"].as_str(), Some(FIXTURE_TEAM_ID));
    assert!(runs[0].get("mission_id").is_none());
    assert!(runs[0].get("wave_id").is_none());
    assert_eq!(
        runs[0]["execution_root"].as_str(),
        Some(project_root.as_str())
    );
    assert_eq!(runs[0]["budget_limit_usd"].as_f64(), Some(5.5));
    let member_ids: Vec<&str> = runs[0]["member_run_ids"]
        .as_array()
        .expect("member_run_ids")
        .iter()
        .filter_map(|id| id.as_str())
        .collect();
    assert_eq!(member_ids.len(), 2, "member ids: {member_ids:?}");

    // status --json: members + no actions yet + no conversation mail.
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(status["team_run"]["id"].as_str(), Some(run_id.as_str()));
    let members = status["members"].as_array().expect("members");
    assert_eq!(members.len(), 2, "members: {members:?}");
    let controlled_member = members
        .iter()
        .find(|entry| entry["member_run"]["name"].as_str() == Some("worker-1"))
        .expect("worker-1 ProviderRuntimeProjection");
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["model"]["requested"].as_str(),
        Some("gpt-5")
    );
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["reasoning_effort"]["requested"]
            .as_str(),
        Some("max")
    );
    assert_eq!(
        controlled_member["member_run"]["provider_controls"]["service_tier"]["requested"].as_str(),
        Some("priority")
    );
    assert_eq!(
        members[0]["member_run"]["name"].as_str(),
        Some("lead"),
        "member order follows --member order"
    );
    assert_eq!(members[1]["member_run"]["model"].as_str(), Some("gpt-5"));
    assert_eq!(
        members[1]["member_run"]["provider_cwd_hint"].as_str(),
        Some(project_root.as_str())
    );
    assert_eq!(
        members[1]["member_run"]["owned_paths"],
        serde_json::json!(["crates/a", "docs"]),
        "owned_paths parsed from @path1,path2"
    );
    assert!(
        members.iter().all(|m| m["latest_action"].is_null()),
        "no member actions journaled yet: {members:?}"
    );
    assert_eq!(
        status["unacked_messages"].as_u64(),
        Some(0),
        "queued deliveries are not actionable manual acknowledgements"
    );
    let member_detail = command_json(
        &home,
        &project_id,
        &["member-run", "show", "--id", member_ids[1], "--json"],
    );
    assert_eq!(
        member_detail["member_run"]["id"].as_str(),
        Some(member_ids[1])
    );
    assert_eq!(
        member_detail["team_run"]["id"].as_str(),
        Some(run_id.as_str())
    );
    assert_eq!(
        member_detail["works"].as_array().map(Vec::len),
        Some(1),
        "member detail includes its durable owned Work"
    );
    assert_eq!(
        member_detail["mailbox"]["inbox"].as_array().map(Vec::len),
        Some(0),
        "Work ownership is not duplicated into TeamMessageProjection"
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let works = store.latest_works().expect("latest Works");
    let worker_work = works
        .iter()
        .find(|work| work.active_member_run_id.as_deref() == Some(member_ids[1]))
        .expect("worker Work");

    // send --json: a blocker from the worker to the lead.
    let message = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--body",
            "BLOCKER: API design is unresolved",
            "--work-id",
            &worker_work.id,
            "--json",
        ],
    );
    assert_eq!(message["kind"].as_str(), Some("message"));
    assert_eq!(message["sender_runtime_id"].as_str(), Some(member_ids[1]));
    assert_eq!(message["team_run_id"].as_str(), Some(run_id.as_str()));
    assert_eq!(
        message["deliveries"][0]["status"].as_str(),
        Some("queued"),
        "delivery queued: {message:?}"
    );
    assert!(
        !message["correlation_id"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "correlation id assigned"
    );
    assert!(
        message["response_intent"].is_null(),
        "peer-to-peer message mail carries no explicit intent (informational by default): {message:?}"
    );

    // Sender-aware default (ADR 0046 §4): the same bare `--kind message` from
    // Host stays response-required, because `message` is the only legal
    // carrier for Host questions, revisions, and acceptance decisions.
    let host_mail = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--body",
            "Revise the API surface and report back",
            "--json",
        ],
    );
    assert!(
        host_mail["response_intent"].is_null(),
        "Host mail also carries no explicit intent; the default is sender-aware: {host_mail:?}"
    );

    // --informational is the explicit downward override for Host mail that is
    // genuinely FYI-only, mirroring the HTTP/MCP `response_intent` field.
    let host_fyi = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--informational",
            "--body",
            "FYI: the nightly gate is green",
            "--json",
        ],
    );
    assert_eq!(
        host_fyi["response_intent"].as_str(),
        Some("informational"),
        "CLI --informational sets the explicit downward override: {host_fyi:?}"
    );

    // --response-required marks mail that must wake an idle peer into a new
    // provider round (ADR 0046 §4).
    let flagged = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            member_ids[0],
            "--kind",
            "message",
            "--response-required",
            "--body",
            "QUESTION: which API revision should the peer lane implement?",
            "--json",
        ],
    );
    assert_eq!(
        flagged["response_intent"].as_str(),
        Some("response_required"),
        "CLI --response-required sets explicit intent: {flagged:?}"
    );
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            member_ids[0],
            "--json",
        ],
    );
    assert!(
        inbox
            .as_array()
            .expect("CLI inbox array")
            .iter()
            .any(|item| item["id"] == message["id"]),
        "CLI inbox must expose peer coordination mail: {inbox}"
    );

    // events --json: 5 create-time events + 4 send events, seq 1..=9 in order.
    let events = team_run_json(&home, &project_id, &["events", "--id", &run_id, "--json"]);
    let events = events.as_array().expect("events array");
    assert_eq!(events.len(), 9, "events: {events:?}");
    let seqs: Vec<u64> = events.iter().filter_map(|e| e["seq"].as_u64()).collect();
    assert_eq!(
        seqs,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
        "seq strictly increasing"
    );
    assert_eq!(events[0]["entity_type"].as_str(), Some("team_run"));
    assert_eq!(events[0]["operation"].as_str(), Some("created"));
    assert_eq!(events[0]["source_kind"].as_str(), Some("host"));
    // The send folded a member-sourced message event (v0: no member status flip).
    let last = &events[5];
    assert_eq!(last["entity_type"].as_str(), Some("message"));
    assert_eq!(last["source_kind"].as_str(), Some("member"));
    assert_eq!(last["member_run_id"].as_str(), Some(member_ids[1]));

    // events --after-seq 5: only the four send events remain.
    let tail = team_run_json(
        &home,
        &project_id,
        &["events", "--id", &run_id, "--after-seq", "5", "--json"],
    );
    let tail = tail.as_array().expect("tail array");
    assert_eq!(tail.len(), 4, "tail: {tail:?}");
    let tail_seqs: Vec<u64> = tail.iter().filter_map(|e| e["seq"].as_u64()).collect();
    assert_eq!(tail_seqs, vec![6, 7, 8, 9]);

    // Member-to-Host mail is actionable immediately; CLI ACK is the complete
    // control-plane path and removes it from the default Inbox without erasing
    // the latest historical projection.
    let host_message = team_run_json(
        &home,
        &project_id,
        &[
            "send",
            "--id",
            &run_id,
            "--from",
            member_ids[1],
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "RESULT: ready for Host review",
            "--work-id",
            &worker_work.id,
            "--correlation-id",
            message["correlation_id"]
                .as_str()
                .expect("conversation correlation"),
            "--causation-id",
            message["id"].as_str().expect("conversation root"),
            "--json",
        ],
    );
    assert_eq!(
        host_message["deliveries"][0]["status"].as_str(),
        Some("delivered")
    );
    let host_inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(host_inbox.as_array().map(Vec::len), Some(1));
    let ack = team_run_json(
        &home,
        &project_id,
        &[
            "ack",
            "--id",
            &run_id,
            "--message-id",
            host_message["id"].as_str().expect("Host message id"),
            "--member-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(
        ack["deliveries"][0]["status"].as_str(),
        Some("acknowledged")
    );
    let actionable_after_ack = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--json",
        ],
    );
    assert_eq!(actionable_after_ack.as_array().map(Vec::len), Some(0));
    let history_after_ack = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            "host",
            "--all",
            "--json",
        ],
    );
    assert_eq!(history_after_ack.as_array().map(Vec::len), Some(1));

    // create --json: the full created bundle (run + member runs + Works).
    let created = team_run_json(
        &home,
        &project_id,
        &[
            "create",
            "--objective",
            "Second run",
            "--member",
            "solo:worker:kimi#Complete the solo lane",
            "--json",
        ],
    );
    assert_eq!(created["team_run"]["status"].as_str(), Some("planning"));
    assert_eq!(
        created["member_runs"].as_array().map(Vec::len),
        Some(1),
        "member runs: {created:?}"
    );
    let works = created["works"].as_array().expect("Works");
    assert_eq!(works.len(), 1);
    assert_eq!(works[0]["phase"].as_str(), Some("open"));
    assert!(works[0]["active_member_run_id"].as_str().is_some());
}
