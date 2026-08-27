use super::*;

#[test]
fn canonical_acceptance_rolls_up_delegation_in_the_same_operation() {
    let harness = TestStore::new("canonical-delegation-rollup");
    seed_active_team_work(&harness.store, "delegation-source", "source-rollup");
    let source = harness
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == "source-rollup")
        .expect("source Work");
    let target_run = seed_team(&harness.store, "delegation-target", &["target-worker"]);
    let target_runtime_id = "runtime-target-worker";
    create_member_and_run(
        &harness.store,
        &human("host"),
        &target_run.id,
        "target-worker",
        target_runtime_id,
        false,
    );
    append_legacy_projection(
        &harness.store,
        "member_runs.jsonl",
        &RuntimeMemberRun {
            id: target_runtime_id.into(),
            team_run_id: target_run.id.clone(),
            slot_id: None,
            agent_member_id: "target-worker".into(),
            name: "Target Worker".into(),
            role: "worker".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            zero_output_streak: 0,
            last_consumed_work_version: None,
            started_at: "t1".into(),
            last_event_at: None,
            finished_at: None,
        },
    );
    let host_actor = harness
        .store
        .exact_team_run_host_actor(&source.team_run_id)
        .expect("resolve exact source TeamRun Host");
    let (delegation, target) = harness
        .store
        .create_work_delegation_with_target_work(
            WorkDelegation {
                id: "delegation-rollup".into(),
                source_work_ref: WorkRef {
                    team_run_id: source.team_run_id.clone(),
                    work_id: source.id.clone(),
                },
                source_work_version: source.version,
                source_owner_member_id: source.owner_member_id.clone().expect("source owner"),
                created_by_member_run_id: None,
                target_agent_team_id: target_run.agent_team_id.clone(),
                target_work_ref: WorkRef {
                    team_run_id: String::new(),
                    work_id: String::new(),
                },
                delegated_by_actor: host_actor.clone(),
                state: WorkDelegationState::Active,
                resolution_summary: None,
                blocker_reason: None,
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            Work {
                id: "target-rollup".into(),
                team_run_id: target_run.id.clone(),
                accountable_team_id: None,
                assignee_membership_id: None,
                created_by_member_id: None,
                legacy_containment_ref: None,
                title: "Delegated target".into(),
                context_markdown: "execute delegated target".into(),
                completion_criteria_markdown: "exact candidate accepted".into(),
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::HostAssign,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: host_actor.clone(),
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: "delegation-create".into(),
                performed_by_actor: host_actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "delegation-create".into(),
                created_at: "t2".into(),
                duplicate_ok: false,
            },
        )
        .expect("atomically create Delegation and target Work");
    let target_host = harness
        .store
        .exact_team_run_host_actor(&target_run.id)
        .expect("resolve exact target TeamRun Host");
    let target = harness
        .store
        .assign_work_to_membership(
            &target.id,
            target.version,
            "membership-team-delegation-target-target-worker",
            SPACE,
            WorkCommandContext {
                event_id: "delegation-target-assign".into(),
                performed_by_actor: target_host,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "delegation-target-assign".into(),
                created_at: "t2.1".into(),
                duplicate_ok: false,
            },
        )
        .expect("assign delegated target through stable TeamMembership responsibility");
    let target_session = AgentSession {
        id: "session-runtime-target-worker".into(),
        agent_member_id: "target-worker".into(),
        node_id: NODE.into(),
        execution_space_id: SPACE.into(),
        node_daemon_id: "daemon-test".into(),
        node_daemon_generation: 1,
        provider_kind: "codex".into(),
        provider_profile_ref: "codex-default".into(),
        permission_envelope_ref: "permission-default".into(),
        effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
        workspace_cwd: None,
        lifecycle: AgentSessionStatus::Idle,
        runtime_generation: 1,
        control_state: AgentSessionControlState {
            execution_driver: MemberExecutionDriver::HostDriven,
            driver_generation: 1,
            driver_ref: RuntimeDriverRef::NodeDaemon {
                node_daemon_id: "daemon-test".into(),
                node_daemon_generation: 1,
            },
            composition_fingerprint: Some("composition:test".into()),
            capability_fingerprint: Some("capability:test".into()),
            ..Default::default()
        },
        native_session_ref: None,
        current_turn_id: None,
        queued_input_count: 0,
        version: 1,
        opened_at: "t2".into(),
        last_active_at: "t2".into(),
        closed_at: None,
    };
    harness
        .store
        .create_agent_session(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "session.create",
                &target_session.id,
                0,
            ),
            target_session.clone(),
        )
        .expect("create delegated target AgentSession");
    harness
        .store
        .bind_responsible_work_execution(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.bind",
                "binding-target-rollup",
                0,
            ),
            &RuntimeCommandBinding {
                target_member_run_id: Some(target_runtime_id.into()),
                target_member_run_generation: Some(1),
                target_session_id: Some(target_session.id.clone()),
                target_runtime_generation: Some(1),
                target_driver_generation: Some(1),
                target_driver: target_session.control_state.driver_ref.clone(),
                composition_fingerprint: target_session
                    .control_state
                    .composition_fingerprint
                    .clone(),
                capability_fingerprint: target_session.control_state.capability_fingerprint.clone(),
                permission_envelope_ref: Some(target_session.permission_envelope_ref.clone()),
                ..Default::default()
            },
            WorkExecutionBinding {
                id: "binding-target-rollup".into(),
                work_id: target.id.clone(),
                work_revision: target.version,
                team_id: target_run.agent_team_id.clone(),
                team_membership_id: "membership-team-delegation-target-target-worker".into(),
                agent_member_id: "target-worker".into(),
                agent_session_id: target_session.id,
                agent_session_generation: 1,
                delivery_id: "work-delivery:target-rollup:1".into(),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                bound_at: "t2".into(),
                ended_at: None,
            },
        )
        .expect("bind delegated target execution authority");
    harness
        .store
        .claim_work_for_provider(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.claim",
                "claim-target-rollup",
                0,
            ),
            "work-delivery:target-rollup:1",
            NODE,
            "daemon-test",
            1,
            "claim-target-rollup",
            RuntimeDispatchMode::QueueOnly,
            "t2.5",
        )
        .expect("claim delegated target delivery");
    harness
        .store
        .record_work_provider_receipt(
            &context(
                ActorRef {
                    kind: ActorKind::Service,
                    id: "daemon-test".into(),
                },
                "work.receipt",
                "receipt-target-rollup",
                0,
            ),
            "work-delivery:target-rollup:1",
            NODE,
            "daemon-test",
            1,
            "claim-target-rollup",
            "provider-receipt-target-rollup",
            "t2.75",
        )
        .expect("record delegated target provider receipt");
    let started = harness
        .store
        .start_work(
            &target.id,
            target.version,
            target_runtime_id,
            WorkCommandContext {
                event_id: "target-start".into(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::ProviderRuntimeProjection,
                    id: target_runtime_id.into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "target-start".into(),
                created_at: "t3".into(),
                duplicate_ok: false,
            },
        )
        .expect("start delegated target");
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "delegated-candidate".into(),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(&candidate).unwrap());
    let mut result = report(
        "report-delegated-target",
        WorkReportKind::Result,
        &member_actor("target-worker"),
    );
    result.work_id = target.id.clone();
    result.work_revision = started.version + 1;
    result.candidate = Some(candidate);
    result.candidate_fingerprint = Some(candidate_fingerprint.clone());
    result.evidence_refs = vec!["evidence://delegated-candidate".into()];
    harness
        .store
        .create_trust_work_report(
            &context(
                member_actor("target-worker"),
                "report.create",
                "report-delegated-target",
                0,
            ),
            &target_run.agent_team_id,
            result,
        )
        .expect("submit delegated target result");
    let accepted = harness
        .store
        .accept_trust_work(
            &context(
                human("host"),
                "work.accept",
                "accept-delegated-target",
                started.version + 1,
            ),
            &target_run.agent_team_id,
            &target.id,
            "report-delegated-target",
            &candidate_fingerprint,
            "t5",
        )
        .expect("accept delegated target");
    let rolled_up = harness
        .store
        .latest_work_delegations()
        .unwrap()
        .into_iter()
        .find(|row| row.id == delegation.id)
        .expect("rolled-up Delegation");
    assert_eq!(rolled_up.state, WorkDelegationState::Completed);
    assert_eq!(rolled_up.version, delegation.version + 1);
    let operation = harness
        .store
        .canonical_operations()
        .unwrap()
        .into_iter()
        .find(|operation| operation.event.id == accepted.event.id)
        .expect("canonical acceptance operation");
    assert!(operation.immutable_side_records.iter().any(|record| {
        serde_json::from_value::<firm_core::WorkDelegationRevision>(record.clone())
            .is_ok_and(|revision| revision.delegation.id == delegation.id)
    }));
    let source_after = harness
        .store
        .latest_works()
        .unwrap()
        .into_iter()
        .find(|work| work.id == source.id)
        .expect("source Work remains visible");
    assert_eq!(source_after, source, "roll-up must not mutate source Work");
}
