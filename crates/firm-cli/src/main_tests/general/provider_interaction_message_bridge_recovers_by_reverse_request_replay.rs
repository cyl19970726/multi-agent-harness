use super::*;

#[cfg(any())] // Historical reverse-request bridge over retired Team message projection.
fn provider_interaction_message_bridge_recovers_by_reverse_request_replay() {
    let (store, root) = temp_store("provider-interaction-message-bridge");
    let created = create_two_member_team_run(&store);
    let initial_run = created.team_run.clone();
    let mut active_run = initial_run.clone();
    active_run.status = TeamRunStatus::Running;
    active_run.updated_at = "unix-ms:provider-bridge-running".into();
    store
        .compare_and_append_team_run_lifecycle(&initial_run, &active_run)
        .expect("seed running TeamRun");
    let initial = created.member_runs[0].clone();
    let mut running = initial.clone();
    running.status = MemberRunStatus::Running;
    running.native_session = Some(NativeSessionRef {
        provider: running.provider.clone(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "codex-session-bridge".into(),
        native_locator_kind: "codex_thread".into(),
        provider_version: None,
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: None,
        parent_native_session_id: None,
    });
    store
        .compare_and_append_member_run(&initial, &running)
        .expect("seed native running member");
    let ledger = TeamRunLedger::without_supervisor(&store, &created.team_run.id);
    let options = vec![ProviderInteractionMessageOption {
        id: "answer::0".into(),
        label: "Proceed".into(),
        intent: Some("answer".into()),
    }];
    let (request, created_now) = provider_interaction_request_message(
        &ledger,
        &running,
        "reverse-7",
        "item/tool/requestUserInput",
        ProviderInteractionType::Question,
        "Continue?".into(),
        options.clone(),
    )
    .expect("create message request");
    assert!(created_now);
    let (replayed, created_again) = provider_interaction_request_message(
        &ledger,
        &running,
        "reverse-7",
        "item/tool/requestUserInput",
        ProviderInteractionType::Question,
        "Continue?".into(),
        options,
    )
    .expect("exact provider replay converges");
    assert!(!created_again);
    assert_eq!(replayed.id, request.id);
    let mut replacement_runtime = running.clone();
    replacement_runtime.runtime_generation += 1;
    let stale_replay = provider_interaction_request_message(
        &ledger,
        &replacement_runtime,
        "reverse-7",
        "item/tool/requestUserInput",
        ProviderInteractionType::Question,
        "Continue?".into(),
        vec![ProviderInteractionMessageOption {
            id: "answer::0".into(),
            label: "Proceed".into(),
            intent: Some("answer".into()),
        }],
    )
    .expect_err(
        "a new ProviderRuntimeProjection generation cannot consume the old reverse request",
    );
    assert!(stale_replay
        .to_string()
        .contains("replayed with different semantics"));
    let response = answer_provider_message_value(
        &store,
        &created.team_run.id,
        &request.id,
        &serde_json::json!({
            "resolved_by": "host",
            "option_id": "answer::0"
        }),
    )
    .expect("resolve request message");
    assert_eq!(
        response["kind"].as_str(),
        Some("provider_interaction_response")
    );
    let exact_retry = answer_provider_message_value(
        &store,
        &created.team_run.id,
        &request.id,
        &serde_json::json!({
            "resolved_by": "host",
            "option_id": "answer::0"
        }),
    )
    .expect("exact response retry converges");
    assert_eq!(exact_retry["id"], response["id"]);
    let conflict = answer_provider_message_value(
        &store,
        &created.team_run.id,
        &request.id,
        &serde_json::json!({
            "resolved_by": "host",
            "response_text": "different"
        }),
    )
    .expect_err("different second response conflicts");
    assert!(conflict
        .to_string()
        .contains("PROVIDER_INTERACTION_RESPONSE_CONFLICT"));

    // A replacement Supervisor/provider callback does not fabricate an
    // in-memory responder. It recovers only when the provider replays the
    // same native reverse request; that replay reuses the durable request
    // and consumes the already queued answer under the new lease.
    let now = current_unix_ms_u64();
    let old_lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-before-restart",
            std::process::id(),
            "test://before-restart",
            now,
            30_000,
        )
        .expect("old Supervisor lease");
    store
        .release_team_supervisor_lease(
            &created.team_run.id,
            &old_lease.supervisor_id,
            old_lease.generation,
            now + 1,
        )
        .expect("release old Supervisor");
    let replacement = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-after-restart",
            std::process::id(),
            "test://after-restart",
            now + 2,
            30_000,
        )
        .expect("replacement Supervisor lease");
    assert!(replacement.generation > old_lease.generation);
    let restarted = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &replacement.supervisor_id,
        replacement.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let (request_after_restart, duplicate_after_restart) = provider_interaction_request_message(
        &restarted,
        &running,
        "reverse-7",
        "item/tool/requestUserInput",
        ProviderInteractionType::Question,
        "Continue?".into(),
        vec![ProviderInteractionMessageOption {
            id: "answer::0".into(),
            label: "Proceed".into(),
            intent: Some("answer".into()),
        }],
    )
    .expect("replayed reverse request after restart");
    assert!(!duplicate_after_restart);
    assert_eq!(request_after_restart.id, request.id);
    let (_, claimed_after_restart) =
        wait_for_provider_interaction_response(&restarted, &running, &request_after_restart)
            .expect("replacement callback reads durable answer")
            .expect("durable answer exists");
    let claimed_after_restart = claimed_after_restart.expect("replacement claims Inject");
    restarted
        .complete_provider_interaction_response(
            &claimed_after_restart,
            &running.id,
            "test-provider-receipt-after-restart",
        )
        .expect("replacement completes after native write");

    let messages =
        canonical_team_messages_for_run(&store, &created.team_run.id).expect("canonical messages");
    let latest_request = messages
        .iter()
        .find(|message| message.id == request.id)
        .expect("latest request");
    assert!(latest_request.deliveries.iter().any(|delivery| {
        delivery.member_id == "host" && delivery.status == TeamDeliveryStatus::Acknowledged
    }));
    let response_message = messages
        .iter()
        .find(|message| {
            message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                && message.causation_id.as_deref() == Some(request.id.as_str())
        })
        .expect("response message");
    assert!(response_message.deliveries.iter().any(|delivery| {
        delivery.member_id == running.id
            && delivery.policy == TeamDeliveryPolicy::Inject
            && delivery.status == TeamDeliveryStatus::Delivered
            && delivery.provider_receipt_id.as_deref()
                == Some("test-provider-receipt-after-restart")
    }));
    assert_eq!(
        messages
            .iter()
            .filter(|message| {
                message.kind == ProviderDispatchIntent::ProviderInteractionRequest
            })
            .count(),
        1,
        "provider replay must reuse the request message"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
