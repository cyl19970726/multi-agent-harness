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
    path == "/v1/agent-members"
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
                    binding_id, next, ..
                },
                ["v1", "member-runs", _, "workspace", action],
            ) => {
                !binding_id.is_empty()
                    && matches!(
                        (next, *action),
                        (WorkspaceLifecycle::Attached, "attach")
                            | (WorkspaceLifecycle::Archived, "archive")
                            | (WorkspaceLifecycle::Removed, "cleanup")
                    )
            }
            (Self::CreateWorkReport { team_id, report }, ["v1", "teams", team, "works", work, "reports"]) => {
                team_id == team && &report.work_id == work
            }
            (
                Self::CreateWorkFinding { team_id, finding },
                ["v1", "teams", team, "works", work, "findings"],
            ) => team_id == team && &finding.work_id == work,
            (
                Self::CreateFailureAnalysis { team_id, analysis },
                ["v1", "teams", team, "works", work, "failure-analyses"],
            ) => team_id == team && &analysis.work_id == work,
            (Self::BindWorkModule { team_id, binding }, ["v1", "teams", team, "works", work, "modules"]) => {
                team_id == team && &binding.work_id == work
            }
            (
                Self::CreateGateRequirement { team_id, requirement },
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
    pub authority_actor: Option<ActorRef>,
    pub idempotency_key: String,
    pub expected_version: u64,
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
    let context = MutationContext {
        execution_space_id: auth.execution_space_id,
        authenticated_actor: auth.actor.clone(),
        authority_actor: auth.authority_actor,
        command_name: command.name().to_string(),
        idempotency_key: auth.idempotency_key,
        expected_version: auth.expected_version,
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
            binding_id,
            next,
            proof,
            updated_at,
        } => result(store.transition_trust_workspace_binding(
            &context,
            &binding_id,
            next,
            &proof,
            &updated_at,
        )?),
        TrustCommand::CreateWorkReport { team_id, mut report } => {
            report.authored_by = auth.actor;
            result(store.create_trust_work_report(&context, &team_id, report)?)
        }
        TrustCommand::CreateWorkFinding { team_id, mut finding } => {
            finding.reported_by = auth.actor;
            result(store.create_trust_finding(&context, &team_id, finding)?)
        }
        TrustCommand::CreateFailureAnalysis { team_id, mut analysis } => {
            analysis.reported_by = auth.actor;
            result(store.create_trust_failure_analysis(&context, &team_id, analysis)?)
        }
        TrustCommand::BindWorkModule { team_id, mut binding } => {
            binding.attached_by = auth.actor;
            result(store.bind_trust_work_module(&context, &team_id, binding)?)
        }
        TrustCommand::CreateGateRequirement { team_id, requirement } => {
            result(store.create_trust_gate_requirement(&context, &team_id, requirement)?)
        }
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
