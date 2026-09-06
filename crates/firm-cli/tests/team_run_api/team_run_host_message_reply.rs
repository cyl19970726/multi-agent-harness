use super::*;

#[allow(clippy::too_many_arguments)]
fn host_reply_args<'a>(
    project_id: &'a str,
    run_id: &'a str,
    membership_id: &'a str,
    host_thread_id: &'a str,
    correlation_id: &'a str,
    causation_id: &'a str,
    idempotency_key: &'a str,
) -> Vec<&'a str> {
    vec![
        "--project",
        project_id,
        "team-run",
        "message",
        "reply",
        "--team-run-id",
        run_id,
        "--to-membership",
        membership_id,
        "--body",
        "external Host correlated reply",
        "--surface",
        "codex-app",
        "--thread-id",
        host_thread_id,
        "--correlation-id",
        correlation_id,
        "--causation-id",
        causation_id,
        "--idempotency-key",
        idempotency_key,
    ]
}

#[allow(clippy::too_many_arguments)]
fn run_host_reply(
    home: &TempHome,
    project_id: &str,
    run_id: &str,
    membership_id: &str,
    host_thread_id: &str,
    correlation_id: &str,
    causation_id: &str,
    idempotency_key: &str,
) -> std::process::Output {
    run_firm(
        home,
        home.base(),
        &host_reply_args(
            project_id,
            run_id,
            membership_id,
            host_thread_id,
            correlation_id,
            causation_id,
            idempotency_key,
        ),
    )
}

