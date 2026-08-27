use super::{membership_id_for_member_run, unix_ms, TempHome};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus,
    MutationContext, PermissionCeiling, RuntimeActivity, RuntimeCommandBinding, RuntimeDriverRef,
    RuntimeResidency, WorkDeliveryStatus, WorkExecutionBinding, WorkExecutionBindingStatus,
};
use std::time::{Duration, Instant};

pub fn assign_work_for_member_run(
    home: &TempHome,
    execution_space_id: &str,
    work_id: &str,
    member_run_id: &str,
    bind_execution: bool,
) -> harness_core::Work {
    let store = harness_store::HarnessStore::new(home.spaces_dir().join(execution_space_id));
    let member = store
        .trust_member_runs(execution_space_id)
        .expect("read fixture MemberRuns")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("fixture MemberRun");
    let team_run = store
        .team_runs()
        .expect("read fixture TeamRuns")
        .into_iter()
        .rev()
        .find(|run| run.id == member.team_run_id)
        .expect("fixture TeamRun");
    let work = store
        .latest_works()
        .expect("read fixture Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .expect("fixture Work");
    let membership_id = membership_id_for_member_run(home, execution_space_id, member_run_id);
    let work = if work.assignee_membership_id.as_deref() == Some(membership_id.as_str()) {
        work
    } else {
        store
            .assign_work_to_membership(
                &work.id,
                work.version,
                &membership_id,
                execution_space_id,
                harness_core::WorkCommandContext {
                    event_id: format!("test-assign-{work_id}"),
                    performed_by_actor: team_run.host_actor.clone().expect("exact fixture Host"),
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("test-assign-{work_id}"),
                    created_at: "unix-ms:test-assign".into(),
                    duplicate_ok: false,
                },
            )
            .expect("assign fixture Work responsibility")
    };
    if !bind_execution {
        return work;
    }
    let now = unix_ms();
    let daemon = store
        .latest_node_daemon_lease(&team_run.execution_node_id)
        .expect("read fixture NodeDaemon lease")
        .unwrap_or_else(|| {
            store
                .acquire_node_daemon_lease(
                    &team_run.execution_node_id,
                    "test-node-daemon",
                    "test-node-daemon-instance",
                    now,
                    60_000,
                )
                .expect("acquire fixture NodeDaemon lease")
        });
    let sessions = if member.native_session.is_some() {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let sessions = current_member_sessions(
                &store,
                execution_space_id,
                &member.agent_member_id,
                member.native_session.as_ref(),
            );
            if !sessions.is_empty() {
                break sessions;
            }
            assert!(
                Instant::now() < deadline,
                "fixture MemberRun native session never converged to an exact AgentSession"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        current_member_sessions(&store, execution_space_id, &member.agent_member_id, None)
    };
    let session = match sessions.as_slice() {
        [session] => session.clone(),
        [] => {
            let session = AgentSession {
                id: format!("test-session:{}", member.agent_member_id),
                agent_member_id: member.agent_member_id.clone(),
                node_id: team_run.execution_node_id.clone(),
                execution_space_id: execution_space_id.into(),
                node_daemon_id: daemon.daemon_id.clone(),
                node_daemon_generation: daemon.generation,
                provider_kind: member
                    .provider_profile_snapshot
                    .clone()
                    .unwrap_or_else(|| "codex".into()),
                provider_profile_ref: "test".into(),
                permission_envelope_ref: format!("test-permission:{}", member.agent_member_id),
                effective_permission_ceiling: PermissionCeiling::FullAccess,
                workspace_cwd: Some(
                    std::fs::canonicalize(home.base())
                        .expect("canonical fixture workspace")
                        .to_string_lossy()
                        .into_owned(),
                ),
                lifecycle: AgentSessionStatus::Idle,
                runtime_generation: 1,
                control_state: AgentSessionControlState {
                    driver_generation: 1,
                    driver_ref: RuntimeDriverRef::NodeDaemon {
                        node_daemon_id: daemon.daemon_id.clone(),
                        node_daemon_generation: daemon.generation,
                    },
                    composition_fingerprint: Some("test-composition-v1".into()),
                    capability_fingerprint: Some("test-capability-v1".into()),
                    runtime_residency: RuntimeResidency::Detached,
                    activity: RuntimeActivity::Idle,
                    ..Default::default()
                },
                native_session_ref: None,
                current_turn_id: None,
                queued_input_count: 0,
                version: 1,
                opened_at: "unix-ms:test-session".into(),
                last_active_at: "unix-ms:test-session".into(),
                closed_at: None,
            };
            store
                .create_agent_session(
                    &MutationContext {
                        execution_space_id: execution_space_id.into(),
                        authenticated_actor: ActorRef {
                            kind: ActorKind::Service,
                            id: daemon.daemon_id.clone(),
                        },
                        authority_actor: None,
                        command_name: "test.session.create".into(),
                        idempotency_key: session.id.clone(),
                        expected_version: 0,
                        request_fingerprint: None,
                    },
                    session.clone(),
                )
                .expect("create fixture AgentSession");
            session
        }
        _ => panic!("fixture AgentMember has ambiguous current AgentSessions"),
    };

    if exact_active_binding_exists(
        &store,
        execution_space_id,
        &work,
        &membership_id,
        &member,
        &session,
        &daemon,
    ) {
        return work;
    }
    let binding_generation = store
        .fabric_work_execution_bindings(execution_space_id)
        .expect("read fixture WorkExecutionBindings")
        .into_iter()
        .filter(|binding| binding.work_id == work.id)
        .map(|binding| binding.binding_generation)
        .max()
        .unwrap_or(0)
        + 1;
    let binding_id = format!("work-binding:{work_id}:{binding_generation}");
    let result = store.bind_responsible_work_execution(
        &MutationContext {
            execution_space_id: execution_space_id.into(),
            authenticated_actor: ActorRef {
                kind: ActorKind::Service,
                id: daemon.daemon_id.clone(),
            },
            authority_actor: None,
            command_name: "test.work.bind".into(),
            idempotency_key: binding_id.clone(),
            expected_version: 0,
            request_fingerprint: None,
        },
        &RuntimeCommandBinding {
            target_member_run_id: Some(member.id.clone()),
            target_member_run_generation: Some(member.runtime_generation),
            target_session_id: Some(session.id.clone()),
            target_runtime_generation: Some(session.runtime_generation),
            target_driver_generation: Some(session.control_state.driver_generation),
            target_driver: session.control_state.driver_ref.clone(),
            native_session_ref: session.native_session_ref.clone(),
            composition_fingerprint: session.control_state.composition_fingerprint.clone(),
            capability_fingerprint: session.control_state.capability_fingerprint.clone(),
            permission_envelope_ref: Some(session.permission_envelope_ref.clone()),
            ..Default::default()
        },
        WorkExecutionBinding {
            id: binding_id,
            work_id: work.id.clone(),
            work_revision: work.version,
            team_id: team_run.agent_team_id,
            team_membership_id: membership_id.clone(),
            agent_member_id: member.agent_member_id.clone(),
            agent_session_id: session.id.clone(),
            agent_session_generation: session.runtime_generation,
            delivery_id: format!("work-delivery:{work_id}:{binding_generation}"),
            binding_generation,
            status: WorkExecutionBindingStatus::Active,
            version: 1,
            created_by: ActorRef {
                kind: ActorKind::Service,
                id: daemon.daemon_id.clone(),
            },
            bound_at: "unix-ms:test-bind".into(),
            ended_at: None,
        },
    );
    if let Err(error) = result {
        assert!(
            exact_active_binding_exists(
                &store,
                execution_space_id,
                &work,
                &membership_id,
                &member,
                &session,
                &daemon,
            ),
            "bind fixture Work execution: {error}"
        );
    }
    work
}

fn current_member_sessions(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    agent_member_id: &str,
    native_session: Option<&harness_core::agentfirm_api::NativeSessionRef>,
) -> Vec<AgentSession> {
    store
        .fabric_agent_sessions(execution_space_id)
        .expect("read fixture AgentSessions")
        .into_iter()
        .filter(|session| {
            session.agent_member_id == agent_member_id
                && session.lifecycle != AgentSessionStatus::Closed
                && native_session.is_none_or(|native| {
                    session
                        .native_session_ref
                        .as_ref()
                        .is_some_and(|current| current.same_identity_as(native))
                })
        })
        .collect()
}

fn exact_active_binding_exists(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    work: &harness_core::Work,
    membership_id: &str,
    member: &harness_core::agentfirm_api::MemberRun,
    session: &AgentSession,
    daemon: &harness_core::NodeDaemonLease,
) -> bool {
    let bindings = store
        .fabric_work_execution_bindings(execution_space_id)
        .expect("read fixture WorkExecutionBindings")
        .into_iter()
        .filter(|binding| {
            binding.work_id == work.id && binding.status == WorkExecutionBindingStatus::Active
        })
        .collect::<Vec<_>>();
    let [binding] = bindings.as_slice() else {
        assert!(
            bindings.is_empty(),
            "fixture Work has ambiguous active bindings"
        );
        return false;
    };
    let exact = binding.work_revision == work.version
        && binding.team_membership_id == membership_id
        && binding.agent_member_id == member.agent_member_id
        && binding.agent_session_id == session.id
        && binding.agent_session_generation == session.runtime_generation;
    assert!(
        exact,
        "fixture Work has conflicting active binding: {binding:?}"
    );
    let runtime_binding = store
        .work_execution_runtime_binding(execution_space_id, &binding.id)
        .expect("read fixture WorkExecutionBinding runtime authority");
    assert_eq!(
        runtime_binding.target_member_run_id.as_deref(),
        Some(member.id.as_str()),
        "fixture binding targets another MemberRun"
    );
    assert_eq!(
        runtime_binding.target_member_run_generation,
        Some(member.runtime_generation),
        "fixture binding targets another MemberRun generation"
    );
    assert_eq!(
        runtime_binding.target_session_id.as_deref(),
        Some(session.id.as_str()),
        "fixture binding targets another AgentSession"
    );
    assert_eq!(
        runtime_binding.target_runtime_generation,
        Some(session.runtime_generation),
        "fixture binding targets another AgentSession generation"
    );
    assert_eq!(
        runtime_binding.target_driver_generation,
        Some(session.control_state.driver_generation),
        "fixture binding targets another driver generation"
    );
    assert_eq!(
        runtime_binding.target_driver, session.control_state.driver_ref,
        "fixture binding targets another execution driver"
    );
    assert_eq!(
        session.node_daemon_id, daemon.daemon_id,
        "fixture session daemon id"
    );
    assert_eq!(
        session.node_daemon_generation, daemon.generation,
        "fixture session daemon generation"
    );
    assert_eq!(
        runtime_binding.composition_fingerprint, session.control_state.composition_fingerprint,
        "fixture binding composition fingerprint"
    );
    assert_eq!(
        runtime_binding.capability_fingerprint, session.control_state.capability_fingerprint,
        "fixture binding capability fingerprint"
    );
    assert_eq!(
        runtime_binding.permission_envelope_ref.as_deref(),
        Some(session.permission_envelope_ref.as_str()),
        "fixture binding permission envelope"
    );
    if let (Some(frozen), Some(current)) = (
        runtime_binding.native_session_ref.as_ref(),
        session.native_session_ref.as_ref(),
    ) {
        assert!(
            harness_core::agentfirm_api::native_session_identity_matches(frozen, current),
            "fixture binding native session identity drifted"
        );
    }
    let deliveries = store
        .fabric_work_deliveries(execution_space_id)
        .expect("read fixture Work deliveries");
    let matching = deliveries
        .iter()
        .filter(|delivery| {
            delivery.id == binding.delivery_id
                && delivery.work_execution_binding_id == binding.id
                && delivery.work_id == binding.work_id
                && delivery.work_revision == binding.work_revision
                && delivery.recipient_agent_member_id == binding.agent_member_id
                && delivery.recipient_session_id == binding.agent_session_id
                && delivery.recipient_session_generation == binding.agent_session_generation
                && delivery.target_node_id == session.node_id
        })
        .collect::<Vec<_>>();
    let [delivery] = matching.as_slice() else {
        panic!("fixture binding lacks one exact delivery: {matching:?}");
    };
    match delivery.status {
        WorkDeliveryStatus::Queued => {
            assert!(delivery.claim_id.is_none());
            assert!(delivery.claimed_node_daemon_generation.is_none());
            assert!(delivery.provider_receipt_id.is_none());
        }
        WorkDeliveryStatus::Claimed => {
            assert!(delivery.claim_id.is_some());
            assert_eq!(
                delivery.claimed_node_daemon_generation,
                Some(daemon.generation)
            );
            assert!(delivery.provider_receipt_id.is_none());
        }
        WorkDeliveryStatus::ProviderReceived => {
            assert!(delivery.claim_id.is_some());
            assert_eq!(
                delivery.claimed_node_daemon_generation,
                Some(daemon.generation)
            );
            assert!(delivery.provider_receipt_id.is_some());
        }
        WorkDeliveryStatus::Failed => panic!("fixture binding delivery is failed: {delivery:?}"),
    }
    true
}
