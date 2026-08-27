use super::*;

#[test]
fn team_trash_restore_preserves_work_message_membership_and_native_session_records() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-trash", "run-trash");
    store
        .create_agent_session(
            &service_context("session.create", "session-trash-host", 0),
            session("session-trash-host", "fixture-host"),
        )
        .unwrap();
    store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "native-trash-host", 1),
            "session-trash-host",
            1,
            settled_native_session("native-trash-host"),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context(
                "fixture-host",
                "member_run.create",
                "member-run-trash-host",
                0,
            ),
            MemberRun {
                id: "member-run-trash-host".into(),
                agent_member_id: "fixture-host".into(),
                team_run_id: "run-trash".into(),
                role_snapshot: "host".into(),
                provider_profile_snapshot: None,
                requested_controls: serde_json::json!({}),
                effective_controls: serde_json::json!({}),
                coordination_status: MemberCoordinationStatus::Active,
                runtime_status: MemberRuntimeStatus::Idle,
                runtime_generation: 1,
                workspace_binding_id: None,
                native_session: None,
                version: 1,
                started_at: "t-member-run".into(),
                last_event_at: None,
                finished_at: None,
            },
        )
        .unwrap();
    insert_runtime_work(&store, "work-trash", "team-trash", "run-trash");
    let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
        kind: MessageRecipientKind::Team,
        id: "team-trash".into(),
    }];
    let mut message = Message {
        id: "message-trash".into(),
        source_execution_space_id: "space-test".into(),
        source_node_id: "11111111-1111-4111-8111-111111111111".into(),
        source_node_daemon_id: "daemon-1".into(),
        source_authority_generation: 1,
        sender_actor_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "fixture-host".into(),
        },
        sender_agent_member_id: Some("fixture-host".into()),
        sender_session_id: Some("session-trash-host".into()),
        address_kind: firm_core::agentfirm_api::MessageAddressKind::TeamChannel,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("team-trash".into()),
        team_run_id: Some("run-trash".into()),
        work_id: Some("work-trash".into()),
        collaboration_scope: None,
        kind: firm_core::agentfirm_api::MessageKind::Message,
        body: "retain this Team record".into(),
        body_digest: format!("sha256:{:x}", Sha256::digest(b"retain this Team record")),
        correlation_id: "trash-correlation".into(),
        causation_id: None,
        response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "message-trash".into(),
        created_at: "t-message".into(),
    };
    message.content_fingerprint = message_content_fingerprint(&message);
    store
        .author_message(
            &service_context("message.author", "message-trash", 0),
            message,
        )
        .unwrap();
    let works_before = store.latest_works().unwrap();
    let messages_before = store.fabric_messages("space-test").unwrap();
    let deliveries_before = store.fabric_message_deliveries("space-test").unwrap();
    let sessions_before = store.fabric_agent_sessions("space-test").unwrap();
    let membership_before = store
        .team_host_membership("space-test", "team-trash", true)
        .unwrap();
    let trashed = store
        .transition_agent_team(
            &context("fixture-host", "agent_team.trash", "team-trash", 1),
            "team-trash",
            AgentTeamStatus::Trashed,
            "t-trash",
        )
        .unwrap();
    assert_eq!(
        trashed.projection.node_id,
        "11111111-1111-4111-8111-111111111111"
    );
    let restored = store
        .transition_agent_team(
            &context("fixture-host", "agent_team.restore", "team-restore", 2),
            "team-trash",
            AgentTeamStatus::Inactive,
            "t-restore",
        )
        .unwrap();
    assert_eq!(restored.projection.status, AgentTeamStatus::Inactive);
    assert_eq!(store.latest_works().unwrap(), works_before);
    assert_eq!(
        store.fabric_messages("space-test").unwrap(),
        messages_before
    );
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries_before
    );
    assert_eq!(
        store.fabric_agent_sessions("space-test").unwrap(),
        sessions_before
    );
    let retained_membership = store
        .fabric_team_memberships("space-test")
        .unwrap()
        .into_iter()
        .find(|membership| membership.id == membership_before.id)
        .unwrap();
    assert_eq!(retained_membership.state, TeamMembershipStatus::Inactive);
    store
        .activate_team_membership(
            &context(
                "fixture-host",
                "membership.activate",
                "membership-trash-activate",
                retained_membership.revision,
            ),
            &retained_membership.id,
            "t-membership-active",
        )
        .unwrap();
    store
        .transition_agent_team(
            &context(
                "fixture-host",
                "agent_team.activate",
                "team-trash-active",
                3,
            ),
            "team-trash",
            AgentTeamStatus::Active,
            "t-active",
        )
        .unwrap();
    assert_eq!(
        store
            .team_host_membership("space-test", "team-trash", true)
            .unwrap()
            .id,
        membership_before.id
    );
    fs::remove_dir_all(root).unwrap();
}
