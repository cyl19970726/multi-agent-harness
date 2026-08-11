//! Closed semantic action adapter for RoleView mutations.
//!
//! The browser may select an action and provide action-specific content, but
//! it never supplies actor identity, Host authority, CAS, idempotency, event
//! identity, or runtime identity. Those are bound from the authenticated
//! transport and the addressed Team/Work at this boundary.

use harness_core::agentfirm_api::{
    ActorKind, ActorRef, CandidateKind, CandidateRef, Confidence, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateRequirementSource, GateVerdict,
    GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberWorkspaceBinding, MessageKind,
    MutationContext, PrimaryCauseStatus, RetrySafety, RuntimeRecoveryResolution, WorkFinding,
    WorkFindingKind, WorkReport, WorkReportKind, WorkspaceLifecycle, WorkspaceMode,
    WorkspaceOwnership, WorkspaceSafetyProof,
};
use harness_core::{
    NodeDaemonLeaseStatus, TeamActorKind, TeamActorRef, Work, WorkCausationRef, WorkClaimMode,
    WorkCommandContext, WorkCondition, WorkPhase, WorkPriority,
};
use harness_store::{canonical_json_fingerprint, HarnessStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agentfirm_api::AuthenticatedMutation;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::enum_variant_names)]
enum RoleActionIntent {
    AcceptWork,
    CreateWork {
        work_id: String,
        title: String,
        #[serde(default)]
        context_markdown: String,
        completion_criteria_markdown: String,
        #[serde(default)]
        parent_work_id: Option<String>,
        #[serde(default)]
        eligible_member_ids: Vec<String>,
        #[serde(default)]
        prerequisite_work_ids: Vec<String>,
        #[serde(default = "default_claim_mode")]
        claim_mode: WorkClaimMode,
        #[serde(default = "default_priority")]
        priority: WorkPriority,
    },
    AssignWork {
        member_run_id: String,
    },
    RebindWork {
        member_run_id: String,
    },
    ReleaseWork,
    CancelWork {
        reason: String,
    },
    ClaimWork,
    StartWork,
    BlockWork {
        reason: String,
    },
    UnblockWork {
        resolution: String,
    },
    SubmitWork {
        result_summary: String,
        #[serde(default)]
        artifact_refs: Vec<String>,
        #[serde(default)]
        check_refs: Vec<String>,
        #[serde(default)]
        base_revision: Option<String>,
        #[serde(default)]
        candidate_revision: Option<String>,
    },
    ReviseWork {
        result_summary: String,
        #[serde(default)]
        artifact_refs: Vec<String>,
        #[serde(default)]
        check_refs: Vec<String>,
        #[serde(default)]
        base_revision: Option<String>,
        candidate_revision: String,
    },
    RequestChanges {
        reason: String,
    },
    SendMessage {
        recipient_ids: Vec<String>,
        body: String,
        #[serde(default)]
        work_id: Option<String>,
        #[serde(default)]
        evidence_refs: Vec<String>,
        #[serde(default)]
        response_required: bool,
    },
    ReplyMessage {
        recipient_ids: Vec<String>,
        body: String,
        correlation_id: String,
        causation_id: String,
        #[serde(default)]
        work_id: Option<String>,
        #[serde(default)]
        evidence_refs: Vec<String>,
        #[serde(default)]
        response_required: bool,
    },
    RequestDecision {
        body: String,
        #[serde(default)]
        work_id: Option<String>,
        #[serde(default)]
        evidence_refs: Vec<String>,
    },
    CloseMemberRun,
    ReopenMemberRun,
    RetireMemberRun,
    ResumeNativeSession,
    ProvisionWorkspace {
        project_binding_id: String,
        #[serde(default)]
        work_id: Option<String>,
        mode: WorkspaceMode,
        ownership: WorkspaceOwnership,
        canonical_root: String,
        #[serde(default)]
        base_ref: Option<String>,
    },
    AttachWorkspace,
    ArchiveWorkspace,
    CleanupWorkspace,
    WriteReport {
        summary: String,
        #[serde(default)]
        evidence_refs: Vec<String>,
        #[serde(default)]
        recommended_next_action: Option<String>,
    },
    WriteFinding {
        kind: WorkFindingKind,
        summary: String,
        detail_markdown: String,
        #[serde(default)]
        evidence_refs: Vec<String>,
        confidence: Confidence,
    },
    WriteFailure {
        observed_failure: String,
        impact: String,
        primary_cause_status: PrimaryCauseStatus,
        #[serde(default)]
        primary_cause: Option<String>,
        retry_safety: RetrySafety,
        recommended_host_decision: String,
        #[serde(default)]
        evidence_refs: Vec<String>,
        confidence: Confidence,
    },
    RequestGateEvaluation {
        gate_type: String,
        gate_contract_version: String,
        evaluator_ref: ActorRef,
        evaluator_version: String,
        #[serde(default)]
        resolved_config: serde_json::Value,
        #[serde(default = "default_true")]
        required: bool,
    },
    EvaluateGate {
        verdict: GateVerdict,
        summary: String,
        #[serde(default)]
        evidence_refs: Vec<String>,
    },
    WaiveGate {
        reason: String,
        #[serde(default)]
        evidence_refs: Vec<String>,
    },
    RevokeWaiver,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum OperatorActionIntent {
    ReconcileDelivery {
        evidence_ref: String,
    },
    ReconcileMessageDelivery {
        outcome: DeliveryReconcileOutcome,
        evidence_ref: String,
    },
    ResolveRuntimeRecovery {
        resolution: RuntimeRecoveryResolution,
        evidence_ref: String,
    },
    DaemonStart {
        #[serde(default = "default_daemon_concurrency")]
        max_concurrency: usize,
        daemon_generation: u64,
    },
    DaemonStop {
        daemon_generation: u64,
    },
    Diagnose,
    AdmitProvider {
        provider: String,
        execution_mode: String,
        eligibility_fingerprint: String,
    },
}

pub(crate) const OPERATOR_PROVIDER_ADMISSION_TUPLES: [(&str, &str); 4] = [
    ("codex", "codex_app_server"),
    ("claude", "claude_agent_sdk"),
    ("kimi", "kimi_acp"),
    ("pi", "pi_rpc"),
];

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProviderAdmissionActionBinding {
    pub provider: String,
    pub execution_mode: String,
    pub eligibility: &'static str,
    pub eligibility_fingerprint: String,
    pub project_binding_id: String,
    pub source_store_identity: String,
    pub registration_identity: String,
    pub registration_revision: u64,
    #[serde(skip_serializing)]
    pub disabled_reason: Option<String>,
}

pub(crate) fn provider_admission_action_binding(
    store: &HarnessStore,
    execution_space_id: &str,
    node_id: &str,
    node_revision: u64,
    provider: &str,
    execution_mode: &str,
) -> ProviderAdmissionActionBinding {
    let registered = OPERATOR_PROVIDER_ADMISSION_TUPLES.contains(&(provider, execution_mode));
    let scope = store.provider_compatibility_scope();
    let source_store_identity = std::fs::canonicalize(store.root())
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    let scoped_project_id = scope.map(|(project_id, _)| project_id).unwrap_or("");
    let active_registration = store
        .latest_node_project_registrations()
        .unwrap_or_default()
        .into_iter()
        .find(|registration| {
            registration.node_id == node_id
                && registration.execution_space_id == execution_space_id
                && registration.project_binding_id == scoped_project_id
                && registration.status == harness_core::NodeProjectRegistrationStatus::Active
        });
    let scope_reason = if !registered {
        Some("provider/execution-mode tuple is not in the server admission registry".to_string())
    } else if scope.is_none() {
        Some("canonical provider compatibility scope is unavailable".to_string())
    } else if active_registration.as_ref().is_none_or(|registration| {
        scope.is_none_or(|(project_id, _)| registration.project_binding_id != project_id)
    }) {
        Some("exact Node/project/Execution Space admission scope is unavailable".to_string())
    } else {
        None
    };
    let probe = scope_reason.as_ref().map_or_else(
        || crate::operator_provider_admission_probe(provider, execution_mode),
        |reason| Err(reason.clone()),
    );
    let (eligibility, disabled_reason, provider_version, adapter_contract_version) = match probe {
        Ok((version, contract)) => ("eligible", None, Some(version), Some(contract)),
        Err(reason) => ("disabled", Some(reason), None, None),
    };
    let (project_id, store_id) = scope.unwrap_or(("", ""));
    let registration_identity = format!("{node_id}:{execution_space_id}:{project_id}");
    let registration_revision = store
        .node_project_registration_revision(node_id, execution_space_id, project_id)
        .unwrap_or_default();
    let eligibility_fingerprint = canonical_json_fingerprint(&json!({
        "protocol":"agentfirm.provider_admission.action.v1",
        "execution_space_id":execution_space_id,
        "node_id":node_id,
        "node_revision":node_revision,
        "project_id":project_id,
        "store_id":store_id,
        "source_store_identity":source_store_identity,
        "registration_identity":registration_identity,
        "registration_revision":registration_revision,
        "provider":provider,
        "execution_mode":execution_mode,
        "eligibility":eligibility,
        "provider_version":provider_version,
        "adapter_contract_version":adapter_contract_version,
        "disabled_reason":disabled_reason,
    }));
    ProviderAdmissionActionBinding {
        provider: provider.to_string(),
        execution_mode: execution_mode.to_string(),
        eligibility,
        eligibility_fingerprint,
        project_binding_id: project_id.to_string(),
        source_store_identity,
        registration_identity,
        registration_revision,
        disabled_reason,
    }
}

fn role_action_scope(
    store: &HarnessStore,
    execution_space_id: &str,
    path: &str,
) -> serde_json::Value {
    let provider_node_id = match parse_canonical_route(path) {
        Some(CanonicalRoute::Operator {
            node_id,
            operation: "provider-admission",
        }) => Some(node_id),
        _ => None,
    };
    let Some(node_id) = provider_node_id else {
        // Daemon lifecycle, diagnostics and delivery reconciliation are Node /
        // Execution-Space operations. They intentionally do not acquire a
        // Project Binding merely because the HTTP request carried a selector.
        return serde_json::Value::Null;
    };
    let source_store_identity = std::fs::canonicalize(store.root())
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    let (project_binding_id, store_scope_id) =
        store.provider_compatibility_scope().unwrap_or(("", ""));
    let registration_identity = format!("{node_id}:{execution_space_id}:{project_binding_id}");
    let registration_revision = store
        .node_project_registration_revision(node_id, execution_space_id, project_binding_id)
        .unwrap_or_default();
    let registration_active = store
        .latest_node_project_registrations()
        .unwrap_or_default()
        .into_iter()
        .any(|registration| {
            registration.node_id == node_id
                && registration.execution_space_id == execution_space_id
                && registration.project_binding_id == project_binding_id
                && registration.status == harness_core::NodeProjectRegistrationStatus::Active
        });
    json!({
        "project_binding_id": project_binding_id,
        "source_store_identity": source_store_identity,
        "store_scope_id": store_scope_id,
        "registration_identity": registration_identity,
        "registration_revision": registration_revision,
        "registration_active": registration_active,
    })
}

fn default_true() -> bool {
    true
}
fn default_daemon_concurrency() -> usize {
    4
}

fn default_claim_mode() -> WorkClaimMode {
    WorkClaimMode::HostAssign
}

fn default_priority() -> WorkPriority {
    WorkPriority::Normal
}

#[derive(Debug, Serialize)]
pub struct RoleActionResult {
    pub ok: bool,
    pub action_protocol_version: &'static str,
    pub projection: serde_json::Value,
    pub event_id: String,
    pub resulting_version: u64,
    pub store_sequence: u64,
    pub replayed: bool,
}

fn canonical_report_count(
    store: &HarnessStore,
    space_id: &str,
    work_id: &str,
) -> Result<u64, StoreError> {
    Ok(store
        .canonical_operations_for_space(space_id)?
        .into_iter()
        .filter(|operation| {
            operation.event.aggregate_kind == "work_report"
                && operation.resulting_projection["work_id"] == work_id
        })
        .count() as u64)
}

#[derive(Debug)]
struct Route<'a> {
    team_run_id: &'a str,
    work_id: Option<&'a str>,
    operation: &'a str,
}

fn parse_route(path: &str) -> Option<Route<'_>> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "team-runs", run_id, "works"] => Some(Route {
            team_run_id: run_id,
            work_id: None,
            operation: "create",
        }),
        ["v1", "agentfirm", "team-runs", run_id, "works", work_id, operation]
            if matches!(
                *operation,
                "assign"
                    | "rebind"
                    | "release"
                    | "cancel"
                    | "claim"
                    | "start"
                    | "block"
                    | "resume"
                    | "submit"
            ) =>
        {
            Some(Route {
                team_run_id: run_id,
                work_id: Some(work_id),
                operation,
            })
        }
        _ => None,
    }
}

