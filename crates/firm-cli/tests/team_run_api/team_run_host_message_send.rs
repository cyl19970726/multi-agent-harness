use super::*;

#[test]
fn external_host_cli_send_uses_canonical_message_authoring_and_exact_binding() {
    let home = TempHome::new("team-run-host-message-send");
    let project_id = init_project(&home, "alpha");
    let space_id = current_space_id(&home);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let host_thread_id = "host-message-send-thread";
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise external Host CLI Message authoring",
            "host_surface": "codex-app",
            "host_thread_id": host_thread_id,
            "host_runtime_mode": "external_interactive",
            "members": [
                {"agent_member_id": "worker", "name": "worker", "role": "builder", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "TeamRun: {created}");
    let run_id = created["result"]["team_run"]["id"]
        .as_str()
        .expect("TeamRun id");
    let member_run_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("MemberRun id");

    let (status, bootstrap) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_run_id],
            "kind": "message",
            "body": "bootstrap canonical message fabric"
        }),
    );
    assert_eq!(status, 200, "NodeDaemon bootstrap: {bootstrap}");

    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    let membership = store
        .fabric_team_memberships(&space_id)
        .expect("TeamMemberships")
        .into_iter()
        .find(|membership| membership.agent_member_id == "worker")
        .expect("worker TeamMembership");
    let message_count_before = store
        .fabric_messages(&space_id)
        .expect("Messages before rejected send")
        .len();
    let bad = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "message",
            "send",
            "--team-run-id",
            run_id,
            "--to-membership",
            &membership.id,
            "--body",
            "Host CLI message",
            "--surface",
            "codex-app",
            "--thread-id",
            "wrong-native-thread",
        ],
    );
    assert!(
        !bad.status.success(),
        "wrong binding unexpectedly sent: {bad:?}"
    );
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("UNAUTHORIZED_ACTOR"),
        "wrong-binding error: {bad:?}"
    );
    assert_eq!(
        store
            .fabric_messages(&space_id)
            .expect("Messages after rejected send")
            .len(),
        message_count_before,
        "binding rejection must happen before canonical Message authoring"
    );

    let send = || {
        run_firm(
            &home,
            home.base(),
            &[
                "--project",
                &project_id,
                "team-run",
                "message",
                "send",
                "--team-run-id",
                run_id,
                "--to-membership",
                &membership.id,
                "--body",
                "Host CLI message",
                "--response-required",
                "--surface",
                "codex-app",
                "--thread-id",
                host_thread_id,
                "--idempotency-key",
                "host-cli-send-idempotency",
            ],
        )
    };
    let first = send();
    assert!(first.status.success(), "Host CLI send failed: {first:?}");
    println!(
        "sample Host CLI output: {}",
        String::from_utf8_lossy(&first.stdout).trim()
    );
    let first_json: serde_json::Value =
        serde_json::from_slice(&first.stdout).expect("first Host CLI send JSON");
    assert_eq!(first_json["replayed"], false);
    assert_eq!(first_json["delivery_ids"].as_array().map(Vec::len), Some(1));
    let message_id = first_json["message_id"]
        .as_str()
        .expect("canonical message id");
    let delivery_id = first_json["delivery_ids"][0]
        .as_str()
        .expect("canonical delivery id");

    let second = send();
    assert!(
        second.status.success(),
        "Host CLI replay failed: {second:?}"
    );
    let second_json: serde_json::Value =
        serde_json::from_slice(&second.stdout).expect("second Host CLI send JSON");
    assert_eq!(second_json["message_id"], message_id);
    assert_eq!(second_json["delivery_ids"][0], delivery_id);
    assert_eq!(second_json["replayed"], true);

    let canonical = store
        .fabric_messages(&space_id)
        .expect("canonical Messages")
        .into_iter()
        .find(|message| message.id == message_id)
        .expect("Host-authored canonical Message");
    assert_eq!(canonical.sender_actor_ref.id, FIXTURE_HOST_ID);
    assert_eq!(canonical.body, "Host CLI message");
    assert_eq!(
        canonical.response_intent,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired
    );
    let matching_deliveries = store
        .fabric_message_deliveries(&space_id)
        .expect("canonical deliveries")
        .into_iter()
        .filter(|delivery| delivery.message_id == message_id)
        .collect::<Vec<_>>();
    assert_eq!(matching_deliveries.len(), 1);
    assert_eq!(matching_deliveries[0].id, delivery_id);
    assert_eq!(
        matching_deliveries[0].recipient_agent_member_id.as_deref(),
        Some("worker")
    );
}
