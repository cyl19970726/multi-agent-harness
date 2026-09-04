use super::*;
use harness_core::CurrentWorkDraft;

fn create_unrelated_team_run(store: &HarnessStore, index: usize) {
    let worker_id = format!("unrelated-worker-{index}");
    create_team_run(
        store,
        None,
        None,
        None,
        &format!("Unrelated TeamRun {index}"),
        None,
        "test",
        None,
        HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            TeamMemberSpec {
                agent_member_id: worker_id,
                name: format!("Unrelated Worker {index}"),
                role: "worker".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
            TeamMemberSpec {
                agent_member_id: "host".into(),
                name: format!("Unrelated Host {index}"),
                role: "host".into(),
                provider: "codex".into(),
                execution_mode: Some("codex_app_server".into()),
                model: None,
                effort: None,
                service_tier: None,
                provider_cwd_hint: None,
                owned_paths: Vec::new(),
                resume_native_session_id: None,
                initial_work: None,
            },
        ],
    )
    .expect("create unrelated TeamRun");
}

#[test]
fn scoped_dashboard_snapshot_matches_filtered_global_with_large_unrelated_history() {
    let (store, root) = temp_store("scoped-dashboard-snapshot-equivalence");
    let selected = create_two_member_team_run(&store);
    for index in 0..3 {
        create_unrelated_team_run(&store, index);
    }
    for index in 0..800 {
        store
            .append_message(&RegistryMessage {
                id: format!("unrelated-registry-message-{index}"),
                task_id: None,
                from_agent_id: "unrelated-sender".into(),
                to_agent_id: Some("unrelated-recipient".into()),
                channel: None,
                kind: RegistryMessageIntent::Message,
                delivery_status: RegistryDeliveryStatus::Acknowledged,
                content: "x".repeat(8 * 1024),
                evidence_ids: Vec::new(),
                created_at: format!("unix-ms:{index}"),
                delivery: None,
                sender_kind: SenderKind::Agent,
            })
            .expect("append large unrelated registry history");
    }

    let lease = store
        .acquire_test_supervisor_lease(
            &selected.team_run.id,
            "scoped-dashboard-snapshot-equivalence",
            std::process::id(),
            "test://scoped-dashboard-snapshot-equivalence",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire selected TeamRun Supervisor");
    ensure_test_runtime_fabric(&store, &selected, &lease);
    let owner = &selected.member_runs[0];
    let work = store
        .insert_work(
            {
                let mut draft = CurrentWorkDraft::new(
                    "scoped-dashboard-work".into(),
                    selected.team_run.id.clone(),
                    selected.team_run.agent_team_id.clone(),
                    "Resolve one scoped snapshot".into(),
                    "Keep the selected Work fabric".into(),
                    "Scoped and filtered-global projections match".into(),
                    WorkClaimMode::HostAssign,
                    WorkPriority::Normal,
                    compatibility_team_actor("host", "test"),
                    "unix-ms:900".into(),
                );
                draft.eligible_member_ids = vec![owner.agent_member_id.clone()];
                draft.into_work()
            },
            WorkCommandContext {
                event_id: "scoped-dashboard-work-created".into(),
                performed_by_actor: compatibility_team_actor("host", "test"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "scoped-dashboard-work-created".into(),
                created_at: "unix-ms:900".into(),
                duplicate_ok: false,
            },
        )
        .expect("create selected Work");
    let assigned =
        assign_test_work_to_member(&store, &lease.execution_space_id, &selected, owner, &work);
    bind_test_responsible_work_execution(&store, &lease, owner, &assigned);
    let team = store
        .latest_teams()
        .expect("selected Team")
        .remove(&selected.team_run.agent_team_id)
        .expect("selected Team exists");
    let pending_message = author_test_canonical_message(
        &store,
        &selected,
        &lease,
        &lease.execution_space_id,
        "scoped-dashboard-message",
        &team.host_agent_id,
        &owner.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Scoped snapshot message",
        "scoped-dashboard-correlation",
        None,
        harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
    );
    let acknowledged_message = author_test_canonical_message(
        &store,
        &selected,
        &lease,
        &lease.execution_space_id,
        "scoped-dashboard-acknowledged-message",
        &team.host_agent_id,
        &owner.agent_member_id,
        harness_core::agentfirm_api::MessageKind::Message,
        "Scoped snapshot acknowledged message",
        "scoped-dashboard-acknowledged-correlation",
        None,
        harness_core::agentfirm_api::ResponseIntent::Informational,
    );
    let delivery = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("selected canonical deliveries")
        .into_iter()
        .find(|delivery| delivery.message_id == acknowledged_message.id)
        .expect("acknowledged message delivery");
    store
        .claim_message_for_provider(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "test.scoped_snapshot.claim",
                "scoped-dashboard-acknowledged-message:claim".into(),
                0,
            ),
            &delivery.id,
            &selected.team_run.execution_node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            "scoped-dashboard-claim",
            harness_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            "unix-ms:901",
        )
        .expect("claim acknowledged message");
    let claimed = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("claimed canonical deliveries")
        .into_iter()
        .find(|delivery| delivery.message_id == acknowledged_message.id)
        .expect("claimed message delivery");
    store
        .record_message_provider_receipt(
            &canonical_delivery_context(
                &lease.execution_space_id,
                &lease.node_daemon_id,
                "test.scoped_snapshot.receipt",
                "scoped-dashboard-acknowledged-message:receipt".into(),
                0,
            ),
            &claimed.id,
            &selected.team_run.execution_node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            claimed.claim_id.as_deref().expect("delivery claim id"),
            "scoped-dashboard-provider-receipt",
            "unix-ms:902",
        )
        .expect("record acknowledged message provider receipt");
    let received = store
        .fabric_message_deliveries(&lease.execution_space_id)
        .expect("provider-received canonical deliveries")
        .into_iter()
        .find(|delivery| delivery.message_id == acknowledged_message.id)
        .expect("provider-received message delivery");
    store
        .acknowledge_message_delivery(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: lease.execution_space_id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: owner.agent_member_id.clone(),
                },
                authority_actor: None,
                command_name: "test.scoped_snapshot.acknowledge".into(),
                idempotency_key: "scoped-dashboard-acknowledged-message:ack".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            &received.id,
            "unix-ms:903",
        )
        .expect("acknowledge selected message");
    store
        .append_member_action(&MemberAction {
            id: "scoped-dashboard-pending-action".into(),
            seq: 1,
            team_run_id: selected.team_run.id.clone(),
            member_run_id: owner.id.clone(),
            task_id: Some(assigned.id.clone()),
            provider_call_id: Some("scoped-dashboard-interaction".into()),
            action_type: "interaction_requested".into(),
            status: MemberActionStatus::Started,
            provider_status: Some("input_required".into()),
            semantic_status: None,
            title: "Operator input required".into(),
            summary: "Selected run has one pending interaction".into(),
            evidence_refs: Vec::new(),
            started_at: "unix-ms:904".into(),
            completed_at: None,
        })
        .expect("append selected pending interaction action");

    let global_started = std::time::Instant::now();
    let mut filtered_global = dashboard_team_run_snapshot_via_global(&store, &selected.team_run.id)
        .expect("filter the global Dashboard snapshot");
    let global_elapsed = global_started.elapsed();
    let global_bytes = serde_json::to_vec(&dashboard_snapshot(&store).unwrap())
        .unwrap()
        .len();

    let scoped_started = std::time::Instant::now();
    let mut scoped = dashboard_team_run_snapshot(&store, &selected.team_run.id)
        .expect("resolve the selected TeamRun directly");
    let scoped_elapsed = scoped_started.elapsed();
    let scoped_bytes = serde_json::to_vec(&scoped).unwrap().len();
    filtered_global["generated_at"] = serde_json::Value::Null;
    scoped["generated_at"] = serde_json::Value::Null;

    eprintln!(
        "SCOPED_SNAPSHOT_METRICS global_ms={} scoped_ms={} global_json_bytes={} scoped_json_bytes={} unrelated_registry_rows=800 unrelated_team_runs=3",
        global_elapsed.as_millis(),
        scoped_elapsed.as_millis(),
        global_bytes,
        scoped_bytes,
    );
    assert_eq!(scoped, filtered_global);
    assert!(
        global_bytes > scoped_bytes * 10,
        "large unrelated history must make the global JSON materially larger"
    );
    assert_eq!(scoped["team_runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(scoped["works"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        scoped["work_execution_bindings"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        scoped["canonical_message_deliveries"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(scoped["member_actions"].as_array().map(Vec::len), Some(1));
    assert!(scoped["canonical_messages"]
        .as_array()
        .expect("canonical messages")
        .iter()
        .any(|message| message["id"] == pending_message.id));
    assert_eq!(
        scoped["canonical_messages"].as_array().map(Vec::len),
        Some(2)
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