fn parse_accept_route(path: &str) -> Option<(&str, &str)> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "teams", team_id, "works", work_id, "accept"] => {
            Some((team_id, work_id))
        }
        _ => None,
    }
}

fn parse_operator_route(path: &str) -> Option<(&str, &str)> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "nodes", node_id, "work-deliveries", delivery_id, "reconcile"] => {
            Some((node_id, delivery_id))
        }
        _ => None,
    }
}

#[derive(Debug)]
enum CanonicalRoute<'a> {
    Message {
        team_run_id: &'a str,
        operation: &'a str,
    },
    MemberRun {
        member_run_id: &'a str,
        operation: &'a str,
    },
    Workspace {
        member_run_id: &'a str,
        operation: &'a str,
    },
    WorkRecord {
        team_id: &'a str,
        work_id: &'a str,
        operation: &'a str,
    },
    Gate {
        requirement_id: &'a str,
        operation: &'a str,
    },
    Waiver {
        waiver_id: &'a str,
    },
    MessageDelivery {
        node_id: &'a str,
        delivery_id: &'a str,
    },
    RuntimeRecovery {
        node_id: &'a str,
        command_id: &'a str,
    },
    Operator {
        node_id: &'a str,
        operation: &'a str,
    },
}

fn parse_canonical_route(path: &str) -> Option<CanonicalRoute<'_>> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "team-runs", run, "messages", operation]
            if matches!(*operation, "send" | "reply" | "request-decision") =>
        {
            Some(CanonicalRoute::Message {
                team_run_id: run,
                operation,
            })
        }
        ["v1", "agentfirm", "member-runs", run, operation]
            if matches!(
                *operation,
                "close" | "reopen" | "retire" | "resume-native-session"
            ) =>
        {
            Some(CanonicalRoute::MemberRun {
                member_run_id: run,
                operation,
            })
        }
        ["v1", "agentfirm", "member-runs", run, "workspace", operation]
            if matches!(*operation, "provision" | "attach" | "archive" | "cleanup") =>
        {
            Some(CanonicalRoute::Workspace {
                member_run_id: run,
                operation,
            })
        }
        ["v1", "agentfirm", "teams", team, "works", work, operation]
            if matches!(
                *operation,
                "request-changes"
                    | "revise"
                    | "reports"
                    | "findings"
                    | "failure-analyses"
                    | "gate-requirements"
            ) =>
        {
            Some(CanonicalRoute::WorkRecord {
                team_id: team,
                work_id: work,
                operation,
            })
        }
        ["v1", "agentfirm", "gate-requirements", requirement, operation]
            if matches!(*operation, "evaluate" | "waive") =>
        {
            Some(CanonicalRoute::Gate {
                requirement_id: requirement,
                operation,
            })
        }
        ["v1", "agentfirm", "gate-waivers", waiver, "revoke"] => {
            Some(CanonicalRoute::Waiver { waiver_id: waiver })
        }
        ["v1", "agentfirm", "nodes", node, "message-deliveries", delivery, "reconcile"] => {
            Some(CanonicalRoute::MessageDelivery {
                node_id: node,
                delivery_id: delivery,
            })
        }
        ["v1", "agentfirm", "nodes", node, "runtime-commands", command, "resolve"] => {
            Some(CanonicalRoute::RuntimeRecovery {
                node_id: node,
                command_id: command,
            })
        }
        ["v1", "agentfirm", "nodes", node, operation]
            if matches!(
                *operation,
                "daemon-start" | "daemon-stop" | "diagnostics" | "provider-admission"
            ) =>
        {
            Some(CanonicalRoute::Operator {
                node_id: node,
                operation,
            })
        }
        _ => None,
    }
}

pub fn is_http_mutation_path(path: &str) -> bool {
    parse_route(path).is_some()
        || parse_accept_route(path).is_some()
        || parse_operator_route(path).is_some()
        || parse_canonical_route(path).is_some()
}

/// Old Work and WorkDelegation HTTP writers were body-authorized and are not
/// safe compatibility aliases. They remain explicit 410 surfaces so no caller
/// can accidentally retain a second mutation authority.
pub fn is_retired_legacy_write_path(path: &str) -> bool {
    if path == "/v1/work-delegations" || path.starts_with("/v1/work-delegations/") {
        return true;
    }
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        ["v1", "team-runs", _, "works"] | ["v1", "team-runs", _, "works", _, _]
    )
}

fn encoded_error(
    code: &str,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
    current_version: Option<u64>,
) -> StoreError {
    StoreError::Conflict(
        serde_json::to_string(&json!({
            "code": code,
            "message": message.into(),
            "retryable": false,
            "resource_kind": resource_kind,
            "resource_id": resource_id,
            "current_version": current_version,
        }))
        .expect("role action error serializes"),
    )
}

fn now_string() -> String {
    crate::role_views_api::now()
}

fn host_context(
    auth: &AuthenticatedMutation,
    host_id: &str,
    duplicate_ok: bool,
) -> WorkCommandContext {
    WorkCommandContext {
        event_id: format!("role-action:{}", auth.idempotency_key),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_id.to_string(),
            display_name: None,
            authn_source: Some("agentfirm_http_credential".into()),
        },
        authority_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: host_id.to_string(),
            display_name: None,
            authn_source: Some("agentfirm_http_credential".into()),
        }),
        causation_ref: auth
            .request_fingerprint
            .as_ref()
            .map(|id| WorkCausationRef {
                kind: "agentfirm.role_action.v1".into(),
                id: id.clone(),
            }),
        idempotency_key: auth.idempotency_key.clone(),
        created_at: now_string(),
        duplicate_ok,
    }
}

fn member_context(auth: &AuthenticatedMutation, member_run_id: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: format!("role-action:{}", auth.idempotency_key),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member_run_id.to_string(),
            display_name: None,
            authn_source: Some("agentfirm_http_credential".into()),
        },
        authority_actor: None,
        causation_ref: auth
            .request_fingerprint
            .as_ref()
            .map(|id| WorkCausationRef {
                kind: "agentfirm.role_action.v1".into(),
                id: id.clone(),
            }),
        idempotency_key: auth.idempotency_key.clone(),
        created_at: now_string(),
        duplicate_ok: false,
    }
}

fn team_for_run(
    store: &HarnessStore,
    team_run_id: &str,
) -> Result<(harness_core::AgentTeamRun, harness_core::AgentTeam), StoreError> {
    let run = store
        .team_runs()?
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "TeamRun does not exist",
                "team_run",
                team_run_id,
                None,
            )
        })?;
    let team = store
        .latest_teams()?
        .remove(&run.agent_team_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "TeamRun has no current AgentTeam",
                "team_run",
                team_run_id,
                None,
            )
        })?;
    Ok((run, team))
}

fn is_host(auth: &AuthenticatedMutation, host_id: &str) -> bool {
    (auth.actor.kind == ActorKind::AgentMember && auth.actor.id == host_id)
        || auth
            .authorized_authority_actors
            .iter()
            .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == host_id)
}

fn require_host<'a>(
    auth: &AuthenticatedMutation,
    host_id: &'a str,
    resource_kind: &str,
    resource_id: &str,
) -> Result<&'a str, StoreError> {
    is_host(auth, host_id).then_some(host_id).ok_or_else(|| {
        encoded_error(
            "UNAUTHORIZED_ACTOR",
            "credential is not bound to this Team's exact Host authority",
            resource_kind,
            resource_id,
            None,
        )
    })
}

fn resolve_member_run(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    team_run_id: &str,
) -> Result<String, StoreError> {
    if auth.actor.kind != ActorKind::AgentMember {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "this action requires an authenticated AgentMember",
            "team_run",
            team_run_id,
            None,
        ));
    }
    let mut runs = store
        .trust_member_runs(&auth.execution_space_id)?
        .into_iter()
        .filter(|run| {
            run.agent_member_id == auth.actor.id
                && run.team_run_id == team_run_id
                && run.coordination_status == MemberCoordinationStatus::Active
        })
        .collect::<Vec<_>>();
    if runs.len() != 1 {
        return Err(encoded_error(
            "IDENTITY_CONFLICT",
            format!(
                "expected exactly one active MemberRun for this actor and TeamRun; found {}",
                runs.len()
            ),
            "team_run",
            team_run_id,
            None,
        ));
    }
    Ok(runs.remove(0).id)
}

fn require_exact_work_member(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    work: &Work,
) -> Result<String, StoreError> {
    let member_run_id = resolve_member_run(store, auth, &work.team_run_id)?;
    if work.owner_member_id.as_deref() != Some(auth.actor.id.as_str())
        || work.active_member_run_id.as_deref() != Some(member_run_id.as_str())
    {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "member-owned Work mutation requires the exact accountable AgentMember and current active WorkExecutionBinding",
            "work",
            &work.id,
            Some(work.version),
        ));
    }
    Ok(member_run_id)
}

fn current_work(
    store: &HarnessStore,
    team_run_id: &str,
    work_id: &str,
) -> Result<Work, StoreError> {
    let work = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == work_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "Work does not exist",
                "work",
                work_id,
                None,
            )
        })?;
    if work.team_run_id != team_run_id {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Work does not belong to the addressed TeamRun",
            "work",
            work_id,
            Some(work.version),
        ));
    }
    Ok(work)
}

fn current_canonical_work(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
) -> Result<Work, StoreError> {
    let mut current = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == work_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "Work does not exist",
                "work",
                work_id,
                None,
            )
        })?;
    for operation in store.canonical_operations_for_space(execution_space_id)? {
        let candidates = std::iter::once(&operation.resulting_projection)
            .chain(operation.immutable_side_records.iter());
        for candidate in candidates {
            if let Ok(work) = serde_json::from_value::<Work>(candidate.clone()) {
                if work.id == work_id && work.version >= current.version {
                    current = work;
                }
            }
        }
    }
    Ok(current)
}

fn require_confirmed(
    action: &str,
    confirmed_action: Option<&str>,
    resource_id: &str,
) -> Result<(), StoreError> {
    if matches!(action, "cancel") && confirmed_action != Some(action) {
        return Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            format!("server confirmation header must exactly confirm {action}"),
            "work",
            resource_id,
            None,
        ));
    }
    Ok(())
}

fn trust_result(result: crate::agentfirm_api::TrustCommandResult) -> RoleActionResult {
    RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: result.projection,
        event_id: result.event_id,
        resulting_version: result.resulting_version,
        store_sequence: result.store_sequence,
        replayed: result.replayed,
    }
}

fn canonical_mutation_result<T: Serialize>(
    result: harness_store::CanonicalMutationResult<T>,
) -> Result<RoleActionResult, StoreError> {
    let projection = serde_json::to_value(result.projection)?;
    let resulting_version = projection
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(result.event.resulting_version);
    Ok(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection,
        event_id: result.event.id,
        resulting_version,
        store_sequence: result.event.store_sequence,
        replayed: result.replayed,
    })
}

fn deterministic_id(kind: &str, auth: &AuthenticatedMutation) -> String {
    format!("{kind}:{}", auth.idempotency_key)
}

fn canonical_replay(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    aggregate_kind: &str,
    aggregate_id: &str,
) -> Result<Option<RoleActionResult>, StoreError> {
    let Some(operation) = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    else {
        return Ok(None);
    };
    let fingerprint_matches = auth.request_fingerprint.as_deref()
        == Some(operation.event.canonical_request_fingerprint.as_str());
    if operation.event.aggregate_kind != aggregate_kind
        || operation.event.aggregate_id != aggregate_id
        || operation.event.performed_by_actor != auth.actor
        || !fingerprint_matches
    {
        return Err(encoded_error(
            "IDEMPOTENCY_KEY_REUSED",
            "idempotency key is already bound to a different authenticated semantic action",
            aggregate_kind,
            aggregate_id,
            Some(operation.event.resulting_version),
        ));
    }
    Ok(Some(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: operation.resulting_projection,
        event_id: operation.event.id,
        resulting_version: operation.event.resulting_version,
        store_sequence: operation.event.store_sequence,
        replayed: true,
    }))
}

