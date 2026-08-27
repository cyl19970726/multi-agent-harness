use super::*;

#[test]
fn host_inbox_shows_work_attention_after_submit() {
    let (store, root) = temp_store("host-attn-inbox");
    let created = create_two_member_team_run(&store);
    let member = &created.member_runs[0];
    let current = latest_team_run(&store, &created.team_run.id).expect("current run");
    let mut bound = current.clone();
    bound.host_surface = "codex-app".into();
    bound.host_thread_id = Some("codex-thread-a".into());
    bound.updated_at = "unix-ms:host-bound".into();
    store
        .compare_and_append_team_run(&current, &bound)
        .expect("bind native Host");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-host-attention-inbox",
            std::process::id(),
            "test://host-attention-inbox",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);

    // Create a Work assigned to member_a
    let work_ctx = WorkCommandContext {
        event_id: generated_id("work-event-insert"),
        performed_by_actor: compatibility_team_actor("host", "test"),
        authority_actor: None,
        causation_ref: None,
        idempotency_key: generated_id("work-command-insert"),
        created_at: "unix-ms:10".into(),
        duplicate_ok: false,
    };
    let work = harness_application::WorkApplication::new(&store)
        .create(harness_application::CreateWorkCommand {
            work_id: generated_id("work-attn"),
            team_run_id: bound.id.clone(),
            accountable_team_id: bound.agent_team_id.clone(),
            title: "Test attention flow".into(),
            context_markdown: "Context for attention test".into(),
            completion_criteria_markdown: "Attention appears in host-inbox".into(),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: work_ctx,
        })
        .expect("insert Work");
    let work =
        assign_test_work_to_member(&store, &lease.execution_space_id, &created, member, &work);
    bind_test_responsible_work_execution(&store, &lease, member, &work);

    // Start the work as the assigned member
    let start_ctx = WorkCommandContext {
        event_id: generated_id("work-event-start"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member.id.clone(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: generated_id("work-command-start"),
        created_at: "unix-ms:20".into(),
        duplicate_ok: false,
    };
    let work = store
        .start_work(&work.id, work.version, &member.id, start_ctx)
        .expect("start work");

    // Submit the work — this generates a HostAttention (WorkReviewRequested)
    let submit_ctx = WorkCommandContext {
        event_id: generated_id("work-event-submit"),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member.id.clone(),
            display_name: None,
            authn_source: Some("bound-runtime:test".into()),
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: generated_id("work-command-submit"),
        created_at: "unix-ms:30".into(),
        duplicate_ok: false,
    };
    let work = store
        .submit_work(
            &work.id,
            work.version,
            &member.id,
            "All tasks complete",
            vec!["https://example.com/artifact".into()],
            vec!["https://example.com/check".into()],
            submit_ctx,
        )
        .expect("submit work");

    // Verify attention appears in the bound host-inbox
    let exact = host_inbox_for_native_thread(&store, "codex-app", "codex-thread-a", false)
        .expect("Host inbox");
    assert_eq!(exact.len(), 1, "one team run should appear");
    let entry = &exact[0];
    assert_eq!(entry["team_run_id"], bound.id);

    let attentions = entry["attentions"]
        .as_array()
        .expect("attentions must be an array");
    assert!(
        !attentions.is_empty(),
        "submitting work creates a HostAttention"
    );
    let attn = attentions
        .iter()
        .find(|attention| attention["kind"] == "work_review_requested")
        .expect("review request attention");
    assert_eq!(attn["kind"].as_str().unwrap(), "work_review_requested");
    assert_eq!(attn["work_id"].as_str().unwrap(), work.id);
    assert_eq!(attn["status"].as_str().unwrap(), "actionable");
    assert!(
        attn["member_run_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "attention must reference the submitting member"
    );

    let binding_id = format!("binding-{}", work.id);
    store
        .release_work_execution_binding(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: lease.execution_space_id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                    id: member.agent_member_id.clone(),
                },
                authority_actor: None,
                command_name: "test.work_binding.release".into(),
                idempotency_key: format!("release-{binding_id}"),
                expected_version: 1,
                request_fingerprint: None,
            },
            &binding_id,
            "unix-ms:after-submit",
        )
        .expect("release submitted Work binding");
    let after_release = host_inbox_for_native_thread(&store, "codex-app", "codex-thread-a", false)
        .expect("Host inbox after binding release");
    let persisted = after_release[0]["attentions"]
        .as_array()
        .expect("persisted attentions")
        .iter()
        .find(|attention| attention["id"] == attn["id"])
        .expect("submitted attention persists after binding release");
    assert_eq!(
        persisted["member_run_id"], attn["member_run_id"],
        "immutable submitter evidence must not depend on a live binding"
    );

    // A different native thread must not see anything
    assert!(
        host_inbox_for_native_thread(&store, "codex-app", "another-thread", false)
            .expect("other thread inbox")
            .is_empty(),
        "different thread gets no entries"
    );

    // --all shows the attention too
    let all = host_inbox_for_native_thread(&store, "codex-app", "codex-thread-a", true)
        .expect("all inbox");
    assert_eq!(all.len(), 1);
    let all_attns = all[0]["attentions"]
        .as_array()
        .expect("--all must include attentions");
    assert!(!all_attns.is_empty());

    let _ = std::fs::remove_dir_all(root);
}
