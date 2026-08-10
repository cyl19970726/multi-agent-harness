//! Transport-neutral application service for the Member Execution Trust Kernel.
//!
//! HTTP, MCP and CLI decode their own authenticated transport context and then
//! call [`execute`]. No request payload can select or override the actor.

use harness_core::agentfirm_api::{
    ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateWaiver, MemberCoordinationStatus,
    MemberRun, MemberWorkspaceBinding, MutationContext, TeamMessage, WorkFinding,
    WorkModuleBinding, WorkReport, WorkspaceLifecycle, WorkspaceSafetyProof,
};
use harness_store::{CanonicalMutationResult, HarnessStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MEMBER_TRUST_PROTOCOL_VERSION: &str = "agentfirm-member-trust/1";

pub fn parse_actor_kind(value: &str) -> Option<ActorKind> {
    match value {
        "human" => Some(ActorKind::Human),
        "agent_member" => Some(ActorKind::AgentMember),
        "external" => Some(ActorKind::External),
        "service" => Some(ActorKind::Service),
        _ => None,
    }
}

pub fn is_http_mutation_path(path: &str) -> bool {
    crate::role_actions_api::is_http_mutation_path(path)
        || path == "/v1/agent-members"
        || path.starts_with("/v1/agent-members/")
        || path.starts_with("/v1/member-runs/")
        || path.starts_with("/v1/message-deliveries/")
        || path.starts_with("/v1/work-deliveries/")
        || path.starts_with("/v1/gate-requirements/")
        || path.starts_with("/v1/gate-waivers/")
        || (path.starts_with("/v1/team-runs/")
            && (path.ends_with("/member-runs") || path.ends_with("/messages")))
        || (path.starts_with("/v1/teams/")
            && [
                "/reports",
                "/findings",
                "/failure-analyses",
                "/modules",
                "/gate-requirements",
                "/accept",
            ]
            .iter()
            .any(|suffix| path.ends_with(suffix)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustCommand {
    CreateAgentMember {
        member: AgentMember,
    },
    PauseAgentMember {
        member_id: String,
        updated_at: String,
    },
    ResumeAgentMember {
        member_id: String,
        updated_at: String,
    },
    RetireAgentMember {
        member_id: String,
        updated_at: String,
    },
    CreateMemberRun {
        run: MemberRun,
    },
    CloseMemberRun {
        member_run_id: String,
        updated_at: String,
    },
    ReopenMemberRun {
        member_run_id: String,
        updated_at: String,
    },
    RetireMemberRun {
        member_run_id: String,
        updated_at: String,
    },
    ResumeNativeSession {
        member_run_id: String,
        updated_at: String,
    },
    CreateTeamMessage {
        message: TeamMessage,
        updated_at: String,
    },
    RetryMessageDelivery {
        delivery_id: String,
        updated_at: String,
    },
    ReconcileMessageDelivery {
        delivery_id: String,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: String,
        updated_at: String,
    },
    CreateWorkDeliveries {
        work_event_id: String,
        work_id: String,
        work_revision: u64,
        recipient_member_run_ids: Vec<String>,
        updated_at: String,
    },
    RetryWorkDelivery {
        delivery_id: String,
        current_work_revision: u64,
        updated_at: String,
    },
    ReconcileWorkDelivery {
        delivery_id: String,
        evidence_ref: String,
        updated_at: String,
    },
    ProvisionWorkspace {
        binding: MemberWorkspaceBinding,
    },
    TransitionWorkspace {
        member_run_id: String,
        binding_id: String,
        next: WorkspaceLifecycle,
        proof: WorkspaceSafetyProof,
        updated_at: String,
    },
    CreateWorkReport {
        team_id: String,
        report: WorkReport,
    },
    CreateWorkFinding {
        team_id: String,
        finding: WorkFinding,
    },
    CreateFailureAnalysis {
        team_id: String,
        analysis: FailureAnalysis,
    },
    BindWorkModule {
        team_id: String,
        binding: WorkModuleBinding,
    },
    CreateGateRequirement {
        team_id: String,
        requirement: GateRequirement,
    },
    AcceptWork {
        team_id: String,
        work_id: String,
        work_report_id: String,
        candidate_fingerprint: String,
        updated_at: String,
    },
    EvaluateGate {
        evaluation: GateEvaluation,
    },
    WaiveGate {
        waiver: GateWaiver,
    },
    RevokeGateWaiver {
        waiver_id: String,
        revoked_at: String,
    },
}

impl TrustCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CreateAgentMember { .. } => "agent_member.create",
            Self::PauseAgentMember { .. } => "agent_member.pause",
            Self::ResumeAgentMember { .. } => "agent_member.resume",
            Self::RetireAgentMember { .. } => "agent_member.retire",
            Self::CreateMemberRun { .. } => "member_run.create",
            Self::CloseMemberRun { .. } => "member_run.close",
            Self::ReopenMemberRun { .. } => "member_run.reopen",
            Self::RetireMemberRun { .. } => "member_run.retire",
            Self::ResumeNativeSession { .. } => "member_run.resume_native_session",
            Self::CreateTeamMessage { .. } => "team_message.create",
            Self::RetryMessageDelivery { .. } => "message_delivery.retry",
            Self::ReconcileMessageDelivery { .. } => "message_delivery.reconcile",
            Self::CreateWorkDeliveries { .. } => "work_delivery.create",
            Self::RetryWorkDelivery { .. } => "work_delivery.retry",
            Self::ReconcileWorkDelivery { .. } => "work_delivery.reconcile",
            Self::ProvisionWorkspace { .. } => "workspace.provision",
            Self::TransitionWorkspace { next, .. } => match next {
                WorkspaceLifecycle::Archived => "workspace.archive",
                WorkspaceLifecycle::Removed => "workspace.cleanup",
                WorkspaceLifecycle::Attached => "workspace.attach",
                _ => "workspace.transition",
            },
            Self::CreateWorkReport { .. } => "work_report.create",
            Self::CreateWorkFinding { .. } => "work_finding.create",
            Self::CreateFailureAnalysis { .. } => "failure_analysis.create",
            Self::BindWorkModule { .. } => "work_module.bind",
            Self::CreateGateRequirement { .. } => "gate_requirement.create",
            Self::AcceptWork { .. } => "work.accept",
            Self::EvaluateGate { .. } => "gate_requirement.evaluate",
            Self::WaiveGate { .. } => "gate_requirement.waive",
            Self::RevokeGateWaiver { .. } => "gate_waiver.revoke",
        }
    }

    pub fn matches_http_route(&self, path: &str) -> bool {
        let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
        match (self, parts.as_slice()) {
            (Self::CreateAgentMember { .. }, ["v1", "agent-members"]) => true,
            (Self::PauseAgentMember { member_id, .. }, ["v1", "agent-members", id, "pause"])
            | (Self::ResumeAgentMember { member_id, .. }, ["v1", "agent-members", id, "resume"])
            | (Self::RetireAgentMember { member_id, .. }, ["v1", "agent-members", id, "retire"]) => {
                member_id == id
            }
            (Self::CreateMemberRun { run }, ["v1", "team-runs", id, "member-runs"]) => {
                &run.team_run_id == id
            }
            (Self::CloseMemberRun { member_run_id, .. }, ["v1", "member-runs", id, "close"])
            | (Self::ReopenMemberRun { member_run_id, .. }, ["v1", "member-runs", id, "reopen"])
            | (Self::RetireMemberRun { member_run_id, .. }, ["v1", "member-runs", id, "retire"]) => {
                member_run_id == id
            }
            (
                Self::ResumeNativeSession { member_run_id, .. },
                ["v1", "member-runs", id, "resume-native-session"],
            ) => member_run_id == id,
            (Self::CreateTeamMessage { message, .. }, ["v1", "team-runs", id, "messages"]) => {
                &message.team_run_id == id
            }
            (
                Self::RetryMessageDelivery { delivery_id, .. },
                ["v1", "message-deliveries", id, "retry"],
            )
            | (
                Self::ReconcileMessageDelivery { delivery_id, .. },
                ["v1", "message-deliveries", id, "reconcile"],
            ) => delivery_id == id,
            (
                Self::RetryWorkDelivery { delivery_id, .. },
                ["v1", "work-deliveries", id, "retry"],
            )
            | (
                Self::ReconcileWorkDelivery { delivery_id, .. },
                ["v1", "work-deliveries", id, "reconcile"],
            ) => delivery_id == id,
            (
                Self::ProvisionWorkspace { binding },
                ["v1", "member-runs", id, "workspace", "provision"],
            ) => &binding.member_run_id == id,
            (
                Self::TransitionWorkspace {
                    member_run_id,
                    binding_id,
                    next,
                    ..
                },
                ["v1", "member-runs", member, "workspace", action],
            ) => {
                member_run_id == member
                    && !binding_id.is_empty()
                    && matches!(
                        (next, *action),
                        (WorkspaceLifecycle::Attached, "attach")
                            | (WorkspaceLifecycle::Archived, "archive")
                            | (WorkspaceLifecycle::Removed, "cleanup")
                    )
            }
            (
                Self::CreateWorkReport { team_id, report },
                ["v1", "teams", team, "works", work, "reports"],
            ) => team_id == team && &report.work_id == work,
            (
                Self::CreateWorkFinding { team_id, finding },
                ["v1", "teams", team, "works", work, "findings"],
            ) => team_id == team && &finding.work_id == work,
            (
                Self::CreateFailureAnalysis { team_id, analysis },
                ["v1", "teams", team, "works", work, "failure-analyses"],
            ) => team_id == team && &analysis.work_id == work,
            (
                Self::BindWorkModule { team_id, binding },
                ["v1", "teams", team, "works", work, "modules"],
            ) => team_id == team && &binding.work_id == work,
            (
                Self::CreateGateRequirement {
                    team_id,
                    requirement,
                },
                ["v1", "teams", team, "works", work, "gate-requirements"],
            ) => team_id == team && &requirement.work_id == work,
            (
                Self::AcceptWork {
                    team_id, work_id, ..
                },
                ["v1", "teams", team, "works", work, "accept"],
            ) => team_id == team && work_id == work,
            (Self::EvaluateGate { evaluation }, ["v1", "gate-requirements", id, "evaluate"]) => {
                &evaluation.requirement_id == id
            }
            (Self::WaiveGate { waiver }, ["v1", "gate-requirements", id, "waive"]) => {
                &waiver.requirement_id == id
            }
            (Self::RevokeGateWaiver { waiver_id, .. }, ["v1", "gate-waivers", id, "revoke"]) => {
                waiver_id == id
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedMutation {
    pub execution_space_id: String,
    pub actor: ActorRef,
    /// Authority identities bound to this credential/session by the transport.
    /// A request body may reference one of these identities but can never add
    /// to this server-resolved set.
    pub authorized_authority_actors: Vec<ActorRef>,
    pub idempotency_key: String,
    pub expected_version: u64,
    /// Present only for the closed browser semantic adapter. It binds route,
    /// typed intent, identity, authority and original If-Match across retries.
    pub request_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustCommandResult {
    pub ok: bool,
    pub protocol_version: &'static str,
    pub projection: Value,
    pub event_id: String,
    pub store_sequence: u64,
    pub resulting_version: u64,
    pub replayed: bool,
}

fn unauthorized(resource_kind: &str, resource_id: &str, message: &str) -> StoreError {
    StoreError::Conflict(
        serde_json::to_string(&harness_core::agentfirm_api::TrustError {
            code: harness_core::agentfirm_api::TrustErrorCode::UnauthorizedActor,
            message: message.to_string(),
            retryable: false,
            resource_kind: resource_kind.to_string(),
            resource_id: resource_id.to_string(),
            current_version: None,
        })
        .expect("TrustError serializes"),
    )
}

fn member_run_owned_by(
    store: &HarnessStore,
    execution_space_id: &str,
    member_run_id: &str,
    agent_member_id: &str,
) -> Result<bool, StoreError> {
    Ok(store
        .trust_member_runs(execution_space_id)?
        .into_iter()
        .any(|run| run.id == member_run_id && run.agent_member_id == agent_member_id))
}

fn enforce_machine_scoped_service(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    command: &TrustCommand,
) -> Result<(), StoreError> {
    if auth.actor.kind != ActorKind::Service {
        return Ok(());
    }
    let recipient_member_run_id = match command {
        TrustCommand::RetryWorkDelivery { delivery_id, .. }
        | TrustCommand::ReconcileWorkDelivery { delivery_id, .. } => store
            .trust_work_deliveries(&auth.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == *delivery_id)
            .map(|delivery| delivery.recipient_member_run_id),
        TrustCommand::RetryMessageDelivery { delivery_id, .. }
        | TrustCommand::ReconcileMessageDelivery { delivery_id, .. } => store
            .trust_message_deliveries(&auth.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == *delivery_id)
            .map(|delivery| delivery.recipient_member_run_id),
        _ => return Ok(()),
    };
    let Some(member_run_id) = recipient_member_run_id else {
        return Ok(());
    };
    let team_run_id = store
        .trust_member_runs(&auth.execution_space_id)?
        .into_iter()
        .find(|run| run.id == member_run_id)
        .map(|run| run.team_run_id)
        .ok_or_else(|| {
            unauthorized(
                "member_run",
                &member_run_id,
                "delivery has no canonical MemberRun",
            )
        })?;
    let node_id = store
        .team_runs()?
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .map(|run| run.execution_node_id)
        .ok_or_else(|| {
            unauthorized(
                "team_run",
                &team_run_id,
                "delivery has no canonical TeamRun",
            )
        })?;
    let exact_node = auth.actor.id == node_id
        || auth
            .authorized_authority_actors
            .iter()
            .any(|actor| actor.kind == ActorKind::Service && actor.id == node_id);
    if !exact_node {
        return Err(unauthorized(
            "execution_node",
            &node_id,
            "Service delivery recovery is scoped to its exact Execution Node",
        ));
    }
    Ok(())
}

/// The transport proves who the caller is; this boundary decides what that
/// identity may mutate. Wave 4A deliberately keeps the policy small: Human
/// and Service actors operate the control plane, while AgentMember/External
/// actors may author execution evidence and conversation. An AgentMember may
/// additionally control only its own MemberRun/native session.
fn authorize(
    store: &HarnessStore,
    execution_space_id: &str,
    actor: &ActorRef,
    command: &TrustCommand,
) -> Result<(), StoreError> {
    if matches!(actor.kind, ActorKind::Human | ActorKind::Service) {
        return Ok(());
    }

    let execution_authoring = matches!(
        command,
        TrustCommand::CreateTeamMessage { .. }
            | TrustCommand::CreateWorkReport { .. }
            | TrustCommand::CreateWorkFinding { .. }
            | TrustCommand::CreateFailureAnalysis { .. }
            | TrustCommand::EvaluateGate { .. }
            | TrustCommand::WaiveGate { .. }
            | TrustCommand::RevokeGateWaiver { .. }
    );
    if execution_authoring {
        return Ok(());
    }

    if actor.kind == ActorKind::AgentMember {
        if let TrustCommand::CreateGateRequirement { team_id, .. } = command {
            if store
                .latest_teams()?
                .get(team_id)
                .is_some_and(|team| team.host_agent_id == actor.id)
            {
                return Ok(());
            }
            return Err(unauthorized(
                "agent_team",
                team_id,
                "only the exact Team Host may request a Gate evaluation",
            ));
        }
        if let TrustCommand::AcceptWork { team_id, .. } = command {
            let exact_host = store
                .latest_teams()?
                .get(team_id)
                .is_some_and(|team| team.host_agent_id == actor.id);
            if exact_host {
                return Ok(());
            }
            return Err(unauthorized(
                "agent_team",
                team_id,
                "only the exact Team Host may accept Work",
            ));
        }
        let own_run = match command {
            TrustCommand::CloseMemberRun { member_run_id, .. }
            | TrustCommand::ReopenMemberRun { member_run_id, .. }
            | TrustCommand::RetireMemberRun { member_run_id, .. }
            | TrustCommand::ResumeNativeSession { member_run_id, .. }
            | TrustCommand::TransitionWorkspace { member_run_id, .. } => Some(member_run_id),
            TrustCommand::ProvisionWorkspace { binding } => Some(&binding.member_run_id),
            _ => None,
        };
        if let Some(member_run_id) = own_run {
            if member_run_owned_by(store, execution_space_id, member_run_id, &actor.id)? {
                return Ok(());
            }
            let host_controls_team_run = store
                .trust_member_runs(execution_space_id)?
                .into_iter()
                .find(|run| run.id == *member_run_id)
                .and_then(|member_run| {
                    store
                        .team_runs()
                        .ok()?
                        .into_iter()
                        .find(|run| run.id == member_run.team_run_id)
                })
                .and_then(|team_run| store.latest_teams().ok()?.remove(&team_run.agent_team_id))
                .is_some_and(|team| team.host_agent_id == actor.id);
            if host_controls_team_run {
                return Ok(());
            }
            return Err(unauthorized(
                "member_run",
                member_run_id,
                "AgentMember may mutate only its own MemberRun or a MemberRun in a Team it exactly Hosts",
            ));
        }
    }

    Err(unauthorized(
        "command",
        command.name(),
        "authenticated actor is not authorized for this mutation",
    ))
}

fn result<T: Serialize>(
    mutation: CanonicalMutationResult<T>,
) -> Result<TrustCommandResult, StoreError> {
    Ok(TrustCommandResult {
        ok: true,
        protocol_version: MEMBER_TRUST_PROTOCOL_VERSION,
        projection: serde_json::to_value(mutation.projection)?,
        event_id: mutation.event.id,
        store_sequence: mutation.event.store_sequence,
        resulting_version: mutation.event.resulting_version,
        replayed: mutation.replayed,
    })
}

pub fn execute(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    command: TrustCommand,
) -> Result<TrustCommandResult, StoreError> {
    enforce_machine_scoped_service(store, &auth, &command)?;
    authorize(store, &auth.execution_space_id, &auth.actor, &command)?;
    let claimed_actor = match &command {
        TrustCommand::CreateAgentMember { member } => Some(&member.created_by),
        TrustCommand::CreateTeamMessage { message, .. } => Some(&message.sender),
        TrustCommand::CreateWorkReport { report, .. } => Some(&report.authored_by),
        TrustCommand::CreateWorkFinding { finding, .. } => Some(&finding.reported_by),
        TrustCommand::CreateFailureAnalysis { analysis, .. } => Some(&analysis.reported_by),
        TrustCommand::BindWorkModule { binding, .. } => Some(&binding.attached_by),
        TrustCommand::EvaluateGate { evaluation } => Some(&evaluation.performed_by),
        TrustCommand::WaiveGate { waiver } => Some(&waiver.performed_by_actor),
        _ => None,
    };
    if claimed_actor.is_some_and(|claimed| claimed != &auth.actor) {
        return Err(unauthorized(
            "authenticated_actor",
            &auth.actor.id,
            "request body actor claim does not match the transport-authenticated actor",
        ));
    }
    let requested_authority = match &command {
        TrustCommand::WaiveGate { waiver } => Some(waiver.authority_actor.clone()),
        TrustCommand::RevokeGateWaiver { waiver_id, .. } => store
            .trust_gate_waivers(&auth.execution_space_id)?
            .into_iter()
            .find(|waiver| waiver.id == *waiver_id)
            .map(|waiver| waiver.authority_actor),
        _ => None,
    };
    if requested_authority
        .as_ref()
        .is_some_and(|authority| !auth.authorized_authority_actors.contains(authority))
    {
        return Err(unauthorized(
            "authority_actor",
            requested_authority
                .as_ref()
                .map(|actor| actor.id.as_str())
                .unwrap_or("missing"),
            "credential is not bound to the requested authority actor",
        ));
    }
    let context = MutationContext {
        execution_space_id: auth.execution_space_id,
        authenticated_actor: auth.actor.clone(),
        authority_actor: requested_authority,
        command_name: command.name().to_string(),
        idempotency_key: auth.idempotency_key,
        expected_version: auth.expected_version,
        request_fingerprint: auth.request_fingerprint,
    };
    match command {
        TrustCommand::CreateAgentMember { mut member } => {
            member.created_by = auth.actor;
            result(store.create_trust_agent_member(&context, member)?)
        }
        TrustCommand::PauseAgentMember {
            member_id,
            updated_at,
        } => result(store.transition_trust_agent_member(
            &context,
            &member_id,
            AgentMemberOrganizationStatus::Paused,
            &updated_at,
        )?),
        TrustCommand::ResumeAgentMember {
            member_id,
            updated_at,
        } => result(store.transition_trust_agent_member(
            &context,
            &member_id,
            AgentMemberOrganizationStatus::Active,
            &updated_at,
        )?),
        TrustCommand::RetireAgentMember {
            member_id,
            updated_at,
        } => result(store.transition_trust_agent_member(
            &context,
            &member_id,
            AgentMemberOrganizationStatus::Retired,
            &updated_at,
        )?),
        TrustCommand::CreateMemberRun { run } => {
            result(store.create_trust_member_run(&context, run)?)
        }
        TrustCommand::CloseMemberRun {
            member_run_id,
            updated_at,
        } => result(store.transition_trust_member_run(
            &context,
            &member_run_id,
            MemberCoordinationStatus::Closed,
            &updated_at,
        )?),
        TrustCommand::ReopenMemberRun {
            member_run_id,
            updated_at,
        } => result(store.transition_trust_member_run(
            &context,
            &member_run_id,
            MemberCoordinationStatus::Active,
            &updated_at,
        )?),
        TrustCommand::RetireMemberRun {
            member_run_id,
            updated_at,
        } => result(store.transition_trust_member_run(
            &context,
            &member_run_id,
            MemberCoordinationStatus::Retired,
            &updated_at,
        )?),
        TrustCommand::ResumeNativeSession {
            member_run_id,
            updated_at,
        } => result(store.resume_trust_native_session(&context, &member_run_id, &updated_at)?),
        TrustCommand::CreateTeamMessage {
            mut message,
            updated_at,
        } => {
            message.sender = auth.actor;
            result(store.create_trust_team_message_with_deliveries(
                &context,
                message,
                &updated_at,
            )?)
        }
        TrustCommand::RetryMessageDelivery {
            delivery_id,
            updated_at,
        } => result(store.retry_trust_message_delivery(&context, &delivery_id, &updated_at)?),
        TrustCommand::ReconcileMessageDelivery {
            delivery_id,
            outcome,
            evidence_ref,
            updated_at,
        } => result(store.reconcile_trust_message_delivery(
            &context,
            &delivery_id,
            outcome,
            &evidence_ref,
            &updated_at,
        )?),
        TrustCommand::CreateWorkDeliveries {
            work_event_id,
            work_id,
            work_revision,
            recipient_member_run_ids,
            updated_at,
        } => result(store.create_trust_work_deliveries(
            &context,
            &work_event_id,
            &work_id,
            work_revision,
            &recipient_member_run_ids,
            &updated_at,
        )?),
        TrustCommand::RetryWorkDelivery {
            delivery_id,
            current_work_revision,
            updated_at,
        } => result(store.retry_trust_work_delivery(
            &context,
            &delivery_id,
            current_work_revision,
            &updated_at,
        )?),
        TrustCommand::ReconcileWorkDelivery {
            delivery_id,
            evidence_ref,
            updated_at,
        } => result(store.reconcile_trust_work_delivery(
            &context,
            &delivery_id,
            &evidence_ref,
            &updated_at,
        )?),
        TrustCommand::ProvisionWorkspace { mut binding } => {
            binding.created_by = auth.actor;
            result(store.create_trust_workspace_binding(&context, binding)?)
        }
        TrustCommand::TransitionWorkspace {
            member_run_id,
            binding_id,
            next,
            proof,
            updated_at,
        } => {
            let binding_matches_member = store
                .trust_workspace_bindings(&context.execution_space_id)?
                .into_iter()
                .any(|binding| binding.id == binding_id && binding.member_run_id == member_run_id);
            if !binding_matches_member {
                return Err(StoreError::Conflict(
                    serde_json::to_string(&harness_core::agentfirm_api::TrustError {
                        code:
                            harness_core::agentfirm_api::TrustErrorCode::WorkspaceGenerationFenced,
                        message: "workspace binding does not belong to the addressed MemberRun"
                            .into(),
                        retryable: false,
                        resource_kind: "workspace_binding".into(),
                        resource_id: binding_id,
                        current_version: None,
                    })
                    .expect("TrustError serializes"),
                ));
            }
            result(store.transition_trust_workspace_binding(
                &context,
                &binding_id,
                next,
                &proof,
                &updated_at,
            )?)
        }
        TrustCommand::CreateWorkReport {
            team_id,
            mut report,
        } => {
            report.authored_by = auth.actor;
            result(store.create_trust_work_report(&context, &team_id, report)?)
        }
        TrustCommand::CreateWorkFinding {
            team_id,
            mut finding,
        } => {
            finding.reported_by = auth.actor;
            result(store.create_trust_finding(&context, &team_id, finding)?)
        }
        TrustCommand::CreateFailureAnalysis {
            team_id,
            mut analysis,
        } => {
            analysis.reported_by = auth.actor;
            result(store.create_trust_failure_analysis(&context, &team_id, analysis)?)
        }
        TrustCommand::BindWorkModule {
            team_id,
            mut binding,
        } => {
            binding.attached_by = auth.actor;
            result(store.bind_trust_work_module(&context, &team_id, binding)?)
        }
        TrustCommand::CreateGateRequirement {
            team_id,
            requirement,
        } => result(store.create_trust_gate_requirement(&context, &team_id, requirement)?),
        TrustCommand::AcceptWork {
            team_id,
            work_id,
            work_report_id,
            candidate_fingerprint,
            updated_at,
        } => result(store.accept_trust_work(
            &context,
            &team_id,
            &work_id,
            &work_report_id,
            &candidate_fingerprint,
            &updated_at,
        )?),
        TrustCommand::EvaluateGate { mut evaluation } => {
            evaluation.performed_by = auth.actor;
            result(store.create_trust_gate_evaluation(&context, evaluation)?)
        }
        TrustCommand::WaiveGate { mut waiver } => {
            waiver.performed_by_actor = auth.actor;
            result(store.create_trust_gate_waiver(&context, waiver)?)
        }
        TrustCommand::RevokeGateWaiver {
            waiver_id,
            revoked_at,
        } => result(store.revoke_trust_gate_waiver(&context, &waiver_id, &revoked_at)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (std::path::PathBuf, HarnessStore) {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-auth-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create test root");
        let store = HarnessStore::new(&root);
        (root, store)
    }

    #[test]
    fn transport_actor_policy_is_fail_closed_and_self_scoped() {
        let (_root, store) = store();
        let external = ActorRef {
            kind: ActorKind::External,
            id: "external-1".into(),
        };
        let member = ActorRef {
            kind: ActorKind::AgentMember,
            id: "member-1".into(),
        };
        let service = ActorRef {
            kind: ActorKind::Service,
            id: "node-daemon".into(),
        };
        let privileged = TrustCommand::PauseAgentMember {
            member_id: "member-1".into(),
            updated_at: "unix-ms:1".into(),
        };
        assert!(authorize(&store, "space", &external, &privileged).is_err());
        assert!(authorize(&store, "space", &member, &privileged).is_err());
        assert!(authorize(&store, "space", &service, &privileged).is_ok());

        let self_control = TrustCommand::CloseMemberRun {
            member_run_id: "run-not-owned".into(),
            updated_at: "unix-ms:2".into(),
        };
        assert!(authorize(&store, "space", &member, &self_control).is_err());
    }

    #[test]
    fn transport_credential_cannot_claim_unbound_authority_and_has_zero_side_effects() {
        let (_root, store) = store();
        let actor = ActorRef {
            kind: ActorKind::Human,
            id: "operator-a".into(),
        };
        let allowed_authority = ActorRef {
            kind: ActorKind::Service,
            id: "review-board-a".into(),
        };
        let spoofed_authority = ActorRef {
            kind: ActorKind::Service,
            id: "review-board-b".into(),
        };
        let before = store.canonical_operations().expect("read operations").len();
        let error = execute(
            &store,
            AuthenticatedMutation {
                execution_space_id: "space".into(),
                actor: actor.clone(),
                authorized_authority_actors: vec![allowed_authority],
                idempotency_key: "waive-spoof".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            TrustCommand::WaiveGate {
                waiver: GateWaiver {
                    id: "waiver-spoof".into(),
                    requirement_id: "requirement-1".into(),
                    work_id: "work-1".into(),
                    work_revision: 1,
                    candidate_fingerprint: "sha256:candidate".into(),
                    authority_actor: spoofed_authority,
                    performed_by_actor: actor,
                    reason: "must not be accepted".into(),
                    evidence_refs: vec!["evidence://spoof".into()],
                    state: harness_core::agentfirm_api::GateWaiverState::Active,
                    version: 1,
                    created_at: "unix-ms:1".into(),
                    revoked_at: None,
                },
            },
        )
        .expect_err("credential may not expand its server-resolved authority set");
        let StoreError::Conflict(encoded) = error else {
            panic!("expected trust conflict")
        };
        let trust: harness_core::agentfirm_api::TrustError =
            serde_json::from_str(&encoded).expect("decode trust error");
        assert_eq!(
            trust.code,
            harness_core::agentfirm_api::TrustErrorCode::UnauthorizedActor
        );
        assert_eq!(
            store.canonical_operations().expect("read operations").len(),
            before,
            "rejected authority spoof must not append a canonical operation"
        );
    }
}
