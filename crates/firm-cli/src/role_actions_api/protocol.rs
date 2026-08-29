use super::*;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::enum_variant_names)]
pub(super) enum RoleActionIntent {
    AcceptWork,
    CreateWork {
        work_id: String,
        title: String,
        #[serde(default)]
        context_markdown: String,
        completion_criteria_markdown: String,
        #[serde(default)]
        eligible_member_ids: Vec<String>,
        #[serde(default)]
        prerequisite_work_ids: Vec<String>,
        #[serde(default = "default_claim_mode")]
        claim_mode: WorkClaimMode,
        #[serde(default = "default_priority")]
        priority: WorkPriority,
    },
    ReplaceWorkDependencies {
        prerequisite_work_ids: Vec<String>,
        reason: String,
    },
    AssignWork {
        /// Canonical assignee: one TeamMembership of the accountable Team.
        membership_id: String,
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
    InterruptMemberRun {
        reason: String,
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
pub(super) enum OperatorActionIntent {
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
    RecoverDaemonPredecessor {
        daemon_id: String,
        instance_id: String,
        daemon_generation: u64,
        provider_process_groups_terminated_confirmed: bool,
        evidence_ref: String,
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

pub(super) fn role_action_scope(
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

pub(super) fn default_true() -> bool {
    true
}
pub(super) fn default_daemon_concurrency() -> usize {
    4
}

pub(super) fn default_claim_mode() -> WorkClaimMode {
    WorkClaimMode::HostAssign
}

pub(super) fn default_priority() -> WorkPriority {
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

pub(super) fn canonical_report_count(
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
pub(super) struct Route<'a> {
    pub(super) team_run_id: &'a str,
    pub(super) work_id: Option<&'a str>,
    pub(super) operation: &'a str,
}

pub(super) fn parse_route(path: &str) -> Option<Route<'_>> {
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
                "assign" | "release" | "cancel" | "claim" | "start" | "block" | "resume" | "submit"
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

pub(super) fn parse_accept_route(path: &str) -> Option<(&str, &str)> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "teams", team_id, "works", work_id, "accept"] => {
            Some((team_id, work_id))
        }
        _ => None,
    }
}

pub(super) fn parse_dependencies_route(path: &str) -> Option<(&str, &str)> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["v1", "agentfirm", "teams", team_id, "works", work_id, "dependencies"] => {
            Some((team_id, work_id))
        }
        _ => None,
    }
}

#[derive(Debug)]
pub(super) enum CanonicalRoute<'a> {
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

pub(super) fn parse_canonical_route(path: &str) -> Option<CanonicalRoute<'_>> {
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
                "interrupt" | "close" | "reopen" | "retire" | "resume-native-session"
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
                "daemon-start"
                    | "daemon-stop"
                    | "daemon-recover-predecessor"
                    | "diagnostics"
                    | "provider-admission"
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

#[derive(Debug)]
pub(crate) struct AuthorizedMemberInterrupt {
    pub team_run_id: String,
    pub member_run_id: String,
    pub requested_by: String,
    pub reason: String,
}

/// Resolve one live-only Interrupt intent against the same authenticated Host
/// or Member authority and exact MemberRun CAS used by durable Role Actions.
/// The returned permit is not an effect receipt: the caller must route it to
/// the owning Supervisor, whose provider-native acknowledgement settles the
/// durable InterruptCurrentCycle RuntimeCommand.
pub(crate) fn authorize_member_interrupt(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    path: &str,
    body: &[u8],
) -> Result<AuthorizedMemberInterrupt, StoreError> {
    let Some(CanonicalRoute::MemberRun {
        member_run_id,
        operation: "interrupt",
    }) = parse_canonical_route(path)
    else {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic Interrupt intent does not match the exact MemberRun route",
            "route",
            path,
            None,
        ));
    };
    let RoleActionIntent::InterruptMemberRun { reason } =
        serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid MemberRun Interrupt intent: {error}"),
                "member_run",
                member_run_id,
                None,
            )
        })?
    else {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match MemberRun Interrupt route",
            "member_run",
            member_run_id,
            None,
        ));
    };
    if reason.trim().is_empty() {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "Interrupt requires a non-empty reason",
            "member_run",
            member_run_id,
            None,
        ));
    }
    let (run, _) = require_member_or_host(store, auth, member_run_id)?;
    if auth.expected_version != run.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "MemberRun Interrupt requires its exact current revision",
            "member_run",
            member_run_id,
            Some(run.version),
        ));
    }
    if run.coordination_status != MemberCoordinationStatus::Active
        || run.runtime_status != harness_core::agentfirm_api::MemberRuntimeStatus::Running
    {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "Interrupt requires an Active MemberRun with one running provider turn",
            "member_run",
            member_run_id,
            Some(run.version),
        ));
    }
    Ok(AuthorizedMemberInterrupt {
        team_run_id: run.team_run_id,
        member_run_id: member_run_id.to_string(),
        requested_by: auth.actor.id.clone(),
        reason,
    })
}

pub fn is_http_mutation_path(path: &str) -> bool {
    parse_route(path).is_some()
        || parse_accept_route(path).is_some()
        || parse_dependencies_route(path).is_some()
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

pub(super) fn encoded_error(
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

pub(super) fn now_string() -> String {
    crate::role_views_api::now()
}

pub(super) fn host_context(
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

pub(super) fn member_context(
    auth: &AuthenticatedMutation,
    member_run_id: &str,
) -> WorkCommandContext {
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

pub(super) fn team_for_run(
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

pub(super) fn is_host(auth: &AuthenticatedMutation, host_id: &str) -> bool {
    (auth.actor.kind == ActorKind::AgentMember && auth.actor.id == host_id)
        || auth
            .authorized_authority_actors
            .iter()
            .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == host_id)
}

pub(super) fn require_host<'a>(
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

pub(super) fn resolve_member_run(
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

pub(super) fn current_work(
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

pub(super) fn current_canonical_work(
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

pub(super) fn require_confirmed(
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

pub(super) fn trust_result(result: crate::agentfirm_api::TrustCommandResult) -> RoleActionResult {
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

pub(super) fn canonical_mutation_result<T: Serialize>(
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

pub(super) fn deterministic_id(kind: &str, auth: &AuthenticatedMutation) -> String {
    format!("{kind}:{}", auth.idempotency_key)
}

pub(super) fn canonical_replay(
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

pub(super) fn active_member_run(
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

pub(super) fn team_for_member_run(
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

pub(super) fn require_member_or_host(
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

pub(super) fn latest_workspace(
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

pub(super) fn observe_workspace_proof(
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