fn work_replay(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    work_id: &str,
    kind: harness_core::WorkEventKind,
) -> Result<Option<RoleActionResult>, StoreError> {
    let operations = store.work_operations()?;
    let Some(operation) = operations
        .iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    else {
        return Ok(None);
    };
    let fingerprint_matches = auth.request_fingerprint.as_deref()
        == operation
            .event
            .causation_ref
            .as_ref()
            .filter(|reference| reference.kind == "agentfirm.role_action.v1")
            .map(|reference| reference.id.as_str());
    if operation.event.work_id != work_id
        || operation.event.kind != kind
        || operation.event.expected_version != auth.expected_version
        || !fingerprint_matches
    {
        return Err(encoded_error(
            "IDEMPOTENCY_KEY_REUSED",
            "idempotency key is already bound to a different authenticated Work action",
            "work",
            work_id,
            Some(operation.event.resulting_version),
        ));
    }
    Ok(Some(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: serde_json::to_value(&operation.work)?,
        event_id: operation.event.id.clone(),
        resulting_version: operation.event.resulting_version,
        store_sequence: operations.len() as u64,
        replayed: true,
    }))
}

fn active_member_run(
    store: &HarnessStore,
    space_id: &str,
    member_run_id: &str,
) -> Result<harness_core::agentfirm_api::MemberRun, StoreError> {
    store
        .trust_member_runs(space_id)?
        .into_iter()
        .find(|run| run.id == member_run_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "MemberRun does not exist",
                "member_run",
                member_run_id,
                None,
            )
        })
}

fn team_for_member_run(
    store: &HarnessStore,
    space_id: &str,
    member_run_id: &str,
) -> Result<
    (
        harness_core::agentfirm_api::MemberRun,
        harness_core::AgentTeamRun,
        harness_core::AgentTeam,
    ),
    StoreError,
> {
    let member_run = active_member_run(store, space_id, member_run_id)?;
    let (team_run, team) = team_for_run(store, &member_run.team_run_id)?;
    Ok((member_run, team_run, team))
}

fn require_member_or_host(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    member_run_id: &str,
) -> Result<
    (
        harness_core::agentfirm_api::MemberRun,
        harness_core::AgentTeam,
    ),
    StoreError,
> {
    let (run, _, team) = team_for_member_run(store, &auth.execution_space_id, member_run_id)?;
    let own = auth.actor.kind == ActorKind::AgentMember && auth.actor.id == run.agent_member_id;
    if !own && !is_host(auth, &team.host_agent_id) {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "credential is neither this MemberRun's AgentMember nor its exact Team Host",
            "member_run",
            member_run_id,
            Some(run.version),
        ));
    }
    Ok((run, team))
}

fn latest_workspace(
    store: &HarnessStore,
    space_id: &str,
    member_run_id: &str,
) -> Result<MemberWorkspaceBinding, StoreError> {
    store
        .trust_workspace_bindings(space_id)?
        .into_iter()
        .filter(|binding| binding.member_run_id == member_run_id)
        .max_by_key(|binding| binding.version)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "MemberRun has no Workspace binding",
                "member_run",
                member_run_id,
                None,
            )
        })
}

