use super::*;

use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus,
    MutationContext, PermissionCeiling, RuntimeActivity, RuntimeDriverRef, RuntimeResidency,
    WorkExecutionBinding, WorkExecutionBindingStatus,
};

pub(super) fn start_claimed_work_for_test(
    store: &HarnessStore,
    claimed: &Work,
    member: &ProviderRuntimeProjection,
    event_id: &str,
    key: &str,
    at: &str,
) -> Work {
    let space_id = "unit-test-space";
    let session_id = format!("session:{}", member.agent_member_id);
    let now_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    if store
        .latest_node_daemon_lease("00000000-0000-4000-8000-000000000001")
        .expect("test NodeDaemon lease")
        .is_none()
    {
        store
            .acquire_node_daemon_lease(
                "00000000-0000-4000-8000-000000000001",
                "test-node-daemon",
                "test-node-daemon-instance",
                now_unix_ms,
                60_000,
            )
            .expect("acquire test NodeDaemon lease");
    }
    if !store
        .fabric_agent_sessions(space_id)
        .expect("test AgentSessions")
        .iter()
        .any(|session| session.id == session_id)
    {
        store
            .create_agent_session(
                &MutationContext {
                    execution_space_id: space_id.into(),
                    authenticated_actor: ActorRef {
                        kind: ActorKind::Service,
                        id: "test-node-daemon".into(),
                    },
                    authority_actor: None,
                    command_name: "test.session.create".into(),
                    idempotency_key: session_id.clone(),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                AgentSession {
                    id: session_id.clone(),
                    agent_member_id: member.agent_member_id.clone(),
                    node_id: "00000000-0000-4000-8000-000000000001".into(),
                    execution_space_id: space_id.into(),
                    node_daemon_id: "test-node-daemon".into(),
                    node_daemon_generation: 1,
                    provider_kind: "codex".into(),
                    provider_profile_ref: "test".into(),
                    permission_envelope_ref: format!(
                        "agent-member:{}:permission",
                        member.agent_member_id
                    ),
                    effective_permission_ceiling: PermissionCeiling::WorkspaceWrite,
                    workspace_cwd: None,
                    lifecycle: AgentSessionStatus::Idle,
                    runtime_generation: 1,
                    control_state: AgentSessionControlState {
                        driver_generation: 1,
                        driver_ref: RuntimeDriverRef::NodeDaemon {
                            node_daemon_id: "test-node-daemon".into(),
                            node_daemon_generation: 1,
                        },
                        runtime_residency: RuntimeResidency::Detached,
                        activity: RuntimeActivity::Idle,
                        composition_fingerprint: Some("test-composition-v1".into()),
                        capability_fingerprint: Some("test-capability-v1".into()),
                        ..Default::default()
                    },
                    native_session_ref: None,
                    current_turn_id: None,
                    queued_input_count: 0,
                    version: 1,
                    opened_at: at.into(),
                    last_active_at: at.into(),
                    closed_at: None,
                },
            )
            .expect("create test AgentSession");
    }
    let membership = store
        .fabric_team_memberships(space_id)
        .expect("test TeamMemberships")
        .into_iter()
        .find(|membership| {
            Some(membership.id.as_str()) == claimed.assignee_membership_id.as_deref()
        })
        .expect("claimed Work TeamMembership");
    let binding_generation = 1;
    let binding_id = format!("work-binding:{}:{binding_generation}", claimed.id);
    store
        .bind_work_execution_fixture(
            &MutationContext {
                execution_space_id: space_id.into(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: "test-node-daemon".into(),
                },
                authority_actor: None,
                command_name: "test.work.bind".into(),
                idempotency_key: binding_id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            WorkExecutionBinding {
                id: binding_id,
                work_id: claimed.id.clone(),
                work_revision: claimed.version,
                team_id: membership.team_id.clone(),
                team_membership_id: membership.id,
                agent_member_id: member.agent_member_id.clone(),
                agent_session_id: session_id,
                agent_session_generation: 1,
                delivery_id: format!("work-delivery:{}:{binding_generation}", claimed.id),
                binding_generation,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: ActorRef {
                    kind: ActorKind::Service,
                    id: "test-node-daemon".into(),
                },
                bound_at: at.into(),
                ended_at: None,
            },
        )
        .expect("bind claimed Work for test");
    store
        .start_work(
            &claimed.id,
            claimed.version,
            &member.id,
            member_work_context(&member.id, event_id, key, at),
        )
        .expect("start claimed Work after stable responsibility")
}
