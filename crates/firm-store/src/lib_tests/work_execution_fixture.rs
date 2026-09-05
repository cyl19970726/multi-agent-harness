use super::*;

use firm_core::agentfirm_api::{
    ActorKind, ActorRef, AgentSession, AgentSessionControlState, AgentSessionStatus, CandidateKind,
    CandidateRef, MutationContext, PermissionCeiling, RuntimeActivity, RuntimeDispatchMode,
    RuntimeDriverRef, RuntimeResidency, WorkExecutionBinding, WorkExecutionBindingStatus,
    WorkReport, WorkReportKind,
};

pub(super) fn start_claimed_work_for_test(
    store: &HarnessStore,
    claimed: &Work,
    member: &ProviderRuntimeProjection,
    event_id: &str,
    key: &str,
    at: &str,
) -> Work {
    let run = store
        .team_runs()
        .expect("test TeamRuns")
        .into_iter()
        .find(|run| run.id == member.team_run_id)
        .expect("test MemberRun TeamRun");
    let space_id = store
        .current_team_run_execution_space(&run)
        .expect("test TeamRun execution space");
    let session_id = format!("session:{}", member.agent_member_id);
    let now_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    if store
        .latest_node_daemon_lease(&run.execution_node_id)
        .expect("test NodeDaemon lease")
        .is_none()
    {
        store
            .acquire_node_daemon_lease(
                &run.execution_node_id,
                "test-node-daemon",
                "test-node-daemon-instance",
                now_unix_ms,
                60_000,
            )
            .expect("acquire test NodeDaemon lease");
    }
    if !store
        .fabric_agent_sessions(&space_id)
        .expect("test AgentSessions")
        .iter()
        .any(|session| session.id == session_id)
    {
        store
            .create_agent_session(
                &MutationContext {
                    execution_space_id: space_id.clone(),
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
                    node_id: run.execution_node_id.clone(),
                    execution_space_id: space_id.clone(),
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
        .fabric_team_memberships(&space_id)
        .expect("test TeamMemberships")
        .into_iter()
        .find(|membership| {
            Some(membership.id.as_str()) == claimed.assignee_membership_id.as_deref()
        })
        .expect("claimed Work TeamMembership");
    let binding_generation = 1;
    let binding_id = format!("work-binding:{}:{binding_generation}", claimed.id);
    let delivery_id = format!("work-delivery:{}:{binding_generation}", claimed.id);
    store
        .bind_work_execution_fixture(
            &MutationContext {
                execution_space_id: space_id.clone(),
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
                delivery_id: delivery_id.clone(),
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
    let daemon = store
        .latest_node_daemon_lease(&run.execution_node_id)
        .expect("read test NodeDaemon lease")
        .expect("test NodeDaemon lease");
    let service_context = |command_name: &str, key: String| MutationContext {
        execution_space_id: space_id.clone(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon.daemon_id.clone(),
        },
        authority_actor: None,
        command_name: command_name.into(),
        idempotency_key: key,
        expected_version: 0,
        request_fingerprint: None,
    };
    let claim_id = format!("claim:{delivery_id}");
    store
        .claim_work_for_provider(
            &service_context("test.work.claim", claim_id.clone()),
            &delivery_id,
            &daemon.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &claim_id,
            RuntimeDispatchMode::QueueOnly,
            at,
        )
        .expect("claim test Work delivery before Start");
    let receipt_id = format!("provider-receipt:{delivery_id}");
    store
        .record_work_provider_receipt(
            &service_context("test.work.receipt", receipt_id.clone()),
            &delivery_id,
            &daemon.node_id,
            &daemon.daemon_id,
            daemon.generation,
            &claim_id,
            &receipt_id,
            at,
        )
        .expect("record test provider receipt before Start");
    store
        .start_work(
            &claimed.id,
            claimed.version,
            &member.id,
            member_work_context(&member.id, event_id, key, at),
        )
        .expect("start claimed Work after stable responsibility")
}

pub(super) fn submit_started_work_for_test(
    store: &HarnessStore,
    active: &Work,
    member: &ProviderRuntimeProjection,
    event_id: &str,
    result_summary: &str,
    evidence_refs: (Vec<String>, Vec<String>),
    at: &str,
) -> Work {
    let run = store
        .team_runs()
        .expect("test TeamRuns")
        .into_iter()
        .find(|run| run.id == member.team_run_id)
        .expect("test MemberRun TeamRun");
    let space_id = store
        .current_team_run_execution_space(&run)
        .expect("test TeamRun execution space");
    let report = result_report_for_test(
        active,
        member,
        event_id,
        result_summary,
        evidence_refs.0,
        evidence_refs.1,
        at,
    );
    let team_id = active
        .accountable_team_id
        .as_deref()
        .expect("test Work accountable Team");
    store
        .create_trust_work_report(
            &MutationContext {
                execution_space_id: space_id,
                authenticated_actor: report.authored_by.clone(),
                authority_actor: None,
                command_name: "test.work_report.create".into(),
                idempotency_key: report.id.clone(),
                expected_version: 0,
                request_fingerprint: None,
            },
            team_id,
            report,
        )
        .expect("submit canonical test Work Result");
    store
        .latest_works()
        .expect("read submitted test Work")
        .into_iter()
        .find(|work| work.id == active.id)
        .expect("submitted test Work")
}

pub(super) fn result_report_for_test(
    work: &Work,
    member: &ProviderRuntimeProjection,
    event_id: &str,
    result_summary: &str,
    artifact_refs: Vec<String>,
    check_refs: Vec<String>,
    at: &str,
) -> WorkReport {
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: format!("candidate-{event_id}"),
    };
    WorkReport {
        id: format!("report-{event_id}"),
        work_id: work.id.clone(),
        work_revision: work.version + 1,
        report_revision: 1,
        kind: WorkReportKind::Result,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: member.agent_member_id.clone(),
        },
        summary: result_summary.into(),
        base_revision: None,
        candidate_fingerprint: Some(canonical_json_fingerprint(
            &serde_json::to_value(&candidate).expect("candidate JSON"),
        )),
        candidate: Some(candidate),
        report_only: false,
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs,
        check_refs,
        github_links: Vec::new(),
        evidence_refs: vec![format!("evidence:{event_id}")],
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: at.into(),
    }
}

pub(super) fn accept_result_for_test(
    store: &HarnessStore,
    submitted: &Work,
    report_event_id: &str,
    idempotency_key: &str,
    at: &str,
) -> Work {
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: format!("candidate-{report_event_id}"),
    };
    let candidate_fingerprint =
        canonical_json_fingerprint(&serde_json::to_value(candidate).expect("candidate JSON"));
    let team_id = submitted
        .accountable_team_id
        .as_deref()
        .expect("test Work accountable Team");
    store
        .accept_trust_work(
            &MutationContext {
                execution_space_id: store
                    .current_team_run_execution_space(
                        &store
                            .team_runs()
                            .expect("test TeamRuns")
                            .into_iter()
                            .find(|run| run.id == submitted.team_run_id)
                            .expect("test Work TeamRun"),
                    )
                    .expect("test TeamRun execution space"),
                authenticated_actor: ActorRef {
                    kind: ActorKind::Human,
                    id: "reviewer".into(),
                },
                authority_actor: None,
                command_name: "test.work.accept".into(),
                idempotency_key: idempotency_key.into(),
                expected_version: submitted.version,
                request_fingerprint: None,
            },
            team_id,
            &submitted.id,
            &format!("report-{report_event_id}"),
            &candidate_fingerprint,
            at,
        )
        .expect("accept canonical test Result");
    store
        .latest_works()
        .expect("read accepted test Work")
        .into_iter()
        .find(|work| work.id == submitted.id)
        .expect("accepted test Work")
}