fn observe_workspace_proof(
    binding: &MemberWorkspaceBinding,
    member_generation: u64,
) -> Result<WorkspaceSafetyProof, StoreError> {
    let root = std::path::Path::new(&binding.canonical_root);
    let canonical = std::fs::canonicalize(root).map_err(|error| {
        encoded_error(
            "WORKSPACE_UNSAFE",
            format!("workspace root cannot be canonicalized: {error}"),
            "workspace_binding",
            &binding.id,
            Some(binding.version),
        )
    })?;
    fn link_safe(root: &std::path::Path, current: &std::path::Path) -> std::io::Result<bool> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                let target = std::fs::canonicalize(&path)?;
                if !target.starts_with(root) {
                    return Ok(false);
                }
            } else if metadata.is_dir() && entry.file_name() != ".git" && !link_safe(root, &path)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    let link_escape_free = link_safe(&canonical, &canonical).map_err(|error| {
        encoded_error(
            "WORKSPACE_UNSAFE",
            format!("workspace link boundary cannot be observed: {error}"),
            "workspace_binding",
            &binding.id,
            Some(binding.version),
        )
    })?;
    if !link_escape_free {
        return Err(encoded_error(
            "WORKSPACE_UNSAFE",
            "workspace contains a symlink escaping its canonical root",
            "workspace_binding",
            &binding.id,
            Some(binding.version),
        ));
    }
    let git_common_dir = std::process::Command::new("git")
        .args([
            "-C",
            &binding.canonical_root,
            "rev-parse",
            "--git-common-dir",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .and_then(|value| {
            let path = std::path::PathBuf::from(value);
            std::fs::canonicalize(if path.is_absolute() {
                path
            } else {
                canonical.join(path)
            })
            .ok()
        })
        .map(|path| path.to_string_lossy().into_owned());
    let status = std::process::Command::new("git")
        .args(["-C", &binding.canonical_root, "status", "--porcelain=v1"])
        .output()
        .ok();
    let status = status
        .filter(|output| output.status.success())
        .ok_or_else(|| {
            encoded_error(
                "WORKSPACE_UNSAFE",
                "workspace git status cannot be observed",
                "workspace_binding",
                &binding.id,
                Some(binding.version),
            )
        })?;
    let is_dirty = !status.stdout.is_empty();
    let conflict_output = std::process::Command::new("git")
        .args([
            "-C",
            &binding.canonical_root,
            "diff",
            "--name-only",
            "--diff-filter=U",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .ok_or_else(|| {
            encoded_error(
                "WORKSPACE_UNSAFE",
                "workspace conflict state cannot be observed",
                "workspace_binding",
                &binding.id,
                Some(binding.version),
            )
        })?;
    let is_conflicted = !conflict_output.stdout.is_empty();
    let repository_matches = git_common_dir.is_some();
    let proof = WorkspaceSafetyProof {
        canonical_root: canonical.to_string_lossy().into_owned(),
        project_binding_id: binding.project_binding_id.clone(),
        git_common_dir,
        link_escape_free,
        repository_matches,
        is_dirty,
        is_conflicted,
        observed_member_generation: member_generation,
    };
    if !proof.repository_matches {
        return Err(encoded_error(
            "WORKSPACE_UNSAFE",
            "workspace repository identity cannot be established",
            "workspace_binding",
            &binding.id,
            Some(binding.version),
        ));
    }
    Ok(proof)
}

fn execute_canonical_role_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    route: CanonicalRoute<'_>,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    match route {
        CanonicalRoute::Message {
            team_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid message intent: {error}"),
                    "team_run",
                    team_run_id,
                    None,
                )
            })?;
            let (_run, team) = team_for_run(store, team_run_id)?;
            let actor_is_host = is_host(&auth, &team.host_agent_id);
            let actor_member_run = resolve_member_run(store, &auth, team_run_id).ok();
            if !actor_is_host && actor_member_run.is_none() {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "message sender must be the exact Team Host or one active Team Member",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let team_revision = store
                .teams()?
                .into_iter()
                .filter(|candidate| candidate.id == team.id)
                .count() as u64;
            if auth.expected_version != team_revision {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Team Message requires the exact current Team revision",
                    "team",
                    &team.id,
                    Some(team_revision),
                ));
            }
            let (
                recipient_ids,
                message_body,
                work_id,
                evidence_refs,
                response_required,
                correlation_id,
                causation_id,
                message_kind,
            ) = match (operation, intent) {
                (
                    "send",
                    RoleActionIntent::SendMessage {
                        recipient_ids,
                        body,
                        work_id,
                        evidence_refs,
                        response_required,
                    },
                ) => (
                    recipient_ids,
                    body,
                    work_id,
                    evidence_refs,
                    response_required,
                    deterministic_id("correlation", &auth),
                    None,
                    MessageKind::Message,
                ),
                (
                    "reply",
                    RoleActionIntent::ReplyMessage {
                        recipient_ids,
                        body,
                        correlation_id,
                        causation_id,
                        work_id,
                        evidence_refs,
                        response_required,
                    },
                ) => (
                    recipient_ids,
                    body,
                    work_id,
                    evidence_refs,
                    response_required,
                    correlation_id,
                    Some(causation_id),
                    MessageKind::Reply,
                ),
                (
                    "request-decision",
                    RoleActionIntent::RequestDecision {
                        body,
                        work_id,
                        evidence_refs,
                    },
                ) => (
                    vec![team.host_agent_id.clone()],
                    body,
                    work_id,
                    evidence_refs,
                    true,
                    deterministic_id("decision", &auth),
                    None,
                    MessageKind::RequestDecision,
                ),
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match message route",
                        "team_run",
                        team_run_id,
                        None,
                    ))
                }
            };
            if let Some(work_id) = work_id.as_deref() {
                let work = current_work(store, team_run_id, work_id)?;
                if !actor_is_host {
                    require_exact_work_member(store, &auth, &work)?;
                }
            }
            if message_body.trim().is_empty() || recipient_ids.is_empty() {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "message body and recipients are required",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let allowed = team
                .member_ids
                .iter()
                .chain(std::iter::once(&team.host_agent_id))
                .collect::<std::collections::BTreeSet<_>>();
            if recipient_ids.iter().any(|id| !allowed.contains(id)) {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "every message recipient must belong to the exact Team",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let memberships = store.fabric_team_memberships(&auth.execution_space_id)?;
            let subscriptions = store.fabric_message_subscriptions(&auth.execution_space_id)?;
            for recipient_id in &recipient_ids {
                let matching = memberships
                    .iter()
                    .filter(|membership| {
                        membership.team_id == team.id
                            && membership.agent_identity_id == *recipient_id
                            && membership.state
                                == harness_core::agentfirm_api::TeamMembershipStatus::Active
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1
                    || !subscriptions.iter().any(|subscription| {
                        subscription.subscriber_agent_id == *recipient_id
                            && subscription.membership_ref.as_deref()
                                == Some(matching[0].id.as_str())
                            && subscription.status
                                == harness_core::agentfirm_api::MessageSubscriptionStatus::Active
                    })
                {
                    return Err(encoded_error(
                        "MESSAGE_ROUTE_UNAVAILABLE",
                        "recipient requires one active canonical TeamMembership and MessageSubscription",
                        "agent_identity",
                        recipient_id,
                        None,
                    ));
                }
            }
            let member_runs = store
                .trust_member_runs(&auth.execution_space_id)?
                .into_iter()
                .filter(|run| run.team_run_id == team_run_id)
                .collect::<Vec<_>>();
            let recipient_runtime_ids = recipient_ids
                .into_iter()
                .map(|identity_id| {
                    if identity_id == team.host_agent_id {
                        Ok("host".to_string())
                    } else {
                        let matching = member_runs
                            .iter()
                            .filter(|run| {
                                run.agent_member_id == identity_id
                                    && run.coordination_status == MemberCoordinationStatus::Active
                            })
                            .collect::<Vec<_>>();
                        match matching.as_slice() {
                            [run] => Ok(run.id.clone()),
                            _ => Err(encoded_error(
                                "AGENT_SESSION_AMBIGUOUS",
                                "message recipient requires exactly one active Team Member",
                                "agent_identity",
                                &identity_id,
                                None,
                            )),
                        }
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let sender = TeamActorRef {
                kind: if actor_is_host {
                    TeamActorKind::Host
                } else {
                    TeamActorKind::AgentMember
                },
                id: if actor_is_host {
                    team.host_agent_id.clone()
                } else {
                    auth.actor.id.clone()
                },
                display_name: None,
                authn_source: Some("agentfirm_http_credential".into()),
            };
            let compatibility_id = format!(
                "role-message:{}",
                canonical_json_fingerprint(&json!({
                    "actor": &auth.actor,
                    "idempotency_key": &auth.idempotency_key,
                }))
            );
            let message = harness_core::TeamMessageProjection {
                id: compatibility_id.clone(),
                team_run_id: team_run_id.to_string(),
                work_id,
                source_plan_ref: None,
                sender: Some(sender.clone()),
                sender_runtime_id: if actor_is_host {
                    "host".into()
                } else {
                    actor_member_run
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| auth.actor.id.clone())
                },
                recipients: recipient_runtime_ids
                    .iter()
                    .map(|id| harness_core::TeamRecipientRef {
                        kind: harness_core::TeamRecipientKind::ProviderRuntimeProjection,
                        id: id.clone(),
                    })
                    .collect(),
                recipient_runtime_ids,
                kind: match message_kind {
                    MessageKind::RequestDecision => harness_core::ProviderDispatchIntent::Control,
                    _ => harness_core::ProviderDispatchIntent::Message,
                },
                body: message_body,
                correlation_id,
                causation_id,
                response_intent: Some(if response_required {
                    harness_core::ProviderResponseIntent::ResponseRequired
                } else {
                    harness_core::ProviderResponseIntent::Informational
                }),
                evidence_refs,
                deliveries: Vec::new(),
                created_at: now_string(),
            };
            let canonical_id = format!("message:{compatibility_id}");
            let replayed = store
                .fabric_messages(&auth.execution_space_id)?
                .iter()
                .any(|message| message.id == canonical_id);
            let published =
                crate::publish_team_message(store, &sender, message).map_err(|error| {
                    encoded_error(
                        "RUNTIME_COMMAND_REJECTED",
                        error.to_string(),
                        "message",
                        &canonical_id,
                        None,
                    )
                })?;
            let canonical = store
                .fabric_messages(&auth.execution_space_id)?
                .into_iter()
                .find(|message| message.id == published.id)
                .ok_or_else(|| {
                    encoded_error(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                        "NodeDaemon returned without a canonical Message",
                        "message",
                        &canonical_id,
                        None,
                    )
                })?;
            let event = store
                .canonical_operations_for_space(&auth.execution_space_id)?
                .into_iter()
                .filter(|operation| {
                    operation.event.aggregate_kind == "message"
                        && operation.event.aggregate_id == canonical.id
                })
                .max_by_key(|operation| operation.event.sequence)
                .ok_or_else(|| {
                    encoded_error(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                        "canonical Message event is missing",
                        "message",
                        &canonical.id,
                        None,
                    )
                })?
                .event;
            Ok(RoleActionResult {
                ok: true,
                action_protocol_version: "agentfirm.role_actions.v1",
                projection: serde_json::to_value(canonical)?,
                event_id: event.id,
                resulting_version: event.resulting_version,
                store_sequence: event.store_sequence,
                replayed,
            })
        }
        CanonicalRoute::MemberRun {
            member_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid MemberRun intent: {error}"),
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
            let (run, _) = require_member_or_host(store, &auth, member_run_id)?;
            let required_confirmation = match operation {
                "close" => Some("close_member_run"),
                "retire" => Some("retire_member_run"),
                _ => None,
            };
            if required_confirmation.is_some_and(|required| confirmed_action != Some(required)) {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    format!(
                        "server confirmation must exactly confirm {}",
                        required_confirmation.unwrap_or_default()
                    ),
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            if let Some(replay) = canonical_replay(store, &auth, "member_run", member_run_id)? {
                return Ok(replay);
            }
            if auth.expected_version != run.version {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "MemberRun action requires its exact current revision",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            let command = match (operation, intent) {
                ("close", RoleActionIntent::CloseMemberRun) => {
                    crate::agentfirm_api::TrustCommand::CloseMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("reopen", RoleActionIntent::ReopenMemberRun) => {
                    crate::agentfirm_api::TrustCommand::ReopenMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("retire", RoleActionIntent::RetireMemberRun) => {
                    crate::agentfirm_api::TrustCommand::RetireMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("resume-native-session", RoleActionIntent::ResumeNativeSession) => {
                    crate::agentfirm_api::TrustCommand::ResumeNativeSession {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match MemberRun route",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ))
                }
            };
            Ok(trust_result(crate::agentfirm_api::execute(
                store, auth, command,
            )?))
        }
        CanonicalRoute::Workspace {
            member_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid Workspace intent: {error}"),
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
            let (run, _) = require_member_or_host(store, &auth, member_run_id)?;
            if operation == "provision" {
                let RoleActionIntent::ProvisionWorkspace {
                    project_binding_id,
                    work_id,
                    mode,
                    ownership,
                    canonical_root,
                    base_ref,
                } = intent
                else {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match Workspace provision",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                };
                if let Some(replay) = canonical_replay(
                    store,
                    &auth,
                    "workspace_binding",
                    &deterministic_id("workspace", &auth),
                )? {
                    return Ok(replay);
                }
                if auth.expected_version != run.version {
                    return Err(encoded_error(
                        "VERSION_CONFLICT",
                        "Workspace provision requires the exact MemberRun revision",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                }
                let canonical = std::fs::canonicalize(&canonical_root).map_err(|error| {
                    encoded_error(
                        "WORKSPACE_UNSAFE",
                        format!("workspace path is not canonical/readable: {error}"),
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    )
                })?;
                let (team_run, team) = team_for_run(store, &run.team_run_id)?;
                if project_binding_id != team_run.project_binding_id
                    || !store
                        .latest_node_project_registrations()?
                        .into_iter()
                        .any(|registration| {
                            registration.node_id == team.node_id
                                && registration.execution_space_id == auth.execution_space_id
                                && registration.project_binding_id == project_binding_id
                                && registration.status
                                    == harness_core::NodeProjectRegistrationStatus::Active
                        })
                {
                    return Err(encoded_error("WORKSPACE_UNSAFE", "workspace project binding is not active on the Team's exact Node and Execution Space", "member_run", member_run_id, Some(run.version)));
                }
                let execution_root = team_run.execution_root.as_deref().ok_or_else(|| {
                    encoded_error(
                        "WORKSPACE_UNSAFE",
                        "TeamRun has no server-observed execution root",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    )
                })?;
                let canonical_execution_root =
                    std::fs::canonicalize(execution_root).map_err(|error| {
                        encoded_error(
                            "WORKSPACE_UNSAFE",
                            format!("TeamRun execution root cannot be canonicalized: {error}"),
                            "member_run",
                            member_run_id,
                            Some(run.version),
                        )
                    })?;
                if !canonical.starts_with(&canonical_execution_root) {
                    return Err(encoded_error(
                        "WORKSPACE_UNSAFE",
                        "workspace escapes the TeamRun execution-root boundary",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                }
                let git_value = |args: &[&str]| {
                    std::process::Command::new("git")
                        .arg("-C")
                        .arg(&canonical)
                        .args(args)
                        .output()
                        .ok()
                        .filter(|output| output.status.success())
                        .and_then(|output| String::from_utf8(output.stdout).ok())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                };
                let git_common_dir = git_value(&["rev-parse", "--git-common-dir"])
                    .and_then(|value| {
                        let path = std::path::PathBuf::from(value);
                        std::fs::canonicalize(if path.is_absolute() {
                            path
                        } else {
                            canonical.join(path)
                        })
                        .ok()
                    })
                    .map(|path| path.to_string_lossy().into_owned());
                let git_head = git_value(&["rev-parse", "HEAD"]);
                let git_branch = git_value(&["branch", "--show-current"]);
                let mut binding = MemberWorkspaceBinding {
                    id: deterministic_id("workspace", &auth),
                    project_binding_id,
                    team_run_id: run.team_run_id.clone(),
                    member_run_id: member_run_id.into(),
                    work_id,
                    mode,
                    ownership,
                    canonical_root: canonical.to_string_lossy().into_owned(),
                    git_common_dir,
                    base_ref,
                    git_head,
                    git_branch,
                    dirty_fingerprint: None,
                    instruction_roots: Vec::new(),
                    skill_roots: Vec::new(),
                    lifecycle: WorkspaceLifecycle::Requested,
                    blocked_reason: None,
                    attached_member_generation: None,
                    version: 1,
                    created_by: auth.actor.clone(),
                    created_at: now_string(),
                    updated_at: now_string(),
                };
                let proof = observe_workspace_proof(&binding, run.runtime_generation)?;
                if proof.is_dirty {
                    binding.dirty_fingerprint = Some(canonical_json_fingerprint(&json!({
                        "canonical_root": &binding.canonical_root,
                        "git_head": &binding.git_head,
                        "observed_dirty": true,
                    })));
                }
                let role_action_key = auth.idempotency_key.clone();
                let binding_id = binding.id.clone();
                let mut create_auth = auth.clone();
                create_auth.idempotency_key = format!("{role_action_key}:workspace-create");
                create_auth.expected_version = 0;
                crate::agentfirm_api::execute(
                    store,
                    create_auth,
                    crate::agentfirm_api::TrustCommand::ProvisionWorkspace { binding },
                )?;
                let mut prepare_auth = auth.clone();
                prepare_auth.idempotency_key = format!("{role_action_key}:workspace-prepare");
                prepare_auth.expected_version = 1;
                crate::agentfirm_api::execute(
                    store,
                    prepare_auth,
                    crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                        member_run_id: member_run_id.into(),
                        binding_id: binding_id.clone(),
                        next: WorkspaceLifecycle::Preparing,
                        proof: proof.clone(),
                        updated_at: now_string(),
                    },
                )?;
                auth.expected_version = 2;
                return Ok(trust_result(crate::agentfirm_api::execute(
                    store,
                    auth,
                    crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                        member_run_id: member_run_id.into(),
                        binding_id,
                        next: WorkspaceLifecycle::Ready,
                        proof,
                        updated_at: now_string(),
                    },
                )?));
            }
            let binding = latest_workspace(store, &auth.execution_space_id, member_run_id)?;
            if let Some(replay) = canonical_replay(store, &auth, "workspace_binding", &binding.id)?
            {
                return Ok(replay);
            }
            if auth.expected_version != binding.version {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Workspace transition requires the exact binding revision",
                    "workspace_binding",
                    &binding.id,
                    Some(binding.version),
                ));
            }
            let next = match (operation, intent) {
                ("attach", RoleActionIntent::AttachWorkspace) => WorkspaceLifecycle::Attached,
                ("archive", RoleActionIntent::ArchiveWorkspace) => WorkspaceLifecycle::Archived,
                ("cleanup", RoleActionIntent::CleanupWorkspace) => WorkspaceLifecycle::Removed,
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match Workspace route",
                        "workspace_binding",
                        &binding.id,
                        Some(binding.version),
                    ))
                }
            };
            if matches!(next, WorkspaceLifecycle::Removed)
                && confirmed_action != Some("cleanup_workspace")
            {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm cleanup_workspace",
                    "workspace_binding",
                    &binding.id,
                    Some(binding.version),
                ));
            }
            let proof = observe_workspace_proof(&binding, run.runtime_generation)?;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                    member_run_id: member_run_id.into(),
                    binding_id: binding.id,
                    next,
                    proof,
                    updated_at: now_string(),
                },
            )?))
        }
        CanonicalRoute::WorkRecord {
            team_id,
            work_id,
            operation,
        } => execute_work_record_action(
            store,
            auth,
            team_id,
            work_id,
            operation,
            body,
            confirmed_action,
        ),
        CanonicalRoute::Gate {
            requirement_id,
            operation,
        } => execute_gate_action(
            store,
            auth,
            requirement_id,
            operation,
            body,
            confirmed_action,
        ),
        CanonicalRoute::Waiver { waiver_id } => {
            execute_waiver_revoke(store, auth, waiver_id, body, confirmed_action)
        }
        CanonicalRoute::MessageDelivery {
            node_id,
            delivery_id,
        } => {
            let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid MessageDelivery intent: {error}"),
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
            let OperatorActionIntent::ReconcileMessageDelivery {
                outcome,
                evidence_ref,
            } = intent
            else {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "semantic action does not match MessageDelivery route",
                    "message_delivery",
                    delivery_id,
                    None,
                ));
            };
            if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "Operator must be the exact Execution Node Service",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            if confirmed_action != Some("reconcile_message_delivery") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm reconcile_message_delivery",
                    "message_delivery",
                    delivery_id,
                    None,
                ));
            }
            let lease = store
                .latest_node_daemon_lease(node_id)?
                .filter(|lease| {
                    lease.status == NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > crate::current_unix_ms_u64()
                })
                .ok_or_else(|| {
                    encoded_error(
                        "NODE_DAEMON_GENERATION_FENCED",
                        "MessageDelivery reconcile requires the exact current NodeDaemon",
                        "execution_node",
                        node_id,
                        None,
                    )
                })?;
            let daemon_actor = ActorRef {
                kind: ActorKind::Service,
                id: lease.daemon_id.clone(),
            };
            let context = MutationContext {
                execution_space_id: auth.execution_space_id,
                authenticated_actor: daemon_actor,
                authority_actor: Some(auth.actor.clone()),
                command_name: "node_daemon.message_delivery.reconcile".into(),
                idempotency_key: format!(
                    "role-message-reconcile:{}:{}",
                    auth.actor.id, auth.idempotency_key
                ),
                expected_version: auth.expected_version,
                request_fingerprint: auth.request_fingerprint,
            };
            canonical_mutation_result(store.reconcile_canonical_message_delivery(
                &context,
                delivery_id,
                node_id,
                &lease.daemon_id,
                lease.generation,
                outcome,
                &evidence_ref,
                &now_string(),
            )?)
        }
        CanonicalRoute::RuntimeRecovery {
            node_id,
            command_id,
        } => {
            let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid RuntimeCommand recovery intent: {error}"),
                    "runtime_command",
                    command_id,
                    None,
                )
            })?;
            let OperatorActionIntent::ResolveRuntimeRecovery {
                resolution,
                evidence_ref,
            } = intent
            else {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "semantic action does not match RuntimeCommand recovery route",
                    "runtime_command",
                    command_id,
                    None,
                ));
            };
            if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "RuntimeCommand recovery requires the exact Execution Node Operator",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            if confirmed_action != Some("resolve_runtime_recovery") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm resolve_runtime_recovery",
                    "runtime_command",
                    command_id,
                    None,
                ));
            }
            let lease = store
                .latest_node_daemon_lease(node_id)?
                .filter(|lease| {
                    lease.status == NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > crate::current_unix_ms_u64()
                })
                .ok_or_else(|| {
                    encoded_error(
                        "NODE_DAEMON_GENERATION_FENCED",
                        "RuntimeCommand recovery requires the exact current NodeDaemon",
                        "execution_node",
                        node_id,
                        None,
                    )
                })?;
            let context = MutationContext {
                execution_space_id: auth.execution_space_id,
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: lease.daemon_id.clone(),
                },
                authority_actor: Some(auth.actor.clone()),
                command_name: "node_daemon.runtime_command.resolve".into(),
                idempotency_key: format!(
                    "role-runtime-recovery:{}:{}",
                    auth.actor.id, auth.idempotency_key
                ),
                expected_version: auth.expected_version,
                request_fingerprint: auth.request_fingerprint,
            };
            canonical_mutation_result(store.resolve_runtime_command_recovery(
                &context,
                command_id,
                node_id,
                &lease.daemon_id,
                lease.generation,
                resolution,
                &evidence_ref,
                &now_string(),
            )?)
        }
        CanonicalRoute::Operator { node_id, operation } => {
            execute_operator_action(store, auth, node_id, operation, body, confirmed_action)
        }
    }
}

