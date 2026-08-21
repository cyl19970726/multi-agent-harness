use super::*;

#[test]
fn canonical_message_fabric_drives_lineage_status_and_member_detail_without_legacy_rows() {
    let (store, root) = temp_store("canonical-current-message-projections");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-current-message-projections",
            std::process::id(),
            "test://canonical-current-message-projections",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire canonical test Supervisor");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let team = store
        .latest_teams()
        .expect("test Team")
        .remove(&created.team_run.agent_team_id)
        .expect("Team exists");
    let member = &created.member_runs[0];
    let legacy_path = store.root().join("team_messages.jsonl");
    std::fs::write(&legacy_path, b"{malformed legacy archive")
        .expect("seed unreadable Legacy archive sentinel");
    let legacy_before = std::fs::read(&legacy_path).expect("legacy bytes before");

    author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "canonical-host-request",
        &team.host_agent_id,
        &member.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Please report status",
        "canonical-conversation",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "canonical-member-reply",
        &member.agent_member_id,
        &team.host_agent_id,
        harness_core::agentfirm_api::MessageKind::Reply,
        "Status complete",
        "canonical-conversation",
        Some("canonical-host-request"),
        harness_core::agentfirm_api::ResponseIntent::Informational,
    );
    let foreign_space = "foreign-colliding-message-space";
    ensure_foreign_test_message_fabric(&store, &created, &lease, foreign_space);
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        foreign_space,
        "foreign-host-request",
        &team.host_agent_id,
        &member.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Foreign request must remain isolated",
        "foreign-conversation",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        foreign_space,
        "foreign-member-reply",
        &member.agent_member_id,
        &team.host_agent_id,
        harness_core::agentfirm_api::MessageKind::Reply,
        "Foreign reply must remain isolated",
        "foreign-conversation",
        Some("foreign-host-request"),
        harness_core::agentfirm_api::ResponseIntent::Informational,
    );

    let lineage = resolve_team_message_lineage(
        &store,
        &created.team_run.id,
        &ProviderDispatchIntent::Message,
        None,
        Some("canonical-member-reply".into()),
    )
    .expect("canonical reply supplies lineage");
    assert_eq!(lineage.0, "canonical-conversation");
    assert_eq!(lineage.1.as_deref(), Some("canonical-member-reply"));
    assert!(resolve_team_message_lineage(
        &store,
        &created.team_run.id,
        &ProviderDispatchIntent::Message,
        None,
        Some("foreign-member-reply".into()),
    )
    .expect_err("foreign-space message cannot establish current-run lineage")
    .to_string()
    .contains("does not identify a message"));
    let current = canonical_team_messages_for_run(&store, &created.team_run.id)
        .expect("exact-space canonical projection");
    assert_eq!(current.len(), 2);
    assert!(current
        .iter()
        .all(|message| !message.id.starts_with("foreign-")));
    let member_inbox = team_run_inbox(&store, &created.team_run.id, &member.id, true)
        .expect("exact-space Member inbox");
    assert_eq!(
        member_inbox
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["canonical-host-request"]
    );
    assert_eq!(
        team_run_unacknowledged_message_count(&store, &created.team_run.id)
            .expect("canonical status count"),
        1,
        "member-to-Host canonical delivery is current unacknowledged status"
    );
    let detail = member_run_detail_json(&store, &member.id).expect("canonical member detail");
    assert_eq!(detail["mailbox"]["inbox"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        detail["mailbox"]["outbox"].as_array().map(Vec::len),
        Some(1),
        "canonical sender identity must project back to its MemberRun"
    );
    let host_inbox =
        team_run_inbox(&store, &created.team_run.id, "host", true).expect("exact-space Host inbox");
    assert_eq!(
        host_inbox
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["canonical-member-reply"]
    );
    let dashboard = dashboard_snapshot(&store).expect("exact-space Dashboard snapshot");
    assert_eq!(
        dashboard["team_messages"]
            .as_array()
            .expect("current Team message projection")
            .iter()
            .filter(|message| {
                message["team_run_id"].as_str() == Some(created.team_run.id.as_str())
            })
            .map(|message| message["id"].as_str().expect("message id"))
            .collect::<Vec<_>>(),
        vec!["canonical-host-request", "canonical-member-reply"],
        "Dashboard current messages must ignore a foreign-space run-id collision"
    );
    assert_eq!(
        std::fs::read(&legacy_path).expect("legacy bytes after"),
        legacy_before,
        "current lineage/status/detail must neither read nor write the Legacy archive"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
