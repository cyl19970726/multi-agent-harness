use super::*;

#[test]
fn verifier_review_message_cli_freezes_actual_sender_session() {
    let home = TempHome::new("verifier-member-message");
    let fixture = bootstrap_runtime(&home, "project");
    let fake_bin = fake_provider::install_kimi_acp_shim(home.base());
    let kimi = fake_bin.join("kimi");
    // Modify only this isolated provider fixture: invoke the shipped Member
    // CLI with the capability supplied by the real Supervisor to this process.
    let script = std::fs::read_to_string(&kimi).unwrap();
    let start = script
        .find("      if [ \"${FAKE_KIMI_MESSAGE_DURING_TURN:-0}\" = \"1\" ]; then")
        .unwrap();
    let end = start
        + script[start..]
            .find("      if [ \"$ask\" = \"1\" ]; then")
            .unwrap();
    let replacement = r#"      if [ -n "${F1_MESSAGE_MARKER:-}" ] && [ ! -e "$F1_MESSAGE_MARKER" ]; then
        sleep 0.1
        work_id=$("$FIRM_BIN" --project "$FIRM_PROJECT_ID" team-run work list \
          --team-run-id "$FIRM_TEAM_RUN_ID" --member-run-id "$FIRM_MEMBER_RUN_ID" \
          | sed -n 's/.*"id": "\([^"]*\)".*/\1/p' | sed -n '1p')
        "$FIRM_BIN" member message send --recipient-agent-id "$F1_RECIPIENT" \
          --body 'REVIEW_RESULT
Verdict: Pass' --work-id "$work_id" --idempotency-key verifier-cli-review \
          > "$F1_MESSAGE_MARKER" 2>&1
      fi
"#;
    std::fs::write(
        &kimi,
        format!("{}{}{}", &script[..start], replacement, &script[end..]),
    )
    .unwrap();
    let run_id = create_run(&home, &fixture, "verifier-worker", &[]);
    let marker = home.base().join("review-cli.json");
    let fake_path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let kimi_path = kimi.display().to_string();
    let marker_path = marker.display().to_string();
    let env = [
        ("PATH", fake_path.as_str()),
        ("KIMI_CODE_BIN", kimi_path.as_str()),
        ("FAKE_KIMI_VERSION", "0.36.1"),
        ("F1_MESSAGE_MARKER", marker_path.as_str()),
        ("F1_RECIPIENT", fixture.host_id.as_str()),
    ];
    let socket = node_daemon_socket_path(&home, &fixture.node_id);
    let mut daemon = spawn_daemon(&home, &fixture, &env);
    wait_for_socket(&mut daemon, &socket);
    let start_result = run_firm_with_env(
        &home,
        &fixture.project_root,
        &[
            "--space",
            &fixture.execution_space_id,
            "--project",
            &fixture.project_id,
            "team-run",
            "start",
            "--id",
            &run_id,
        ],
        &env,
    );
    success(&start_result, "start fake Member");
    let store = HarnessStore::new(home.spaces_dir().join(&fixture.execution_space_id));
    let deadline = Instant::now() + Duration::from_secs(15);
    let message = loop {
        if let Some(message) = store
            .fabric_messages(&fixture.execution_space_id)
            .unwrap()
            .into_iter()
            .find(|m| m.body == "REVIEW_RESULT\nVerdict: Pass")
        {
            break Some(message);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // Drain before assertions so a failure cannot leave this fixture's runtime.
    stop_daemon(&home, &fixture, &mut daemon, &socket);
    let message = message.unwrap_or_else(|| {
        panic!(
            "Member CLI did not author: {}",
            std::fs::read_to_string(marker).unwrap_or_default()
        )
    });
    assert!(
        message.work_id.is_some(),
        "review CLI must retain exact Work link"
    );
    let session_id = message
        .sender_session_id
        .expect("managed CLI must freeze sender Session");
    let session = store
        .fabric_agent_sessions(&fixture.execution_space_id)
        .unwrap()
        .into_iter()
        .find(|s| s.id == session_id)
        .expect("canonical sender Session");
    assert_eq!(
        message.sender_agent_member_id.as_deref(),
        Some("verifier-worker")
    );
    assert_eq!(session.agent_member_id, "verifier-worker");
    assert_eq!(message.team_run_id.as_deref(), Some(run_id.as_str()));
}