fn execute_work_record_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    team_id: &str,
    work_id: &str,
    operation: &str,
    body: &[u8],
    _confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Work record intent: {error}"),
            "work",
            work_id,
            None,
        )
    })?;
    let team = store.latest_teams()?.remove(team_id).ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "AgentTeam does not exist",
            "team",
            team_id,
            None,
        )
    })?;
    let current = current_canonical_work(store, &auth.execution_space_id, work_id)?;
    if current.team_id.as_deref() != Some(team_id) {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Work does not belong to the addressed Team",
            "work",
            work_id,
            Some(current.version),
        ));
    }
    match operation {
        "request-changes" | "gate-requirements" => {
            require_host(&auth, &team.host_agent_id, "work", work_id)?;
        }
        "revise" | "reports" | "findings" | "failure-analyses" => {
            let _ = require_exact_work_member(store, &auth, &current)?;
        }
        _ => {}
    }
    let replay = match operation {
        "request-changes" => work_replay(
            store,
            &auth,
            work_id,
            harness_core::WorkEventKind::ChangesRequested,
        )?,
        "revise" | "reports" => canonical_replay(
            store,
            &auth,
            "work_report",
            &deterministic_id("work-report", &auth),
        )?,
        "findings" => canonical_replay(
            store,
            &auth,
            "work_finding",
            &deterministic_id("work-finding", &auth),
        )?,
        "failure-analyses" => canonical_replay(
            store,
            &auth,
            "failure_analysis",
            &deterministic_id("failure-analysis", &auth),
        )?,
        "gate-requirements" => canonical_replay(
            store,
            &auth,
            "gate_requirement",
            &deterministic_id("gate-requirement", &auth),
        )?,
        _ => None,
    };
    if let Some(replay) = replay {
        return Ok(replay);
    }
    if auth.expected_version != current.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Work record action requires the exact current Work revision",
            "work",
            work_id,
            Some(current.version),
        ));
    }
    if operation == "request-changes" {
        let RoleActionIntent::RequestChanges { reason } = intent else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match request-changes",
                "work",
                work_id,
                Some(current.version),
            ));
        };
        let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
        let before = store.work_operations()?.len();
        let work = store.request_work_changes(
            work_id,
            auth.expected_version,
            &reason,
            host_context(&auth, host_id, false),
        )?;
        return work_action_result(store, &auth, before, work);
    }
    if operation == "revise" {
        let RoleActionIntent::ReviseWork {
            result_summary,
            artifact_refs,
            check_refs,
            base_revision,
            candidate_revision,
        } = intent
        else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match revise",
                "work",
                work_id,
                Some(current.version),
            ));
        };
        let _member_run = require_exact_work_member(store, &auth, &current)?;
        return create_result_report(
            store,
            auth,
            &team,
            &current,
            ResultReportInput {
                result_summary,
                artifact_refs,
                check_refs,
                base_revision,
                candidate_revision,
            },
        );
    }
    match (operation, intent) {
        (
            "reports",
            RoleActionIntent::WriteReport {
                summary,
                evidence_refs,
                recommended_next_action,
            },
        ) => {
            let _member_run = require_exact_work_member(store, &auth, &current)?;
            let report = WorkReport {
                id: deterministic_id("work-report", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                report_revision: canonical_report_count(store, &auth.execution_space_id, work_id)?
                    + 1,
                kind: WorkReportKind::Progress,
                authored_by: auth.actor.clone(),
                summary,
                base_revision: None,
                candidate: None,
                candidate_fingerprint: None,
                finding_refs: Vec::new(),
                failure_analysis_ref: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                evidence_refs,
                known_risks: Vec::new(),
                confidence: Some(Confidence::Medium),
                recommended_next_action,
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateWorkReport {
                    team_id: team_id.into(),
                    report,
                },
            )?))
        }
        (
            "findings",
            RoleActionIntent::WriteFinding {
                kind,
                summary,
                detail_markdown,
                evidence_refs,
                confidence,
            },
        ) => {
            let _member_run = require_exact_work_member(store, &auth, &current)?;
            let finding = WorkFinding {
                id: deterministic_id("work-finding", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                kind,
                summary,
                detail_markdown,
                affected_work_refs: Vec::new(),
                reusable_asset_refs: Vec::new(),
                invalidated_assumptions: Vec::new(),
                evidence_refs,
                confidence,
                reported_by: auth.actor.clone(),
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateWorkFinding {
                    team_id: team_id.into(),
                    finding,
                },
            )?))
        }
        (
            "failure-analyses",
            RoleActionIntent::WriteFailure {
                observed_failure,
                impact,
                primary_cause_status,
                primary_cause,
                retry_safety,
                recommended_host_decision,
                evidence_refs,
                confidence,
            },
        ) => {
            let member_run = require_exact_work_member(store, &auth, &current)?;
            let analysis = FailureAnalysis {
                id: deterministic_id("failure-analysis", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                member_run_id: Some(member_run),
                candidate: None,
                observed_failure,
                impact,
                primary_cause_status,
                primary_cause,
                contributing_causes: Vec::new(),
                attempts_already_made: Vec::new(),
                last_safe_checkpoint: None,
                retry_safety,
                side_effect_summary: None,
                recovery_options: Vec::new(),
                recommended_host_decision,
                evidence_refs,
                confidence,
                reported_by: auth.actor.clone(),
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateFailureAnalysis {
                    team_id: team_id.into(),
                    analysis,
                },
            )?))
        }
        (
            "gate-requirements",
            RoleActionIntent::RequestGateEvaluation {
                gate_type,
                gate_contract_version,
                evaluator_ref,
                evaluator_version,
                resolved_config,
                required,
            },
        ) => {
            require_host(&auth, &team.host_agent_id, "work", work_id)?;
            let report = store
                .canonical_operations_for_space(&auth.execution_space_id)?
                .into_iter()
                .filter(|op| op.event.aggregate_kind == "work_report")
                .filter_map(|op| serde_json::from_value::<WorkReport>(op.resulting_projection).ok())
                .filter(|report| {
                    report.work_id == work_id
                        && report.kind == WorkReportKind::Result
                        && report.work_revision == current.version
                })
                .max_by_key(|report| report.report_revision)
                .ok_or_else(|| {
                    encoded_error(
                        "REPORT_EVIDENCE_MISSING",
                        "Gate request requires the exact current result report",
                        "work",
                        work_id,
                        Some(current.version),
                    )
                })?;
            let candidate_fingerprint = report.candidate_fingerprint.clone().ok_or_else(|| {
                encoded_error(
                    "REPORT_EVIDENCE_MISSING",
                    "result report has no candidate fingerprint",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })?;
            let evaluator_fingerprint = canonical_json_fingerprint(
                &json!({"actor":evaluator_ref,"version":evaluator_version}),
            );
            let config_fingerprint = canonical_json_fingerprint(&resolved_config);
            let requirement = GateRequirement {
                id: deterministic_id("gate-requirement", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                work_report_id: report.id,
                candidate_fingerprint,
                source: GateRequirementSource::Direct,
                source_binding_id: None,
                gate_type,
                gate_contract_version,
                evaluator_ref,
                evaluator_version,
                evaluator_fingerprint,
                resolved_config,
                config_fingerprint,
                required,
                dependency_requirement_ids: Vec::new(),
                requirement_set_fingerprint: String::new(),
                created_at: now_string(),
                version: 1,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateGateRequirement {
                    team_id: team_id.into(),
                    requirement,
                },
            )?))
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Work record route",
            "work",
            work_id,
            Some(current.version),
        )),
    }
}

struct ResultReportInput {
    result_summary: String,
    artifact_refs: Vec<String>,
    check_refs: Vec<String>,
    base_revision: Option<String>,
    candidate_revision: String,
}

fn create_result_report(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    team: &harness_core::AgentTeam,
    current: &Work,
    input: ResultReportInput,
) -> Result<RoleActionResult, StoreError> {
    let ResultReportInput {
        result_summary,
        artifact_refs,
        check_refs,
        base_revision,
        candidate_revision,
    } = input;
    if candidate_revision.trim().is_empty() || artifact_refs.is_empty() && check_refs.is_empty() {
        return Err(encoded_error(
            "REPORT_EVIDENCE_MISSING",
            "result revision and at least one evidence ref are required",
            "work",
            &current.id,
            Some(current.version),
        ));
    }
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: candidate_revision,
    };
    let candidate_fingerprint = canonical_json_fingerprint(&serde_json::to_value(&candidate)?);
    let evidence_refs = artifact_refs
        .iter()
        .chain(check_refs.iter())
        .cloned()
        .collect();
    let report = WorkReport {
        id: deterministic_id("work-report", &auth),
        work_id: current.id.clone(),
        work_revision: current.version + 1,
        report_revision: canonical_report_count(store, &auth.execution_space_id, &current.id)? + 1,
        kind: WorkReportKind::Result,
        authored_by: auth.actor.clone(),
        summary: result_summary,
        base_revision,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs,
        check_refs,
        evidence_refs,
        known_risks: Vec::new(),
        confidence: Some(Confidence::High),
        recommended_next_action: Some("host_review".into()),
        created_at: now_string(),
    };
    auth.expected_version = 0;
    Ok(trust_result(crate::agentfirm_api::execute(
        store,
        auth,
        crate::agentfirm_api::TrustCommand::CreateWorkReport {
            team_id: team.id.clone(),
            report,
        },
    )?))
}

fn work_action_result(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    before: usize,
    work: Work,
) -> Result<RoleActionResult, StoreError> {
    let operations = store.work_operations()?;
    let operation = operations
        .iter()
        .rev()
        .find(|operation| {
            operation.work.id == work.id && operation.event.idempotency_key == auth.idempotency_key
        })
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "Work mutation committed without its operation",
                "work",
                &work.id,
                Some(work.version),
            )
        })?;
    Ok(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: serde_json::to_value(&work)?,
        event_id: operation.event.id.clone(),
        resulting_version: work.version,
        store_sequence: operations.len() as u64,
        replayed: operations.len() == before,
    })
}