#[test]
fn external_host_cli_reply_preserves_exact_lineage_and_refuses_bad_lineage() {
    let home = TempHome::new("team-run-host-message-reply");
    let project_id = init_project(&home, "alpha");
    let space_id = current_space_id(&home);
    let serve = ServeHandle::spawn(&home, home.base(), &[]);
    let host_thread_id = "host-message-reply-thread";
    let (status, created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Exercise external Host CLI Message reply lineage",
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
        .expect("TeamRun id")
        .to_string();
    let member_run_id = created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("MemberRun id")
        .to_string();

    let (status, bootstrap) = serve.post_json(
        &format!("/v1/team-runs/{run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [member_run_id],
            "kind": "message",
            "body": "inbound message the external Host replies to"
        }),
    );
    assert_eq!(status, 200, "NodeDaemon bootstrap: {bootstrap}");

    // A second run of the same fixture Team: its message must never be
    // accepted as reply lineage in the first run.
    let (status, other_created) = serve.post_json(
        "/v1/team-runs",
        &serde_json::json!({
            "objective": "Foreign run for cross-run lineage refusal",
            "host_surface": "codex-app",
            "host_thread_id": "other-host-thread",
            "host_runtime_mode": "external_interactive",
            "members": [
                {"agent_member_id": "worker", "name": "worker", "role": "builder", "provider": "codex"}
            ]
        }),
    );
    assert_eq!(status, 200, "foreign TeamRun: {other_created}");
    let other_run_id = other_created["result"]["team_run"]["id"]
        .as_str()
        .expect("foreign TeamRun id")
        .to_string();
    let other_member_run_id = other_created["result"]["member_runs"][0]["id"]
        .as_str()
        .expect("foreign MemberRun id")
        .to_string();
    let (status, other_bootstrap) = serve.post_json(
        &format!("/v1/team-runs/{other_run_id}/messages"),
        &serde_json::json!({
            "sender_runtime_id": "host",
            "recipient_runtime_ids": [other_member_run_id],
            "kind": "message",
            "body": "foreign run message"
        }),
    );
    assert_eq!(status, 200, "foreign bootstrap: {other_bootstrap}");

    let store = HarnessStore::new(home.spaces_dir().join(&space_id));
    let membership = store
        .fabric_team_memberships(&space_id)
        .expect("TeamMemberships")
        .into_iter()
        .find(|membership| membership.agent_member_id == "worker")
        .expect("worker TeamMembership");
    let messages = store
        .fabric_messages(&space_id)
        .expect("canonical Messages");
    let inbound = messages
        .iter()
        .find(|message| {
            message.team_run_id.as_deref() == Some(run_id.as_str())
                && message.body == "inbound message the external Host replies to"
        })
        .expect("inbound canonical Message")
        .clone();
    let foreign = messages
        .iter()
        .find(|message| message.team_run_id.as_deref() == Some(other_run_id.as_str()))
        .expect("foreign canonical Message")
        .clone();

    let message_count = || {
        store
            .fabric_messages(&space_id)
            .expect("canonical Messages")
            .len()
    };

    // Wrong external Host binding refuses before any authoring.
    let count_before = message_count();
    let bad_binding = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "message",
            "reply",
            "--team-run-id",
            &run_id,
            "--to-membership",
            &membership.id,
            "--body",
            "external Host correlated reply",
            "--surface",
            "codex-app",
            "--thread-id",
            "wrong-native-thread",
            "--correlation-id",
            &inbound.correlation_id,
            "--causation-id",
            &inbound.id,
        ],
    );
    assert!(
        !bad_binding.status.success(),
        "wrong binding unexpectedly replied: {bad_binding:?}"
    );
    assert!(
        String::from_utf8_lossy(&bad_binding.stderr).contains("UNAUTHORIZED_ACTOR"),
        "wrong-binding error: {bad_binding:?}"
    );
    assert_eq!(
        message_count(),
        count_before,
        "binding rejection must happen before canonical Message authoring"
    );

    // Unknown causation refuses.
    let unknown = run_host_reply(
        &home,
        &project_id,
        &run_id,
        &membership.id,
        host_thread_id,
        &inbound.correlation_id,
        "message:role-message:sha256:unknown",
        "host-cli-reply-unknown",
    );
    assert!(
        !unknown.status.success(),
        "unknown causation unexpectedly replied: {unknown:?}"
    );
    assert!(
        String::from_utf8_lossy(&unknown.stderr)
            .contains("does not identify a message in team run"),
        "unknown-causation error: {unknown:?}"
    );

    // Cross-run causation refuses, even though the message id exists.
    let cross_run = run_host_reply(
        &home,
        &project_id,
        &run_id,
        &membership.id,
        host_thread_id,
        &foreign.correlation_id,
        &foreign.id,
        "host-cli-reply-cross-run",
    );
    assert!(
        !cross_run.status.success(),
        "cross-run causation unexpectedly replied: {cross_run:?}"
    );
    assert!(
        String::from_utf8_lossy(&cross_run.stderr)
            .contains("does not identify a message in team run"),
        "cross-run error: {cross_run:?}"
    );

    // Mismatched correlation refuses: the causation names a different
    // conversation than the supplied correlation.
    let mismatched = run_host_reply(
        &home,
        &project_id,
        &run_id,
        &membership.id,
        host_thread_id,
        &foreign.correlation_id,
        &inbound.id,
        "host-cli-reply-mismatched",
    );
    assert!(
        !mismatched.status.success(),
        "mismatched correlation unexpectedly replied: {mismatched:?}"
    );
    assert!(
        String::from_utf8_lossy(&mismatched.stderr).contains("has correlation_id"),
        "mismatched-correlation error: {mismatched:?}"
    );
    assert_eq!(
        message_count(),
        count_before,
        "lineage rejections must not author canonical Messages"
    );

    // Positive: exact incoming lineage persists in the stored canonical Message.
    let reply = || {
        run_host_reply(
            &home,
            &project_id,
            &run_id,
            &membership.id,
            host_thread_id,
            &inbound.correlation_id,
            &inbound.id,
            "host-cli-reply-idempotency",
        )
    };
    let first = reply();
    assert!(first.status.success(), "Host CLI reply failed: {first:?}");
    let first_json: serde_json::Value =
        serde_json::from_slice(&first.stdout).expect("first Host CLI reply JSON");
    assert_eq!(first_json["replayed"], false);
    let reply_message_id = first_json["message_id"]
        .as_str()
        .expect("canonical reply message id")
        .to_string();
    let canonical = store
        .fabric_messages(&space_id)
        .expect("canonical Messages")
        .into_iter()
        .find(|message| message.id == reply_message_id)
        .expect("Host-authored canonical reply");
    assert_eq!(canonical.sender_actor_ref.id, FIXTURE_HOST_ID);
    assert_eq!(
        canonical.kind,
        harness_core::agentfirm_api::MessageKind::Reply
    );
    assert_eq!(canonical.correlation_id, inbound.correlation_id);
    assert_eq!(canonical.causation_id.as_deref(), Some(inbound.id.as_str()));
    assert_eq!(canonical.team_run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(canonical.body, "external Host correlated reply");

    // Idempotent retry replays the same canonical Message.
    let second = reply();
    assert!(
        second.status.success(),
        "Host CLI reply replay failed: {second:?}"
    );
    let second_json: serde_json::Value =
        serde_json::from_slice(&second.stdout).expect("second Host CLI reply JSON");
    assert_eq!(second_json["message_id"], reply_message_id);
    assert_eq!(second_json["replayed"], true);

    // The same idempotency key with different lineage semantics conflicts.
    let conflicting = run_host_reply(
        &home,
        &project_id,
        &run_id,
        &membership.id,
        host_thread_id,
        &inbound.correlation_id,
        &reply_message_id,
        "host-cli-reply-idempotency",
    );
    assert!(
        !conflicting.status.success(),
        "idempotency-key reuse with different lineage unexpectedly succeeded: {conflicting:?}"
    );
    assert!(
        String::from_utf8_lossy(&conflicting.stderr).contains("RUNTIME_COMMAND_REJECTED"),
        "conflicting-reuse error: {conflicting:?}"
    );
}
