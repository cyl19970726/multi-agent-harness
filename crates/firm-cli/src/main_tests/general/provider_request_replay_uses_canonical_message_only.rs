use super::*;

#[test]
fn provider_request_replay_uses_canonical_message_only() {
    let (store, root) = temp_store("canonical-provider-request-replay");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "canonical-provider-request-replay",
            std::process::id(),
            "test://canonical-provider-request-replay",
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
    let mut member = created.member_runs[0].clone();
    let expected = member.clone();
    member.native_session = Some(NativeSessionRef {
        provider: member.provider.clone(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "canonical-replay-session".into(),
        native_locator_kind: "codex_thread".into(),
        provider_version: None,
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: None,
        parent_native_session_id: None,
    });
    store
        .compare_and_append_member_run(&expected, &member)
        .expect("bind provider-native session");
    let options = vec![ProviderInteractionMessageOption {
        id: "answer::0".into(),
        label: "Proceed".into(),
        intent: Some("answer".into()),
    }];
    let body = ProviderInteractionRequestBody {
        interaction_type: ProviderInteractionType::Question,
        prompt: "Continue?".into(),
        options: options.clone(),
        provider: member.provider.clone(),
        provider_request_id: "native-request-7".into(),
        method: "item/tool/requestUserInput".into(),
        session: "canonical-replay-session".into(),
        member: member.id.clone(),
        generation: member.runtime_generation,
    };
    let canonical_body = body.to_canonical_json().expect("canonical request body");
    let correlation = body.correlation_id();
    let legacy_path = store.root().join("team_messages.jsonl");
    std::fs::write(&legacy_path, b"{malformed legacy archive")
        .expect("seed unreadable Legacy archive sentinel");
    let legacy_before = std::fs::read(&legacy_path).expect("legacy bytes before");
    let authored = author_test_canonical_message(
        &store,
        &created,
        &lease,
        &lease.execution_space_id,
        "canonical-provider-request",
        &member.agent_member_id,
        &team.host_agent_id,
        harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest,
        &canonical_body,
        &correlation,
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    let foreign_space = "foreign-colliding-provider-replay-space";
    ensure_foreign_test_message_fabric(&store, &created, &lease, foreign_space);
    author_test_canonical_message(
        &store,
        &created,
        &lease,
        foreign_space,
        "foreign-provider-request",
        &member.agent_member_id,
        &team.host_agent_id,
        harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest,
        "{\"foreign\":true}",
        &correlation,
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let (replayed, created_again) = provider_interaction_request_message(
        &ledger,
        &member,
        "native-request-7",
        "item/tool/requestUserInput",
        ProviderInteractionType::Question,
        "Continue?".into(),
        options,
    )
    .expect("exact canonical provider request replay converges");
    assert!(!created_again);
    assert_eq!(replayed.id, authored.id);
    assert_eq!(
        canonical_team_messages_for_run(&store, &created.team_run.id)
            .expect("canonical messages")
            .iter()
            .filter(|message| {
                message.kind == ProviderDispatchIntent::ProviderInteractionRequest
            })
            .count(),
        1
    );
    assert_eq!(
        std::fs::read(&legacy_path).expect("legacy bytes after"),
        legacy_before,
        "provider request replay must neither read nor write the Legacy archive"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