fn execute_gate_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    requirement_id: &str,
    operation: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Gate intent: {error}"),
            "gate_requirement",
            requirement_id,
            None,
        )
    })?;
    let requirement = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .filter(|operation| operation.event.aggregate_kind == "gate_requirement")
        .flat_map(|operation| {
            std::iter::once(operation.resulting_projection).chain(operation.immutable_side_records)
        })
        .filter_map(|value| serde_json::from_value::<GateRequirement>(value).ok())
        .filter(|item| item.id == requirement_id)
        .max_by_key(|item| item.version)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "GateRequirement does not exist",
                "gate_requirement",
                requirement_id,
                None,
            )
        })?;
    let replay_id = match operation {
        "evaluate" => {
            if auth.actor != requirement.evaluator_ref {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "only the frozen exact evaluator may evaluate this gate",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            deterministic_id("gate-evaluation", &auth)
        }
        "waive" => {
            if confirmed_action != Some("waive_gate") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm waive_gate",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            if auth.authorized_authority_actors.is_empty() {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "credential has no frozen waiver authority",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            deterministic_id("gate-waiver", &auth)
        }
        _ => {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "unknown Gate operation",
                "gate_requirement",
                requirement_id,
                Some(requirement.version),
            ))
        }
    };
    if let Some(replay) = canonical_replay(
        store,
        &auth,
        if operation == "evaluate" {
            "gate_evaluation"
        } else {
            "gate_waiver"
        },
        &replay_id,
    )? {
        return Ok(replay);
    }
    if auth.expected_version != requirement.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Gate action requires the exact current requirement revision",
            "gate_requirement",
            requirement_id,
            Some(requirement.version),
        ));
    }
    match (operation, intent) {
        (
            "evaluate",
            RoleActionIntent::EvaluateGate {
                verdict,
                summary,
                evidence_refs,
            },
        ) => {
            let mut dependency_ids = requirement.dependency_requirement_ids.clone();
            dependency_ids.sort();
            let evaluation = GateEvaluation {
                id: deterministic_id("gate-evaluation", &auth),
                requirement_id: requirement.id.clone(),
                work_id: requirement.work_id.clone(),
                work_revision: requirement.work_revision,
                work_report_id: requirement.work_report_id.clone(),
                candidate_fingerprint: requirement.candidate_fingerprint.clone(),
                config_fingerprint: requirement.config_fingerprint.clone(),
                evaluator_version: requirement.evaluator_version.clone(),
                evaluator_fingerprint: requirement.evaluator_fingerprint.clone(),
                dependency_fingerprint: canonical_json_fingerprint(&serde_json::to_value(
                    dependency_ids,
                )?),
                verdict,
                summary,
                evidence_refs,
                performed_by: auth.actor.clone(),
                evaluated_at: now_string(),
                version: 1,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::EvaluateGate { evaluation },
            )?))
        }
        (
            "waive",
            RoleActionIntent::WaiveGate {
                reason,
                evidence_refs,
            },
        ) => {
            let authority_actor = auth
                .authorized_authority_actors
                .iter()
                .find(|authority| **authority == auth.actor)
                .cloned()
                .or_else(|| auth.authorized_authority_actors.first().cloned())
                .ok_or_else(|| {
                    encoded_error(
                        "UNAUTHORIZED_ACTOR",
                        "credential has no frozen waiver authority",
                        "gate_requirement",
                        requirement_id,
                        Some(requirement.version),
                    )
                })?;
            let waiver = GateWaiver {
                id: deterministic_id("gate-waiver", &auth),
                requirement_id: requirement.id,
                work_id: requirement.work_id,
                work_revision: requirement.work_revision,
                candidate_fingerprint: requirement.candidate_fingerprint,
                authority_actor,
                performed_by_actor: auth.actor.clone(),
                reason,
                evidence_refs,
                state: GateWaiverState::Active,
                version: 1,
                created_at: now_string(),
                revoked_at: None,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::WaiveGate { waiver },
            )?))
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Gate route",
            "gate_requirement",
            requirement_id,
            Some(requirement.version),
        )),
    }
}

fn execute_waiver_revoke(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    waiver_id: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid waiver intent: {error}"),
            "gate_waiver",
            waiver_id,
            None,
        )
    })?;
    if !matches!(intent, RoleActionIntent::RevokeWaiver) {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match waiver revoke",
            "gate_waiver",
            waiver_id,
            None,
        ));
    }
    let waiver = store
        .trust_gate_waivers(&auth.execution_space_id)?
        .into_iter()
        .find(|item| item.id == waiver_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "GateWaiver does not exist",
                "gate_waiver",
                waiver_id,
                None,
            )
        })?;
    if confirmed_action != Some("revoke_waiver") {
        return Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            "server confirmation must exactly confirm revoke_waiver",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    if waiver.performed_by_actor != auth.actor
        || !auth
            .authorized_authority_actors
            .contains(&waiver.authority_actor)
    {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "only the exact waiver actor with its frozen authority may revoke",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    if let Some(replay) = canonical_replay(store, &auth, "gate_waiver", waiver_id)? {
        return Ok(replay);
    }
    if auth.expected_version != waiver.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "waiver revoke requires exact current revision",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    Ok(trust_result(crate::agentfirm_api::execute(
        store,
        auth,
        crate::agentfirm_api::TrustCommand::RevokeGateWaiver {
            waiver_id: waiver_id.into(),
            revoked_at: now_string(),
        },
    )?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperatorActionJournalState {
    Prepared,
    InFlight,
    Completed,
    RecoveryRequired,
}

#[derive(Debug, Serialize, Deserialize)]
struct OperatorActionReceipt {
    request_fingerprint: String,
    state: OperatorActionJournalState,
    #[serde(default)]
    projection: Option<serde_json::Value>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    resulting_version: Option<u64>,
    #[serde(default)]
    store_sequence: Option<u64>,
    #[serde(default)]
    recovery_detail: Option<String>,
}

fn operator_receipt_paths(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
) -> Result<(std::path::PathBuf, std::path::PathBuf, String), StoreError> {
    let request_fingerprint = auth.request_fingerprint.clone().ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "Operator action is missing its server-bound request fingerprint",
            "execution_node",
            node_id,
            None,
        )
    })?;
    let receipt_root = firm_home
        .join("runtime")
        .join("operator-action-receipts")
        .join(node_id);
    let receipt_id = canonical_json_fingerprint(&json!({
        "node_id": node_id,
        "idempotency_key": auth.idempotency_key,
    }));
    Ok((
        receipt_root.join(format!("{receipt_id}.json")),
        receipt_root.join(format!("{receipt_id}.lock")),
        request_fingerprint,
    ))
}

fn operator_journal_result(
    receipt: OperatorActionReceipt,
    node_id: &str,
) -> Result<Option<RoleActionResult>, StoreError> {
    match receipt.state {
        OperatorActionJournalState::Completed => Ok(Some(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: receipt.projection.ok_or_else(|| {
                encoded_error(
                    "RECOVERY_REQUIRED",
                    "completed Operator journal is missing its projection",
                    "execution_node",
                    node_id,
                    receipt.resulting_version,
                )
            })?,
            event_id: receipt.event_id.ok_or_else(|| {
                encoded_error(
                    "RECOVERY_REQUIRED",
                    "completed Operator journal is missing its event id",
                    "execution_node",
                    node_id,
                    receipt.resulting_version,
                )
            })?,
            resulting_version: receipt.resulting_version.unwrap_or_default(),
            store_sequence: receipt.store_sequence.unwrap_or_default(),
            replayed: true,
        })),
        OperatorActionJournalState::InFlight | OperatorActionJournalState::RecoveryRequired => {
            Err(encoded_error(
                "RECOVERY_REQUIRED",
                receipt.recovery_detail.unwrap_or_else(|| {
                    "prior Operator request may have crossed the external-effect boundary; reconcile before retrying".into()
                }),
                "execution_node",
                node_id,
                receipt.resulting_version,
            ))
        }
        OperatorActionJournalState::Prepared => Ok(None),
    }
}

fn read_operator_receipt(
    receipt_path: &std::path::Path,
    node_id: &str,
) -> Result<OperatorActionReceipt, StoreError> {
    let bytes = std::fs::read(receipt_path).map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!("Operator journal cannot be read safely: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!("Operator journal is torn or invalid: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })
}

fn replay_receipted_operator_action(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
) -> Result<Option<RoleActionResult>, StoreError> {
    let (receipt_path, lock_path, request_fingerprint) =
        operator_receipt_paths(firm_home, node_id, auth)?;
    if !receipt_path.exists() {
        return Ok(None);
    }
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.lock()?;
    let receipt = read_operator_receipt(&receipt_path, node_id)?;
    if receipt.request_fingerprint != request_fingerprint {
        return Err(encoded_error(
            "IDEMPOTENCY_CONFLICT",
            "idempotency key was already bound to a different Operator action fingerprint",
            "execution_node",
            node_id,
            receipt.resulting_version,
        ));
    }
    operator_journal_result(receipt, node_id)
}

fn execute_receipted_operator_action<F>(
    firm_home: &std::path::Path,
    node_id: &str,
    auth: &AuthenticatedMutation,
    execute: F,
) -> Result<RoleActionResult, StoreError>
where
    F: FnOnce() -> Result<RoleActionResult, StoreError>,
{
    let (receipt_path, lock_path, request_fingerprint) =
        operator_receipt_paths(firm_home, node_id, auth)?;
    let receipt_root = receipt_path.parent().ok_or_else(|| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            "Operator journal path has no parent",
            "execution_node",
            node_id,
            None,
        )
    })?;
    std::fs::create_dir_all(receipt_root).map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            format!("cannot create Operator receipt directory: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                format!("cannot open Operator receipt lock: {error}"),
                "execution_node",
                node_id,
                None,
            )
        })?;
    lock_file.lock().map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            format!("cannot lock Operator receipt: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    if receipt_path.exists() {
        let receipt = read_operator_receipt(&receipt_path, node_id)?;
        if receipt.request_fingerprint != request_fingerprint {
            return Err(encoded_error(
                "IDEMPOTENCY_CONFLICT",
                "idempotency key was already committed with a different Operator action fingerprint",
                "execution_node",
                node_id,
                receipt.resulting_version,
            ));
        }
        if let Some(result) = operator_journal_result(receipt, node_id)? {
            return Ok(result);
        }
    }
    let write = |receipt: &OperatorActionReceipt| -> Result<(), StoreError> {
        crate::execution_space::atomic_write_bytes(
            &receipt_path,
            &serde_json::to_vec_pretty(receipt)?,
        )
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                format!("cannot commit Operator action journal: {error}"),
                "execution_node",
                node_id,
                receipt.resulting_version,
            )
        })
    };
    write(&OperatorActionReceipt {
        request_fingerprint: request_fingerprint.clone(),
        state: OperatorActionJournalState::Prepared,
        projection: None,
        event_id: None,
        resulting_version: None,
        store_sequence: None,
        recovery_detail: None,
    })?;
    write(&OperatorActionReceipt {
        request_fingerprint: request_fingerprint.clone(),
        state: OperatorActionJournalState::InFlight,
        projection: None,
        event_id: None,
        resulting_version: None,
        store_sequence: None,
        recovery_detail: Some(
            "external effect was started but no durable completion receipt exists".into(),
        ),
    })?;
    let result = execute().map_err(|error| {
        let recovery = OperatorActionReceipt {
            request_fingerprint: request_fingerprint.clone(),
            state: OperatorActionJournalState::RecoveryRequired,
            projection: None,
            event_id: None,
            resulting_version: None,
            store_sequence: None,
            recovery_detail: Some(format!(
                "Operator external effect returned without a provable completion receipt: {error}"
            )),
        };
        let _ = write(&recovery);
        encoded_error(
            "RECOVERY_REQUIRED",
            recovery.recovery_detail.unwrap_or_default(),
            "execution_node",
            node_id,
            None,
        )
    })?;
    write(&OperatorActionReceipt {
        request_fingerprint,
        state: OperatorActionJournalState::Completed,
        projection: Some(result.projection.clone()),
        event_id: Some(result.event_id.clone()),
        resulting_version: Some(result.resulting_version),
        store_sequence: Some(result.store_sequence),
        recovery_detail: None,
    })
    .map_err(|error| {
        encoded_error(
            "RECOVERY_REQUIRED",
            format!(
                "external effect completed but its durable completion receipt could not be committed: {error}"
            ),
            "execution_node",
            node_id,
            Some(result.resulting_version),
        )
    })?;
    Ok(result)
}

