//! Closed semantic action adapter for RoleView mutations.
//!
//! The browser may select an action and provide action-specific content, but
//! it never supplies actor identity, Host authority, CAS, idempotency, event
//! identity, or runtime identity. Those are bound from the authenticated
//! transport and the addressed Team/Work at this boundary.

use harness_core::agentfirm_api::{
    ActorKind, CandidateKind, CandidateRef, Confidence, MemberCoordinationStatus, WorkReport,
    WorkReportKind,
};
use harness_core::{
    TeamActorKind, TeamActorRef, Work, WorkClaimMode, WorkCommandContext, WorkCondition, WorkPhase,
    WorkPriority,
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
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum OperatorActionIntent {
    ReconcileDelivery { evidence_ref: String },
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

pub fn is_http_mutation_path(path: &str) -> bool {
    parse_route(path).is_some()
        || parse_accept_route(path).is_some()
        || parse_operator_route(path).is_some()
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
        causation_ref: None,
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
        causation_ref: None,
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

pub fn execute(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    path: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
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
        let OperatorActionIntent::ReconcileDelivery { evidence_ref } = intent;
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
        let _member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
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
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/messages"
        ));
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/request-changes"
        ));
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/browser-invented"
        ));
        assert!(is_retired_legacy_write_path(
            "/v1/team-runs/run-1/works/work-1/accept"
        ));
        assert!(is_retired_legacy_write_path("/v1/work-delegations"));
    }
}
