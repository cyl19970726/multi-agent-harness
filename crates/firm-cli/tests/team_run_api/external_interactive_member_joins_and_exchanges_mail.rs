use super::*;

#[test]
#[cfg(any())] // Historical external-member CLI mail flow; current Team fabric is Store-live tested.
fn external_interactive_member_joins_and_exchanges_mail() {
    let home = TempHome::new("team-run-external-interactive");
    let project_id = init_project(&home, "alpha");
    let _node_runtime = ServeHandle::spawn(&home, home.base(), &[]);

    // A declared external interactive member may use an arbitrary provider
    // label because Harness never executes it or claims adapter capability.
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
            "custom external provider",
            "--member",
            "custom-reviewer:reviewer:local-agent/external_interactive",
        ],
    );
    assert!(
        out.status.success(),
        "custom external provider must be accepted: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let custom_run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let custom = team_run_json(
        &home,
        &project_id,
        &["status", "--id", &custom_run_id, "--json"],
    );
    assert_eq!(
        custom["members"][0]["member_run"]["provider"],
        "local-agent"
    );
    assert_eq!(
        custom["members"][0]["member_run"]["provider_profile"]["execution_driver"],
        "user_driven"
    );

    // Create a run whose only member is the user's own external interactive
    // session; Harness spawns nothing for it.
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
            "Review the external lane",
            "--member",
            "ext-reviewer:reviewer:kimi/external_interactive#Review the external lane",
        ],
    );
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    let members = status["members"].as_array().expect("members");
    assert_eq!(members.len(), 1, "members: {members:?}");
    let ext = &members[0]["member_run"];
    let ext_id = ext["id"].as_str().expect("external member id").to_string();
    assert_eq!(ext["status"].as_str(), Some("idle"));
    assert_eq!(
        ext["provider_profile"]["execution_mode"].as_str(),
        Some("external_interactive")
    );
    assert_eq!(
        ext["provider_profile"]["execution_driver"].as_str(),
        Some("user_driven")
    );
    assert!(
        ext["native_session"].is_null(),
        "external members have no native session record: {ext}"
    );
    assert!(
        ext["provider_environment_observation"].is_null(),
        "external members get no Harness workspace snapshot: {ext}"
    );

    // add-member accepts the same mode on an active run and records optional
    // initial Work without duplicating ownership into chat.
    let added = team_run_json(
        &home,
        &project_id,
        &[
            "add-member",
            "--id",
            &run_id,
            "--member",
            "ext-helper:helper:codex/external_interactive",
            "--initial-work",
            "Pair on the review",
        ],
    );
    let helper_id = added["member_run"]["id"]
        .as_str()
        .expect("helper member id")
        .to_string();
    assert_eq!(
        added["member_run"]["provider_profile"]["execution_mode"].as_str(),
        Some("external_interactive")
    );
    assert_eq!(
        (
            added["work"]["owner_member_id"].as_str(),
            added["work"]["active_member_run_id"].is_null(),
            added["work"]["assignee_membership_id"].as_str().is_some(),
        ),
        (Some("ext-helper"), true, true),
        "initial Work must carry stable responsibility without runtime identity: {added}"
    );

    // The Supervisor starts the run without spawning an adapter for external
    // members: no adapter error, no Failed status, and start returns promptly
    // because there is nothing to drive.
    let out = run_firm_with_env(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &[("HARNESS_MEMBER_SUPERVISOR_TEST_IDLE_MS", "100")],
    );
    assert!(
        out.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        !stdout.contains("adapter not implemented"),
        "start output: {stdout}"
    );
    let status = team_run_json(&home, &project_id, &["status", "--id", &run_id, "--json"]);
    assert_eq!(status["team_run"]["status"].as_str(), Some("running"));
    for entry in status["members"].as_array().expect("members") {
        let member_status = entry["member_run"]["status"]
            .as_str()
            .expect("member status");
        assert!(
            !matches!(member_status, "failed" | "disconnected"),
            "external member must not be marked {member_status}: {entry}"
        );
    }

    // Host → external member: the delivery stays queued until the session
    // polls its inbox itself.
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
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "Please review crates/firm-core",
        ],
    );
    assert!(
        out.status.success(),
        "host send failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let host_message_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let all_mail = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--all",
            "--json",
        ],
    );
    let correlation = all_mail
        .as_array()
        .expect("external inbox history")
        .iter()
        .find(|message| message["id"].as_str() == Some(host_message_id.as_str()))
        .and_then(|message| message["correlation_id"].as_str())
        .expect("conversation correlation")
        .to_string();

    // The external session polls ordinary mail and acks what it consumed.
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    let inbox_ids: Vec<&str> = inbox
        .as_array()
        .expect("inbox")
        .iter()
        .filter_map(|message| message["id"].as_str())
        .collect();
    assert_eq!(inbox_ids, vec![host_message_id.as_str()]);
    let out = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external ack failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    assert_eq!(
        inbox.as_array().map(Vec::len),
        Some(0),
        "acked mail leaves the actionable inbox: {inbox}"
    );

    // External member → Host reply keeps the conversation correlation and names
    // its direct cause.
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
            &ext_id,
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "Review done: no defects found",
            "--correlation-id",
            &correlation,
            "--causation-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external reply failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reply_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
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
    let reply = host_inbox
        .as_array()
        .expect("host inbox")
        .iter()
        .find(|message| message["id"].as_str() == Some(reply_id.as_str()))
        .expect("reply in host inbox");
    assert_eq!(reply["sender_runtime_id"].as_str(), Some(ext_id.as_str()));
    assert_eq!(reply["correlation_id"].as_str(), Some(correlation.as_str()));
    assert_eq!(
        reply["causation_id"].as_str(),
        Some(host_message_id.as_str())
    );

    // Closing an external member freezes its Harness coordination; there is
    // no provider runtime or native session under Harness control to clean up.
    let closed = team_run_json(
        &home,
        &project_id,
        &[
            "close-member",
            "--id",
            &run_id,
            "--member-run-id",
            &helper_id,
            "--reason",
            "review pair no longer needed",
        ],
    );
    assert_eq!(
        closed["status"].as_str(),
        Some("stopped"),
        "close: {closed}"
    );
    assert_eq!(closed["runtime"].as_str(), Some("external_unmanaged"));
    assert_eq!(closed["runtime_effect"].as_str(), Some("none"));
    assert_eq!(
        closed["coordination_effect"].as_str(),
        Some("member_closed")
    );
    let store = HarnessStore::new(home.spaces_dir().join(&project_id));
    let helper = store
        .member_runs()
        .expect("member rows")
        .into_iter()
        .rev()
        .find(|member| member.id == helper_id)
        .expect("helper member row");
    assert_eq!(helper.status, harness_core::MemberRunStatus::Stopped);
    assert_eq!(
        helper.coordination_status,
        harness_core::MemberCoordinationStatus::Closed
    );
    assert!(
        store
            .latest_team_member_close_request(&helper_id)
            .expect("close request")
            .is_some_and(|close| close.status == harness_core::TeamMemberCloseStatus::Applied),
        "close request must be applied without a supervisor"
    );

    // An external-only TeamRun remains Host-controlled: after a correlated
    // Handoff the Host may close the coordination binding and explicitly
    // complete the run without claiming that any external process was stopped.
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
            &ext_id,
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "External review handoff: checks reported by the user-driven member",
            "--correlation-id",
            &correlation,
            "--causation-id",
            &host_message_id,
        ],
    );
    assert!(
        out.status.success(),
        "external handoff failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Leave one message queued so Close can prove that the frozen coordination
    // binding cannot send, receive, or ACK until explicit Reopen.
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
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "Queued before coordination close",
            "--correlation-id",
            &correlation,
        ],
    );
    assert!(
        out.status.success(),
        "pre-close send failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let queued_before_close_id = String::from_utf8_lossy(&out.stdout).trim().to_string();

    let reviewer_closed = team_run_json(
        &home,
        &project_id,
        &[
            "close-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "Host accepted external review",
        ],
    );
    assert_eq!(reviewer_closed["runtime_effect"].as_str(), Some("none"));

    for args in [
        vec![
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            &ext_id,
            "--to",
            "host",
            "--kind",
            "message",
            "--body",
            "must not send after close",
            "--correlation-id",
            &correlation,
        ],
        vec![
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            &run_id,
            "--from",
            "host",
            "--to",
            &ext_id,
            "--kind",
            "message",
            "--body",
            "must not queue after close",
            "--correlation-id",
            &correlation,
        ],
        vec![
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &queued_before_close_id,
        ],
    ] {
        let out = run_firm(&home, home.base(), &args);
        assert!(
            !out.status.success()
                && String::from_utf8_lossy(&out.stderr).contains("coordination is closed"),
            "closed external coordination must reject {args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let reopened = team_run_json(
        &home,
        &project_id,
        &[
            "reopen-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "continue the same external review",
        ],
    );
    assert_eq!(reopened["member_run"]["id"].as_str(), Some(ext_id.as_str()));
    assert_eq!(
        reopened["member_run"]["coordination_status"].as_str(),
        Some("active")
    );
    assert_eq!(
        reopened["member_run"]["runtime_generation"].as_u64(),
        Some(2)
    );
    assert_eq!(
        reopened["runtime_activation"].as_str(),
        Some("external_user_driven")
    );
    let reopened_inbox = team_run_json(
        &home,
        &project_id,
        &[
            "inbox",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--json",
        ],
    );
    assert!(
        reopened_inbox.as_array().is_some_and(|messages| messages
            .iter()
            .any(|message| { message["id"].as_str() == Some(queued_before_close_id.as_str()) })),
        "mail queued before close must thaw after reopen: {reopened_inbox}"
    );
    let ack = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "ack",
            "--id",
            &run_id,
            "--member-id",
            &ext_id,
            "--message-id",
            &queued_before_close_id,
        ],
    );
    assert!(
        ack.status.success(),
        "reopened external member must ACK frozen mail: {}",
        String::from_utf8_lossy(&ack.stderr)
    );

    let retired = team_run_json(
        &home,
        &project_id,
        &[
            "deactivate-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
            "--reason",
            "external reviewer retired",
        ],
    );
    assert_eq!(retired["coordination_status"].as_str(), Some("retired"));
    let reopen_retired = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "reopen-member",
            "--id",
            &run_id,
            "--member-run-id",
            &ext_id,
        ],
    );
    assert!(
        !reopen_retired.status.success()
            && String::from_utf8_lossy(&reopen_retired.stderr).contains("is retired"),
        "retired member must not reopen: {}",
        String::from_utf8_lossy(&reopen_retired.stderr)
    );

    // TeamRun completion is not Work acceptance. This scenario exercised
    // external coordination only, so the Host explicitly cancels its two
    // untouched Works before ending the run.
    let works = team_run_json(
        &home,
        &project_id,
        &["work", "list", "--team-run-id", &run_id],
    );
    for work in works.as_array().expect("Work list") {
        let work_id = work["id"].as_str().expect("Work id");
        let version = work["version"].as_u64().expect("Work version").to_string();
        team_run_json(
            &home,
            &project_id,
            &[
                "work",
                "cancel",
                "--work-id",
                work_id,
                "--expected-version",
                &version,
                "--reason",
                "external coordination scenario ended without execution",
            ],
        );
    }
    let completed = team_run_json(&home, &project_id, &["complete", "--id", &run_id, "--json"]);
    assert_eq!(completed["status"].as_str(), Some("completed"));
}