fn execute_operator_action(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    node_id: &str,
    operation: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Operator credential is not the exact Execution Node Service",
            "execution_node",
            node_id,
            None,
        ));
    }
    let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Operator intent: {error}"),
            "execution_node",
            node_id,
            None,
        )
    })?;
    let intent_matches = matches!(
        (operation, &intent),
        ("diagnostics", OperatorActionIntent::Diagnose)
            | ("daemon-start", OperatorActionIntent::DaemonStart { .. })
            | ("daemon-stop", OperatorActionIntent::DaemonStop { .. })
            | (
                "provider-admission",
                OperatorActionIntent::AdmitProvider { .. }
            )
    );
    if !intent_matches {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Operator route",
            "execution_node",
            node_id,
            None,
        ));
    }
    let daemon_action = matches!(operation, "daemon-start" | "daemon-stop");
    let receipted_action = daemon_action || operation == "provider-admission";
    if daemon_action && confirmed_action != Some(operation) {
        return Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            format!("server confirmation must exactly confirm {operation}"),
            "execution_node",
            node_id,
            None,
        ));
    }
    let firm_home = receipted_action
        .then(crate::execution_space::firm_home)
        .transpose()
        .map_err(|error| {
            encoded_error(
                "ACTION_UNAVAILABLE",
                error.to_string(),
                "execution_node",
                node_id,
                None,
            )
        })?;
    if let Some(firm_home) = firm_home.as_deref() {
        if let Some(replay) = replay_receipted_operator_action(firm_home, node_id, &auth)? {
            return Ok(replay);
        }
    }
    let node_revision = store
        .execution_nodes()?
        .into_iter()
        .filter(|node| node.id == node_id)
        .count() as u64;
    if auth.expected_version != node_revision {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Operator action requires the exact current ExecutionNode revision",
            "execution_node",
            node_id,
            Some(node_revision),
        ));
    }
    let local_node_id = crate::read_local_node_id().map_err(|error| {
        encoded_error(
            "ACTION_UNAVAILABLE",
            error.to_string(),
            "execution_node",
            node_id,
            Some(node_revision),
        )
    })?;
    if local_node_id != node_id {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Operator action targets a Node other than this machine's immutable Node",
            "execution_node",
            node_id,
            Some(node_revision),
        ));
    }
    match (operation, intent) {
        ("diagnostics", OperatorActionIntent::Diagnose) => {
            let lease = store.latest_node_daemon_lease(node_id)?;
            Ok(RoleActionResult {
                ok: true,
                action_protocol_version: "agentfirm.role_actions.v1",
                projection: json!({"node_id":node_id,"daemon_lease":lease}),
                event_id: format!("diagnostic:{}", auth.idempotency_key),
                resulting_version: auth.expected_version,
                store_sequence: store
                    .canonical_operations_for_space(&auth.execution_space_id)?
                    .len() as u64,
                replayed: false,
            })
        }
        (
            "daemon-start",
            OperatorActionIntent::DaemonStart {
                max_concurrency,
                daemon_generation,
            },
        ) => {
            if max_concurrency == 0 {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "daemon concurrency must be non-zero",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            let firm_home = firm_home.expect("daemon action resolves firm home before dispatch");
            let current_generation = store
                .latest_node_daemon_lease(node_id)?
                .map(|lease| lease.generation)
                .unwrap_or(0);
            if daemon_generation != current_generation {
                return Err(encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon start intent does not match the current NodeDaemon generation",
                    "node_daemon_lease",
                    node_id,
                    Some(current_generation),
                ));
            }
            if crate::supervisor_daemon::daemon_status_via_socket(&firm_home, node_id).is_some() {
                return Err(encoded_error(
                    "ACTION_UNAVAILABLE",
                    "NodeDaemon is already live; refresh the Operator RoleView",
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let status = crate::supervisor_daemon::start_daemon_process_fenced(
                    &firm_home,
                    node_id,
                    max_concurrency,
                    &auth.execution_space_id,
                    daemon_generation,
                )
                .map_err(|error| {
                    encoded_error(
                        "DAEMON_START_FAILED",
                        error.to_string(),
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                let lease = store.latest_node_daemon_lease(node_id)?;
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: json!({"node_id":node_id,"status":status,"lease":lease}),
                    event_id: format!("daemon-start:{}", auth.idempotency_key),
                    resulting_version: node_revision,
                    store_sequence: store
                        .canonical_operations_for_space(&auth.execution_space_id)?
                        .len() as u64,
                    replayed: false,
                })
            })
        }
        ("daemon-stop", OperatorActionIntent::DaemonStop { daemon_generation }) => {
            let firm_home = firm_home.expect("daemon action resolves firm home before dispatch");
            let lease = store.latest_node_daemon_lease(node_id)?.ok_or_else(|| {
                encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon stop requires a current NodeDaemon lease",
                    "node_daemon_lease",
                    node_id,
                    None,
                )
            })?;
            if lease.generation != daemon_generation
                || lease.status != harness_core::NodeDaemonLeaseStatus::Active
                || lease.expires_unix_ms <= crate::current_unix_ms_u64()
            {
                return Err(encoded_error(
                    "SUPERVISOR_GENERATION_FENCED",
                    "daemon stop intent does not match the current live NodeDaemon generation",
                    "node_daemon_lease",
                    node_id,
                    Some(lease.generation),
                ));
            }
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let response = crate::supervisor_daemon::daemon_stop_via_socket(
                    &firm_home,
                    node_id,
                    &auth.execution_space_id,
                    daemon_generation,
                )
                .ok_or_else(|| {
                    encoded_error(
                        "ACTION_UNAVAILABLE",
                        "no live NodeDaemon is available to stop; refresh the Operator RoleView",
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                let control =
                    serde_json::from_str::<serde_json::Value>(&response).map_err(|_| {
                        encoded_error(
                            "RECOVERY_REQUIRED",
                            "NodeDaemon returned an invalid stop receipt",
                            "node_daemon_lease",
                            node_id,
                            Some(daemon_generation),
                        )
                    })?;
                if control["ok"] != true {
                    return Err(encoded_error(
                        "SUPERVISOR_GENERATION_FENCED",
                        control["error"]
                            .as_str()
                            .unwrap_or("NodeDaemon rejected the generation-fenced stop"),
                        "node_daemon_lease",
                        node_id,
                        Some(daemon_generation),
                    ));
                }
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: json!({"node_id":node_id,"status":response}),
                    event_id: format!("daemon-stop:{}", auth.idempotency_key),
                    resulting_version: node_revision,
                    store_sequence: store
                        .canonical_operations_for_space(&auth.execution_space_id)?
                        .len() as u64,
                    replayed: false,
                })
            })
        }
        (
            "provider-admission",
            OperatorActionIntent::AdmitProvider {
                provider,
                execution_mode,
                eligibility_fingerprint,
            },
        ) => {
            let binding = provider_admission_action_binding(
                store,
                &auth.execution_space_id,
                node_id,
                node_revision,
                &provider,
                &execution_mode,
            );
            if binding.eligibility_fingerprint != eligibility_fingerprint {
                return Err(encoded_error(
                    "ACTION_BINDING_MISMATCH",
                    "provider admission tuple or eligibility changed; refetch the Operator RoleView",
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            if let Some(reason) = binding.disabled_reason {
                return Err(encoded_error(
                    "ACTION_UNAVAILABLE",
                    reason,
                    "execution_node",
                    node_id,
                    Some(node_revision),
                ));
            }
            let firm_home = firm_home.expect("receipted provider action resolves firm home");
            execute_receipted_operator_action(&firm_home, node_id, &auth, || {
                let (admission, replayed) = crate::admit_provider_from_operator_action(
                    store,
                    &auth.execution_space_id,
                    node_id,
                    &provider,
                    &execution_mode,
                    &auth.idempotency_key,
                )
                .map_err(|error| {
                    encoded_error(
                        "PROVIDER_ADMISSION_FAILED",
                        error,
                        "execution_node",
                        node_id,
                        Some(node_revision),
                    )
                })?;
                Ok(RoleActionResult {
                    ok: true,
                    action_protocol_version: "agentfirm.role_actions.v1",
                    projection: serde_json::to_value(&admission)?,
                    event_id: admission.id.clone(),
                    resulting_version: 1,
                    store_sequence: store.latest_provider_compatibility_admissions()?.len() as u64,
                    replayed,
                })
            })
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Operator route",
            "execution_node",
            node_id,
            None,
        )),
    }
}

pub fn execute(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    path: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    auth.request_fingerprint = Some(canonical_json_fingerprint(&json!({
        "protocol":"agentfirm.role_actions.v1",
        "execution_space_id":auth.execution_space_id,
        "actor":auth.actor,
        "authorized_authority_actors":auth.authorized_authority_actors,
        "path":path,
        "intent":serde_json::from_slice::<serde_json::Value>(body).unwrap_or(serde_json::Value::Null),
        "expected_version":auth.expected_version,
        "confirmation":confirmed_action,
        "project_scope":role_action_scope(store, &auth.execution_space_id, path),
    })));
    if let Some(route) = parse_canonical_route(path) {
        return execute_canonical_role_action(store, auth, route, body, confirmed_action);
    }
    if let Some((node_id, delivery_id)) = parse_operator_route(path) {
        let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid closed operator action intent: {error}"),
                "route",
                path,
                None,
            )
        })?;
        if confirmed_action != Some("reconcile_delivery") {
            return Err(encoded_error(
                "CONFIRMATION_REQUIRED",
                "server confirmation header must exactly confirm reconcile_delivery",
                "work_delivery",
                delivery_id,
                None,
            ));
        }
        let exact_node = auth.actor.kind == ActorKind::Service && auth.actor.id == node_id;
        if !exact_node {
            return Err(encoded_error(
                "UNAUTHORIZED_ACTOR",
                "operator credential is not bound to the addressed Execution Node",
                "execution_node",
                node_id,
                None,
            ));
        }
        if let Some(replay) = canonical_replay(store, &auth, "work_delivery", delivery_id)? {
            return Ok(replay);
        }
        let OperatorActionIntent::ReconcileDelivery { evidence_ref } = intent else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match WorkDelivery route",
                "work_delivery",
                delivery_id,
                None,
            ));
        };
        let result = crate::agentfirm_api::execute(
            store,
            auth,
            crate::agentfirm_api::TrustCommand::ReconcileWorkDelivery {
                delivery_id: delivery_id.to_string(),
                evidence_ref,
                updated_at: now_string(),
            },
        )?;
        return Ok(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: result.projection,
            event_id: result.event_id,
            resulting_version: result.resulting_version,
            store_sequence: result.store_sequence,
            replayed: result.replayed,
        });
    }
    if let Some((team_id, work_id)) = parse_accept_route(path) {
        let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid closed role action intent: {error}"),
                "route",
                path,
                None,
            )
        })?;
        if !matches!(intent, RoleActionIntent::AcceptWork) {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match the exact route",
                "work",
                work_id,
                None,
            ));
        }
        if confirmed_action != Some("accept") {
            return Err(encoded_error(
                "CONFIRMATION_REQUIRED",
                "server confirmation header must exactly confirm accept",
                "work",
                work_id,
                None,
            ));
        }
        let team = store.latest_teams()?.remove(team_id).ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "AgentTeam does not exist",
                "team",
                team_id,
                None,
            )
        })?;
        require_host(&auth, &team.host_agent_id, "work", work_id)?;
        if let Some(replay) = canonical_replay(store, &auth, "work", work_id)? {
            return Ok(replay);
        }
        let work = current_canonical_work(store, &auth.execution_space_id, work_id)?;
        if work.team_id.as_deref() != Some(team_id) || work.version != auth.expected_version {
            return Err(encoded_error(
                "VERSION_CONFLICT",
                "accept requires the exact current Team-scoped Work revision",
                "work",
                work_id,
                Some(work.version),
            ));
        }
        let report = store
            .canonical_operations_for_space(&auth.execution_space_id)?
            .into_iter()
            .filter(|operation| operation.event.aggregate_kind == "work_report")
            .filter_map(|operation| {
                serde_json::from_value::<WorkReport>(operation.resulting_projection).ok()
            })
            .filter(|report| {
                report.kind == WorkReportKind::Result
                    && report.work_id == work_id
                    && report.work_revision == work.version
            })
            .max_by_key(|report| report.report_revision)
            .ok_or_else(|| {
                encoded_error(
                    "REPORT_EVIDENCE_MISSING",
                    "accept requires the exact current result WorkReport",
                    "work",
                    work_id,
                    Some(work.version),
                )
            })?;
        let candidate_fingerprint = report.candidate_fingerprint.clone().ok_or_else(|| {
            encoded_error(
                "REPORT_EVIDENCE_MISSING",
                "result WorkReport has no candidate fingerprint",
                "work",
                work_id,
                Some(work.version),
            )
        })?;
        let result = crate::agentfirm_api::execute(
            store,
            auth,
            crate::agentfirm_api::TrustCommand::AcceptWork {
                team_id: team_id.to_string(),
                work_id: work_id.to_string(),
                work_report_id: report.id,
                candidate_fingerprint,
                updated_at: now_string(),
            },
        )?;
        return Ok(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: result.projection,
            event_id: result.event_id,
            resulting_version: result.resulting_version,
            store_sequence: result.store_sequence,
            replayed: result.replayed,
        });
    }
    let route = parse_route(path).ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "unknown AgentFirm role action route",
            "route",
            path,
            None,
        )
    })?;
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid closed role action intent: {error}"),
            "route",
            path,
            None,
        )
    })?;
    let (_run, team) = team_for_run(store, route.team_run_id)?;
    if let (
        "submit",
        Some(work_id),
        RoleActionIntent::SubmitWork {
            result_summary,
            artifact_refs,
            check_refs,
            base_revision,
            candidate_revision,
        },
    ) = (route.operation, route.work_id, &intent)
    {
        let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
        if let Some(replay) = canonical_replay(
            store,
            &auth,
            "work_report",
            &deterministic_id("work-report", &auth),
        )? {
            return Ok(replay);
        }
        let current = current_work(store, route.team_run_id, work_id)?;
        if current.owner_member_id.as_deref() != Some(auth.actor.id.as_str())
            || current.active_member_run_id.as_deref() != Some(member_run_id.as_str())
        {
            return Err(encoded_error(
                "UNAUTHORIZED_ACTOR",
                "submit requires the exact accountable AgentMember and current active WorkExecutionBinding",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        if current.version != auth.expected_version {
            return Err(encoded_error(
                "VERSION_CONFLICT",
                format!(
                    "expected version {}, current version is {}",
                    auth.expected_version, current.version
                ),
                "work",
                work_id,
                Some(current.version),
            ));
        }
        let candidate_value = candidate_revision
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                encoded_error(
                    "REPORT_EVIDENCE_MISSING",
                    "candidate_revision is required",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })?;
        let evidence_refs = artifact_refs
            .iter()
            .chain(check_refs.iter())
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>();
        if evidence_refs.is_empty() {
            return Err(encoded_error(
                "REPORT_EVIDENCE_MISSING",
                "at least one artifact_ref or check_ref is required",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        let candidate = CandidateRef {
            kind: CandidateKind::GitCommit,
            value: candidate_value.to_string(),
        };
        let candidate_fingerprint = canonical_json_fingerprint(&serde_json::to_value(&candidate)?);
        let report = WorkReport {
            id: format!("work-report:{}", auth.idempotency_key),
            work_id: work_id.to_string(),
            work_revision: current.version + 1,
            report_revision: canonical_report_count(store, &auth.execution_space_id, work_id)? + 1,
            kind: WorkReportKind::Result,
            authored_by: auth.actor.clone(),
            summary: result_summary.clone(),
            base_revision: base_revision.clone(),
            candidate: Some(candidate),
            candidate_fingerprint: Some(candidate_fingerprint),
            finding_refs: Vec::new(),
            failure_analysis_ref: None,
            artifact_refs: artifact_refs.clone(),
            check_refs: check_refs.clone(),
            evidence_refs,
            known_risks: Vec::new(),
            confidence: Some(Confidence::High),
            recommended_next_action: Some("host_review".into()),
            created_at: now_string(),
        };
        let mut report_auth = auth;
        // The external CAS is the addressed Work revision. WorkReport is a
        // newly-created canonical aggregate, so its internal create CAS is 0.
        report_auth.expected_version = 0;
        let result = crate::agentfirm_api::execute(
            store,
            report_auth,
            crate::agentfirm_api::TrustCommand::CreateWorkReport {
                team_id: team.id.clone(),
                report,
            },
        )?;
        return Ok(RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: result.projection,
            event_id: result.event_id,
            resulting_version: result.resulting_version,
            store_sequence: result.store_sequence,
            replayed: result.replayed,
        });
    }
    let before = store.work_operations()?.len();
    let work = match (route.operation, route.work_id, intent) {
        (
            "create",
            None,
            RoleActionIntent::CreateWork {
                work_id,
                title,
                context_markdown,
                completion_criteria_markdown,
                parent_work_id,
                eligible_member_ids,
                prerequisite_work_ids,
                claim_mode,
                priority,
            },
        ) => {
            let host_id = require_host(&auth, &team.host_agent_id, "team_run", route.team_run_id)?;
            if let Some(replay) =
                work_replay(store, &auth, &work_id, harness_core::WorkEventKind::Created)?
            {
                return Ok(replay);
            }
            if auth.expected_version != 0 {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Work create requires If-Match: 0",
                    "team_run",
                    route.team_run_id,
                    Some(0),
                ));
            }
            let context = host_context(&auth, host_id, false);
            store.insert_work(
                Work {
                    id: work_id,
                    team_run_id: route.team_run_id.to_string(),
                    team_id: Some(team.id.clone()),
                    parent_work_id,
                    title,
                    context_markdown,
                    completion_criteria_markdown,
                    phase: WorkPhase::Open,
                    condition: WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: None,
                    claim_mode,
                    eligible_member_ids,
                    prerequisite_work_ids,
                    priority,
                    created_by_actor: context.performed_by_actor.clone(),
                    created_by_member_id: None,
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: context.created_at.clone(),
                    updated_at: context.created_at.clone(),
                },
                context,
            )?
        }
        (operation, Some(work_id), intent) => {
            require_confirmed(operation, confirmed_action, work_id)?;
            let kind = match operation {
                "assign" => harness_core::WorkEventKind::Assigned,
                "rebind" => harness_core::WorkEventKind::Rebound,
                "release" => harness_core::WorkEventKind::Released,
                "cancel" => harness_core::WorkEventKind::Cancelled,
                "claim" => harness_core::WorkEventKind::Claimed,
                "start" => harness_core::WorkEventKind::Started,
                "block" => harness_core::WorkEventKind::Blocked,
                "resume" => harness_core::WorkEventKind::Resumed,
                "submit" => harness_core::WorkEventKind::Submitted,
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "unknown Work operation",
                        "work",
                        work_id,
                        None,
                    ))
                }
            };
            if matches!(operation, "assign" | "rebind" | "cancel") {
                require_host(&auth, &team.host_agent_id, "work", work_id)?;
            } else if operation != "release" || !is_host(&auth, &team.host_agent_id) {
                let _ = resolve_member_run(store, &auth, route.team_run_id)?;
            }
            if let Some(replay) = work_replay(store, &auth, work_id, kind)? {
                return Ok(replay);
            }
            let current = current_work(store, route.team_run_id, work_id)?;
            if current.version != auth.expected_version {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    format!(
                        "expected version {}, current version is {}",
                        auth.expected_version, current.version
                    ),
                    "work",
                    work_id,
                    Some(current.version),
                ));
            }
            match (operation, intent) {
                ("assign", RoleActionIntent::AssignWork { member_run_id }) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    store.assign_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        host_context(&auth, host_id, false),
                    )?
                }
                ("rebind", RoleActionIntent::RebindWork { member_run_id }) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    store.rebind_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        host_context(&auth, host_id, false),
                    )?
                }
                ("release", RoleActionIntent::ReleaseWork)
                    if is_host(&auth, &team.host_agent_id) =>
                {
                    store.release_work_as_host(
                        work_id,
                        auth.expected_version,
                        host_context(&auth, &team.host_agent_id, false),
                    )?
                }
                ("release", RoleActionIntent::ReleaseWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.release_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        member_context(&auth, &member_run_id),
                    )?
                }
                ("cancel", RoleActionIntent::CancelWork { reason }) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    store.cancel_work(
                        work_id,
                        auth.expected_version,
                        &reason,
                        host_context(&auth, host_id, false),
                    )?
                }
                ("claim", RoleActionIntent::ClaimWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.claim_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        member_context(&auth, &member_run_id),
                    )?
                }
                ("start", RoleActionIntent::StartWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.start_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        member_context(&auth, &member_run_id),
                    )?
                }
                ("block", RoleActionIntent::BlockWork { reason }) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.block_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        &reason,
                        member_context(&auth, &member_run_id),
                    )?
                }
                ("resume", RoleActionIntent::UnblockWork { resolution }) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.resume_work(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        &resolution,
                        member_context(&auth, &member_run_id),
                    )?
                }
                (
                    "submit",
                    RoleActionIntent::SubmitWork {
                        result_summary,
                        artifact_refs,
                        check_refs,
                        base_revision,
                        candidate_revision,
                    },
                ) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    store.submit_work_with_revision_and_links(
                        work_id,
                        auth.expected_version,
                        &member_run_id,
                        &result_summary,
                        artifact_refs,
                        check_refs,
                        Vec::new(),
                        base_revision,
                        candidate_revision,
                        member_context(&auth, &member_run_id),
                    )?
                }
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match the exact route",
                        "work",
                        work_id,
                        Some(current.version),
                    ))
                }
            }
        }
        _ => {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match the exact route",
                "route",
                path,
                None,
            ))
        }
    };
    let operations = store.work_operations()?;
    let operation = operations
        .iter()
        .rev()
        .find(|operation| {
            operation.work.id == work.id && operation.event.idempotency_key == auth.idempotency_key
        })
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "Work mutation committed without a matching operation",
                "work",
                &work.id,
                Some(work.version),
            )
        })?;
    Ok(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: serde_json::to_value(work.clone())?,
        event_id: operation.event.id.clone(),
        resulting_version: work.version,
        store_sequence: operations.len() as u64,
        replayed: operations.len() == before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operator_auth(key: &str, fingerprint: &str) -> AuthenticatedMutation {
        AuthenticatedMutation {
            execution_space_id: "space-test".into(),
            actor: ActorRef {
                kind: ActorKind::Service,
                id: "node-test".into(),
            },
            authorized_authority_actors: Vec::new(),
            idempotency_key: key.into(),
            expected_version: 1,
            request_fingerprint: Some(fingerprint.into()),
        }
    }

    fn operator_result(event_id: &str) -> RoleActionResult {
        RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: json!({"effect":"complete"}),
            event_id: event_id.into(),
            resulting_version: 1,
            store_sequence: 1,
            replayed: false,
        }
    }

    #[test]
    fn operator_journal_replays_completed_effect_and_fences_uncertain_restart() {
        let root = std::env::temp_dir().join(format!(
            "firm-operator-journal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let auth = operator_auth("completed", "fingerprint-completed");
        let first = execute_receipted_operator_action(&root, "node-test", &auth, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("effect-1"))
        })
        .unwrap();
        assert!(!first.replayed);
        let replay = execute_receipted_operator_action(&root, "node-test", &auth, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-run"))
        })
        .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.event_id, "effect-1");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let changed_scope = operator_auth("completed", "fingerprint-changed-tuple-or-scope");
        let error = execute_receipted_operator_action(&root, "node-test", &changed_scope, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-run-for-changed-scope"))
        })
        .expect_err("same key with a changed tuple or scope must fail closed");
        assert!(error.to_string().contains("fingerprint"), "{error}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let uncertain = operator_auth("uncertain", "fingerprint-uncertain");
        let (path, _, fingerprint) =
            operator_receipt_paths(&root, "node-test", &uncertain).unwrap();
        crate::execution_space::atomic_write_bytes(
            &path,
            &serde_json::to_vec(&OperatorActionReceipt {
                request_fingerprint: fingerprint,
                state: OperatorActionJournalState::InFlight,
                projection: None,
                event_id: None,
                resulting_version: None,
                store_sequence: None,
                recovery_detail: Some("fault after effect before receipt".into()),
            })
            .unwrap(),
        )
        .unwrap();
        let error = execute_receipted_operator_action(&root, "node-test", &uncertain, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-repeat"))
        })
        .expect_err("uncertain restart must require recovery");
        assert!(error.to_string().contains("RECOVERY_REQUIRED"), "{error}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let torn = operator_auth("torn", "fingerprint-torn");
        let (torn_path, _, _) = operator_receipt_paths(&root, "node-test", &torn).unwrap();
        std::fs::write(&torn_path, b"{\"state\":").unwrap();
        let error = replay_receipted_operator_action(&root, "node-test", &torn)
            .expect_err("torn journal must require recovery");
        assert!(error.to_string().contains("RECOVERY_REQUIRED"), "{error}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn route_inventory_is_closed() {
        assert!(is_http_mutation_path("/v1/agentfirm/team-runs/run-1/works"));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/start"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/teams/team-1/works/work-1/accept"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/nodes/node-1/work-deliveries/delivery-1/reconcile"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/messages/send"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/teams/team-1/works/work-1/request-changes"
        ));
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/browser-invented"
        ));
        for retired in [
            "/v1/team-runs/run-1/works",
            "/v1/team-runs/run-1/works/work-1/assign",
            "/v1/team-runs/run-1/works/work-1/review",
            "/v1/team-runs/run-1/works/work-1/accept",
            "/v1/work-delegations",
            "/v1/work-delegations/delegation-1",
            "/v1/work-delegations/delegation-1/accept",
        ] {
            assert!(is_retired_legacy_write_path(retired), "{retired}");
        }
        for canonical in [
            "/v1/agentfirm/team-runs/run-1/works",
            "/v1/agentfirm/team-runs/run-1/works/work-1/start",
            "/v1/agentfirm/teams/team-1/works/work-1/accept",
        ] {
            assert!(!is_retired_legacy_write_path(canonical), "{canonical}");
        }
    }
}
