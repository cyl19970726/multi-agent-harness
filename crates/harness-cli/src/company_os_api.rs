//! HTTP read projection and governed mutation surface for Company OS.
//!
//! All durable writes go through HarnessStore. Custom pages may read the
//! projection and dispatch declared ActionCommands, but never receive a generic
//! store-write primitive.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::{
    ActionCommand, ActionCommandStatus, ActionEffect, ActionPolicyDefinition, ActorRef, ActorType,
    Approval, ApprovalStatus, Assignment, AuditEvent, AuditEventKind, Block, BusinessModule,
    Commitment, CommitmentStatus, CustomPageDefinition, CustomPagePackage, Document, DocumentKind,
    EntityKind, LifecycleStatus, MemberStatus, Milestone, OrgUnit, OrganizationMembership, Payment,
    PendingInteractionStatus, Relation, RiskTier, TypedRecord, ValidateCompanyOs, View, WorkItem,
    WorkItemStatus, WorkQuery,
};
use harness_store::{ActionCommandClaimResult, CompanyActor, HarnessStore, StoreError};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

#[derive(Debug)]
pub struct ApiResponse {
    pub status: &'static str,
    pub body: Value,
}

#[derive(Debug)]
struct ApiError {
    status: &'static str,
    code: &'static str,
    detail: String,
}

impl ApiError {
    fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: "400 Bad Request",
            code: "bad_request",
            detail: detail.into(),
        }
    }
    fn forbidden(detail: impl Into<String>) -> Self {
        Self {
            status: "403 Forbidden",
            code: "forbidden",
            detail: detail.into(),
        }
    }
    fn not_found(detail: impl Into<String>) -> Self {
        Self {
            status: "404 Not Found",
            code: "not_found",
            detail: detail.into(),
        }
    }
    fn conflict(detail: impl Into<String>) -> Self {
        Self {
            status: "409 Conflict",
            code: "conflict",
            detail: detail.into(),
        }
    }
    fn validation(detail: impl Into<String>) -> Self {
        Self {
            status: "422 Unprocessable Entity",
            code: "validation_failed",
            detail: detail.into(),
        }
    }
    fn internal(detail: impl Into<String>) -> Self {
        Self {
            status: "500 Internal Server Error",
            code: "internal_error",
            detail: detail.into(),
        }
    }
    fn response(self) -> ApiResponse {
        ApiResponse {
            status: self.status,
            body: json!({"ok": false, "error": self.code, "detail": self.detail}),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::CompanyOsMissingReference(detail) => Self::not_found(detail),
            StoreError::CompanyOsValidation(detail) => Self::validation(detail),
            StoreError::Conflict(detail) => Self::conflict(detail),
            StoreError::Json(error) => Self::bad_request(error.to_string()),
            StoreError::Io(error) => Self::internal(error.to_string()),
            StoreError::LockTimeout(detail) => Self::conflict(detail),
        }
    }
}

fn finish(result: Result<Value, ApiError>) -> ApiResponse {
    match result {
        Ok(value) => ApiResponse {
            status: "200 OK",
            body: json!({"ok": true, "result": value}),
        },
        Err(error) => error.response(),
    }
}

/// Handle a Company OS GET path. None means the path belongs to another API.
pub fn handle_get(
    store: &HarnessStore,
    execution_store: Option<&HarnessStore>,
    path: &str,
) -> Option<ApiResponse> {
    if path == "/v1/company-os/snapshot" {
        return Some(finish(
            snapshot_with_execution(store, execution_store.unwrap_or(store))
                .map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/work-projection" {
        return Some(finish(
            store
                .work_projection(&WorkQuery::default())
                .map_err(ApiError::from)
                .and_then(|projection| {
                    serde_json::to_value(projection)
                        .map_err(|error| ApiError::internal(error.to_string()))
                }),
        ));
    }
    if path == "/v1/company-os/work-cutover" {
        return Some(finish(
            execution_store
                .unwrap_or(store)
                .work_cutover_report(store)
                .map_err(ApiError::from)
                .and_then(|report| {
                    serde_json::to_value(report)
                        .map_err(|error| ApiError::internal(error.to_string()))
                }),
        ));
    }
    // Read-only archived-source provenance and Docs health projections. They
    // resolve the latest ledger rows only; they never write or migrate rows.
    if path == "/v1/company-os/work-provenance" {
        return Some(finish(
            work_source_provenance(store).map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/organization-provenance" {
        return Some(finish(
            organization_source_provenance(store).map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/docs-health" {
        return Some(finish(docs_health_report(store).map_err(ApiError::from)));
    }
    if let Some(response) = docs_v2_get(store, path) {
        return Some(response);
    }
    let suffix = path.strip_prefix("/v1/company-os/")?;
    let mut parts = suffix.split('/');
    let resource = parts.next().unwrap_or_default();
    let id = parts.next();
    if parts.next().is_some() || resource.is_empty() || resource == "actions" {
        return None;
    }
    Some(finish(read_resource(store, resource, id)))
}

/// Handle a Company OS POST path. None means the path belongs to another API.
pub fn handle_post(
    store: &HarnessStore,
    path: &str,
    body: &Value,
    transport_token: Option<&str>,
) -> Option<ApiResponse> {
    if !path.starts_with("/v1/company-os/") {
        return None;
    }
    // Work queries are read-only projections. They accept a typed filter body
    // but never require or consume the mutation capability token.
    if path == "/v1/company-os/work-query" {
        return Some(finish(parse::<WorkQuery>(body).and_then(|query| {
            store
                .work_projection(&query)
                .map_err(ApiError::from)
                .and_then(|projection| {
                    serde_json::to_value(projection)
                        .map_err(|error| ApiError::internal(error.to_string()))
                })
        })));
    }
    if let Err(error) = authenticate_write_transport(transport_token) {
        return Some(error.response());
    }
    if path == "/v1/company-os/actions/dispatch" {
        return Some(finish(dispatch_action(store, body)));
    }
    if let Some(response) = docs_v2_post(store, path, body) {
        return Some(response);
    }
    let resource = path.strip_prefix("/v1/company-os/")?;
    if resource.is_empty() || resource.contains('/') || resource == "snapshot" {
        return None;
    }
    Some(finish(append_resource(
        store,
        resource,
        body,
        AppendMode::Direct,
    )))
}

fn authenticate_write_transport(token: Option<&str>) -> Result<(), ApiError> {
    let expected = std::env::var("HARNESS_COMPANY_OS_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::forbidden(
                "Company OS writes are disabled until HARNESS_COMPANY_OS_TOKEN is configured",
            )
        })?;
    if token != Some(expected.as_str()) {
        return Err(ApiError::forbidden(
            "missing or invalid Company OS transport capability",
        ));
    }
    Ok(())
}

/// Latest-row-wins projection embedded in the main Dashboard snapshot.
pub fn snapshot(store: &HarnessStore) -> Result<Value, StoreError> {
    snapshot_with_execution(store, store)
}

/// Build Company OS truth from `store` while joining explicit Standing
/// Agent→MemberRun participation from the independently selected Execution
/// Space. No rows are copied or re-owned across the boundary.
pub fn snapshot_with_execution(
    store: &HarnessStore,
    execution_store: &HarnessStore,
) -> Result<Value, StoreError> {
    let actors = normalized_actors(store.latest_actors()?);
    let StandingAssignmentProjection {
        assignments: standing_assignments,
        conflicts: standing_assignment_conflicts,
    } = standing_assignment_projection(store, execution_store)?;
    let work_execution_chains =
        work_execution_projection(store, execution_store, now_unix_millis())?;
    let work_cutover = execution_store.work_cutover_report(store)?;
    let commitments = store.latest_commitments()?;
    let payments = store.latest_payments()?;
    let financial_records = commitments
        .iter()
        .map(|record| {
            json!({
                "id": record.id, "type": "commitment",
                "display_name": "Financial commitment",
                "display_amount": display_money(&record.amount.amount, &record.amount.currency),
                "status": record.status, "record": record,
            })
        })
        .chain(payments.iter().map(|record| {
            json!({
                "id": record.id, "type": "payment", "display_name": "Payment",
                "display_amount": display_money(&record.amount.amount, &record.amount.currency),
                "status": record.status, "record": record,
            })
        }))
        .collect::<Vec<_>>();
    let mut projection = json!({
        "snapshot_contract": "company-os-v1",
        "projection_kind": "live_company_os",
        "actors": actors,
        "documents": store.latest_documents()?,
        "blocks": store.latest_blocks()?,
        "typed_records": store.latest_typed_records()?,
        "relations": store.latest_relations()?,
        "views": store.latest_views()?,
        "business_modules": store.latest_business_modules()?,
        "organization": {
            "org_units": store.latest_org_units()?,
            "memberships": store.latest_organization_memberships()?,
        },
        // ADR 0052 Organization identity is execution-space truth even when
        // this Company Store is joined to a separately selected Execution
        // Space. Keep it distinct from the compatibility runtime `members`
        // projection in the outer Dashboard snapshot.
        "durable_agent_members": execution_store
            .latest_durable_members()?
            .into_values()
            .collect::<Vec<_>>(),
        "milestones": store.latest_milestones()?,
        "work_items": store.latest_work_items()?,
        "work": store.work_projection(&WorkQuery::default())?,
        "assignments": store.latest_assignments()?,
        "standing_assignments": standing_assignments,
        "standing_assignment_conflicts": standing_assignment_conflicts,
        "work_execution_chains": work_execution_chains,
        "work_cutover": work_cutover,
        "approvals": store.latest_approvals()?,
        "financial_records": financial_records,
        "commitments": commitments,
        "payments": payments,
        "custom_page_definitions": store.latest_custom_page_definitions()?,
        "custom_page_packages": store.latest_custom_page_packages()?,
        "action_policy_definitions": store.latest_action_policy_definitions()?,
        "action_commands": store.latest_action_commands()?,
        "audit_events": store.latest_audit_events()?,
        "governance_proposals": [],
    });
    let revision = projection_revision(&projection)?;
    let project_id = store
        .root()
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    projection["source"] = json!({
        "kind": "harness_store",
        "authoritative": true,
        "project_id": project_id,
        "store_root": store.root().to_string_lossy(),
        "schema": "company-os/v1",
        "revision": revision,
        "projection": "latest_row_wins",
    });
    Ok(projection)
}

fn work_execution_projection(
    company_store: &HarnessStore,
    execution_store: &HarnessStore,
    now_ms: u128,
) -> Result<Vec<Value>, StoreError> {
    let assignments = company_store
        .latest_assignments()?
        .into_iter()
        .map(|v| json!(v))
        .collect();
    let agents = company_store
        .latest_standing_agents()?
        .into_iter()
        .map(|v| json!(v))
        .collect();
    let agent_members = execution_store
        .members()?
        .into_iter()
        .fold(BTreeMap::new(), |mut m, v| {
            m.insert(v.id.clone(), json!(v));
            m
        })
        .into_values()
        .collect();
    let members = execution_store
        .member_runs()?
        .into_iter()
        .fold(BTreeMap::new(), |mut m, v| {
            m.insert(v.id.clone(), json!(v));
            m
        })
        .into_values()
        .collect();
    let works = execution_store
        .latest_works()?
        .into_iter()
        .map(|v| json!(v))
        .collect();
    let deliveries = execution_store
        .latest_work_deliveries()?
        .into_iter()
        .map(|v| json!(v))
        .collect();
    let messages = execution_store
        .team_messages()?
        .into_iter()
        .fold(BTreeMap::new(), |mut m, v| {
            m.insert(v.id.clone(), json!(v));
            m
        })
        .into_values()
        .collect();
    let records = company_store
        .latest_typed_records()?
        .into_iter()
        .map(|v| json!(v))
        .collect();
    Ok(build_work_execution_chains(
        assignments,
        agents,
        agent_members,
        members,
        works,
        deliveries,
        messages,
        records,
        now_ms,
    ))
}

fn value_text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn record_text<'a>(value: &'a Value, key: &str) -> &'a str {
    let direct = value_text(value, key);
    if direct.is_empty() {
        value
            .get("fields")
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        direct
    }
}

#[allow(clippy::too_many_arguments)]
fn build_work_execution_chains(
    assignments: Vec<Value>,
    agents: Vec<Value>,
    agent_members: Vec<Value>,
    members: Vec<Value>,
    works: Vec<Value>,
    deliveries: Vec<Value>,
    messages: Vec<Value>,
    records: Vec<Value>,
    now_ms: u128,
) -> Vec<Value> {
    assignments.into_iter().map(|assignment| {
        let assignment_id = value_text(&assignment, "id");
        let work_item_id = value_text(&assignment, "work_item_id");
        let evidence_ref = value_text(&assignment, "delivery_evidence_ref");
        let recipient_id = assignment.get("recipient").and_then(|v| v.get("actor_id")).and_then(Value::as_str).unwrap_or("");
        let standing_agent = agents.iter().find(|v| value_text(v, "id") == recipient_id);
        let agent_member_ref = standing_agent
            .and_then(|v| v.get("execution_agent_member_ref"))
            .and_then(Value::as_str);
        let claimant_count = agent_member_ref.map(|id| agents.iter().filter(|v| v.get("execution_agent_member_ref").and_then(Value::as_str) == Some(id)).count()).unwrap_or(0);
        let durable_member_count = agent_member_ref.map(|id| agent_members.iter().filter(|v| value_text(v, "id") == id).count()).unwrap_or(0);
        let durable_agent_member = agent_member_ref.filter(|_| claimant_count == 1 && durable_member_count == 1);
        let work = works.iter().find(|v| value_text(v, "id") == evidence_ref);
        let work_matches_item = work
            .map(|v| value_text(v, "source_work_item_ref") == work_item_id)
            .unwrap_or(false);
        let work_matches_owner = work
            .and_then(|v| v.get("owner_member_id"))
            .and_then(Value::as_str)
            == durable_agent_member;
        let active_member_run_id = work
            .and_then(|v| v.get("active_member_run_id"))
            .and_then(Value::as_str);
        let member = active_member_run_id.and_then(|id| {
            members.iter().find(|member| {
                value_text(member, "id") == id
                    && member.get("agent_member_id").and_then(Value::as_str)
                        == durable_agent_member
                    && work
                        .map(|work| value_text(member, "team_run_id") == value_text(work, "team_run_id"))
                        .unwrap_or(false)
            })
        });
        let link_status = if evidence_ref.is_empty()
            || work.is_none()
            || agent_member_ref.is_none()
            || durable_member_count == 0
        {
            "unavailable"
        } else if claimant_count != 1
            || durable_member_count != 1
            || !work_matches_item
            || !work_matches_owner
            || member.is_none()
        {
            "mismatch"
        } else {
            "linked"
        };
        let linked_member = if link_status == "linked" { member } else { None };
        let work_delivery = linked_member.and_then(|member| {
            deliveries.iter().find(|delivery| {
                value_text(delivery, "work_id") == evidence_ref
                    && value_text(delivery, "recipient_member_run_id") == value_text(member, "id")
            }).map(|delivery| json!({
                "id": value_text(delivery, "id"),
                "status": value_text(delivery, "status"),
                "attempt": delivery.get("attempt").cloned().unwrap_or(json!(0)),
                "provider_receipt_id": delivery.get("provider_receipt_id").cloned(),
            }))
        });
        let member_run = linked_member.map(|v| {
            let native = v.get("native_session");
            json!({"id": value_text(v, "id"), "status": value_text(v, "status"), "native_session_id": native.and_then(|n| n.get("native_session_id")).cloned(), "native_session_availability": native.map(|n| value_text(n, "availability")).filter(|v| !v.is_empty()).unwrap_or("unavailable")})
        });
        let conversations = messages.iter().filter(|v| link_status == "linked" && value_text(v, "work_id") == evidence_ref && value_text(v, "kind") != "handoff").map(|v| {
            json!({
                "id": value_text(v, "id"),
                "kind": value_text(v, "kind"),
                "from_member_id": value_text(v, "from_member_id"),
                "body": value_text(v, "body"),
                "created_at": value_text(v, "created_at"),
            })
        }).collect::<Vec<_>>();
        let handoffs = messages.iter().filter(|v| link_status == "linked" && value_text(v, "kind") == "handoff" && value_text(v, "work_id") == evidence_ref).map(|v| {
            let body = value_text(v, "body");
            let result = body.lines().find_map(|line| line.strip_prefix("RESULT:").map(str::trim)).unwrap_or("");
            json!({"id": value_text(v, "id"), "result": result, "body": body, "created_at": value_text(v, "created_at"), "evidence_refs": v.get("evidence_refs").cloned().unwrap_or(json!([]))})
        }).collect::<Vec<_>>();
        let external_observations = records.iter().filter_map(|record| {
            let kind = value_text(record, "record_type");
            if kind != "github_pull_request_ref" && kind != "github_check_snapshot" { return None; }
            if link_status != "linked" { return None; }
            if record_text(record, "work_id") != evidence_ref { return None; }
            if !record_text(record, "work_item_id").is_empty() && record_text(record, "work_item_id") != work_item_id { return None; }
            let observed = record_text(record, "observed_unix_ms").parse::<u128>().ok();
            let ttl = record_text(record, "freshness_ttl_ms").parse::<u128>().unwrap_or(3_600_000);
            let freshness = observed.map(|at| if now_ms.saturating_sub(at) <= ttl {"fresh"} else {"stale"}).unwrap_or("unavailable");
            Some(json!({
                "id": value_text(record, "id"),
                "kind": if kind == "github_check_snapshot" {"check"} else {"pull_request"},
                "label": value_text(record, "title"),
                "repository": record_text(record, "repository"),
                "pull_request_number": record_text(record, "pull_request_number"),
                "head_ref": record_text(record, "head_ref"),
                "head_sha": record_text(record, "head_sha"),
                "base_ref": record_text(record, "base_ref"),
                "url": record_text(record, "url"),
                "state": record_text(record, "state"),
                "observed_at": record_text(record, "observed_at"),
                "source_updated_at": record_text(record, "source_updated_at"),
                "source_completed_at": record_text(record, "source_completed_at"),
                "freshness": freshness
            }))
        }).collect::<Vec<_>>();
        json!({
            "assignment_id": assignment_id,
            "work_item_id": work_item_id,
            "work_id": work.map(|work| value_text(work, "id")),
            "assignment_state": value_text(&assignment, "delivery_state"),
            "work_state": work.map(|work| value_text(work, "status")),
            "link_status": link_status,
            "detail": match link_status {
                "linked" => "Company Assignment resolves to exact Agent Team Work, owner MemberRun, and provider-native session binding.",
                "mismatch" => "The linked Work does not match the Company WorkItem, Standing Agent identity, or active MemberRun.",
                _ => "Required Work execution-link evidence is unavailable. delivery_evidence_ref must name an Agent Team Work.",
            },
            "work_delivery": work_delivery,
            "member_run": member_run,
            "conversations": conversations,
            "handoffs": handoffs,
            "external_observations": external_observations,
        })
    }).collect()
}

/// Standing Agent participation join plus any locally degraded link conflicts.
///
/// A duplicate link is a data defect in one Company OS row pair. It must not
/// take down the whole Dashboard snapshot, so it is reported as a visible
/// conflict entry instead of an error.
pub(crate) struct StandingAssignmentProjection {
    pub(crate) assignments: Vec<Value>,
    pub(crate) conflicts: Vec<Value>,
}

/// Read-only join from durable Organization identity to explicitly linked
/// Agent Team participation. It is intentionally rebuilt from latest rows and
/// never infers identity from display names, roles, providers, or timestamps.
///
/// The write path (`append_standing_agent`) still rejects a new duplicate
/// `execution_agent_member_ref`. This read path additionally tolerates a store
/// that already contains one: the affected `agent_member_id` is withheld from
/// the join and surfaced in `conflicts`, while every other Standing Agent still
/// projects normally.
fn standing_assignment_projection(
    company_store: &HarnessStore,
    execution_store: &HarnessStore,
) -> Result<StandingAssignmentProjection, StoreError> {
    // Collect every claimant per member id first so a duplicate is scoped to
    // the ids that actually collide instead of aborting the projection.
    let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for agent in company_store.latest_standing_agents()? {
        let Some(member_id) = agent.execution_agent_member_ref else {
            continue;
        };
        claims.entry(member_id).or_default().push(agent.id);
    }
    let mut standing_agent_links = BTreeMap::new();
    let mut conflicted_links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (member_id, mut standing_agent_ids) in claims {
        if standing_agent_ids.len() == 1 {
            standing_agent_links.insert(member_id, standing_agent_ids.remove(0));
        } else {
            // Ambiguous ownership: refuse to guess a winner.
            standing_agent_ids.sort();
            conflicted_links.insert(member_id, standing_agent_ids);
        }
    }
    let member_runs =
        execution_store
            .member_runs()?
            .into_iter()
            .fold(BTreeMap::new(), |mut latest, member| {
                latest.insert(member.id.clone(), member);
                latest
            });
    let team_runs =
        execution_store
            .team_runs()?
            .into_iter()
            .fold(BTreeMap::new(), |mut latest, run| {
                latest.insert(run.id.clone(), run);
                latest
            });
    // Execution ledgers are append-only revision streams. Company projections
    // must join their latest object state, never every physical JSONL row.
    // Otherwise a message delivery revision duplicates one logical record and
    // can also resurrect stale pending/close state.
    let messages = execution_store
        .team_messages()?
        .into_iter()
        .fold(BTreeMap::new(), |mut latest, message| {
            latest.insert(message.id.clone(), message);
            latest
        })
        .into_values()
        .collect::<Vec<_>>();
    let pending_interactions = execution_store
        .pending_interactions()?
        .into_iter()
        .fold(BTreeMap::new(), |mut latest, interaction| {
            latest.insert(interaction.id.clone(), interaction);
            latest
        })
        .into_values()
        .collect::<Vec<_>>();
    let supervisor_leases = execution_store
        .team_supervisor_leases()?
        .into_iter()
        .fold(BTreeMap::new(), |mut latest, lease| {
            latest.insert(lease.team_run_id.clone(), lease);
            latest
        })
        .into_values()
        .collect::<Vec<_>>();
    let close_requests = execution_store
        .team_member_close_requests()?
        .into_iter()
        .fold(BTreeMap::new(), |mut latest, request| {
            latest.insert(request.member_run_id.clone(), request);
            latest
        })
        .into_values()
        .collect::<Vec<_>>();
    let member_actions = execution_store
        .member_actions()?
        .into_iter()
        .fold(BTreeMap::new(), |mut latest, action| {
            latest.insert(action.id.clone(), action);
            latest
        })
        .into_values()
        .collect::<Vec<_>>();
    let works = execution_store.latest_works()?;

    let mut projection = Vec::new();
    let mut affected_member_runs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for member in member_runs.values() {
        let Some(agent_member_id) = member.agent_member_id.as_deref() else {
            continue;
        };
        if conflicted_links.contains_key(agent_member_id) {
            // Withheld from the join, but never silently dropped: the member run
            // is named in the conflict entry so the loss stays visible.
            affected_member_runs
                .entry(agent_member_id.to_string())
                .or_default()
                .push(member.id.clone());
            continue;
        }
        let Some(standing_agent_id) = standing_agent_links.get(agent_member_id) else {
            continue;
        };
        let Some(team_run) = team_runs.get(&member.team_run_id) else {
            continue;
        };
        let member_works = works
            .iter()
            .filter(|work| {
                work.team_run_id == member.team_run_id
                    && work.active_member_run_id.as_deref() == Some(member.id.as_str())
                    && work.owner_member_id.as_deref() == Some(agent_member_id)
            })
            .collect::<Vec<_>>();
        let inbox_count = messages
            .iter()
            .filter(|message| message.to_member_ids.iter().any(|id| id == &member.id))
            .count();
        let pending_count = pending_interactions
            .iter()
            .filter(|interaction| {
                interaction.member_run_id == member.id
                    && interaction.status == PendingInteractionStatus::Pending
            })
            .count();
        let mut participation_evidence_refs = messages
            .iter()
            .filter(|message| {
                message.to_member_ids.iter().any(|id| id == &member.id)
                    || message.from_member_id == member.id
            })
            .flat_map(|message| message.evidence_refs.iter().cloned())
            .collect::<BTreeSet<_>>();
        for work in &member_works {
            participation_evidence_refs.extend(work.artifact_refs.iter().cloned());
            participation_evidence_refs.extend(work.check_refs.iter().cloned());
        }
        for action in member_actions
            .iter()
            .filter(|action| action.member_run_id == member.id)
        {
            participation_evidence_refs.extend(action.evidence_refs.iter().cloned());
        }
        let participation_evidence_refs =
            participation_evidence_refs.into_iter().collect::<Vec<_>>();
        let supervisor = supervisor_leases
            .iter()
            .rev()
            .find(|lease| lease.team_run_id == member.team_run_id);
        let close = close_requests
            .iter()
            .rev()
            .find(|request| request.member_run_id == member.id);
        let lifecycle = json!({
            "mailbox_message_count": inbox_count,
            "pending_interaction_count": pending_count,
            "supervisor_lease": supervisor,
            "close_request": close,
        });
        let navigation_target =
            format!("?surface=team&team={}&memberRun={}", team_run.id, member.id);
        if member_works.is_empty() {
            projection.push(json!({
                "id": format!("standing-participation:{}", member.id),
                "standing_agent_id": standing_agent_id,
                "agent_member_id": agent_member_id,
                "source_kind": "agent_team_participation",
                "source_ref": null,
                "mission_id": team_run.mission_id,
                "wave_id": team_run.wave_id,
                "team_run_id": team_run.id,
                "member_run_id": member.id,
                "title": format!("{} Agent Team participation", member.name),
                "role": member.role,
                "status": member.status,
                "assigned_at": member.started_at,
                "last_activity_at": member.last_event_at,
                "correlation_id": null,
                "native_session": member.native_session,
                "evidence_refs": participation_evidence_refs,
                "lifecycle": lifecycle,
                "navigation_target": navigation_target,
            }));
        } else {
            for work in member_works {
                let work_evidence_refs = work
                    .artifact_refs
                    .iter()
                    .chain(work.check_refs.iter())
                    .chain(
                        messages
                            .iter()
                            .filter(|message| message.work_id.as_deref() == Some(work.id.as_str()))
                            .flat_map(|message| message.evidence_refs.iter()),
                    )
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                projection.push(json!({
                    "id": format!("standing-work:{}:{}", member.id, work.id),
                    "standing_agent_id": standing_agent_id,
                    "agent_member_id": agent_member_id,
                    "source_kind": "agent_team_work",
                    "source_ref": work.id,
                    "work_id": work.id,
                    "mission_id": team_run.mission_id,
                    "wave_id": team_run.wave_id,
                    "team_run_id": team_run.id,
                    "member_run_id": member.id,
                    "title": work.title,
                    "role": member.role,
                    "status": work.status,
                    "assigned_at": work.created_at,
                    "last_activity_at": member.last_event_at,
                    "correlation_id": null,
                    "evidence_refs": work_evidence_refs,
                    "native_session": member.native_session,
                    "lifecycle": lifecycle,
                    "navigation_target": navigation_target,
                }));
            }
        }
    }
    projection.sort_by(|left, right| {
        left["assigned_at"]
            .as_str()
            .cmp(&right["assigned_at"].as_str())
            .then(left["id"].as_str().cmp(&right["id"].as_str()))
    });
    let conflicts = conflicted_links
        .into_iter()
        .map(|(member_id, standing_agent_ids)| {
            let joined = standing_agent_ids.join(", ");
            json!({
                "id": format!("standing-link-conflict:{member_id}"),
                "kind": "duplicate_execution_agent_member_ref",
                "severity": "error",
                "agent_member_id": member_id,
                "standing_agent_ids": standing_agent_ids,
                "affected_member_run_ids": affected_member_runs
                    .get(&member_id)
                    .cloned()
                    .unwrap_or_default(),
                "detail": format!(
                    "duplicate StandingAgent execution_agent_member_ref {member_id}: {joined}; relation must be one-to-one"
                ),
                "resolution_hint": format!(
                    "harness company org actor unlink-execution --authority <human-id> --actor <one of: {joined}>"
                ),
            })
        })
        .collect::<Vec<_>>();
    Ok(StandingAssignmentProjection {
        assignments: projection,
        conflicts,
    })
}

fn projection_revision(value: &Value) -> Result<String, StoreError> {
    let bytes = serde_json::to_vec(value)?;
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn normalized_actors(actors: Vec<CompanyActor>) -> Vec<Value> {
    actors
        .into_iter()
        .map(|actor| match actor {
            CompanyActor::Human(actor) => json!({
                "id": actor.id, "actor_type": "Human",
                "display_name": actor.display_name, "record": actor,
            }),
            CompanyActor::Agent(actor) => json!({
                "id": actor.id, "actor_type": "Standing Agent",
                "display_name": actor.display_name, "record": actor,
            }),
            CompanyActor::External(actor) => json!({
                "id": actor.id, "actor_type": "External",
                "display_name": actor.display_name_or_organization, "record": actor,
            }),
            CompanyActor::Service(actor) => json!({
                "id": actor.id, "actor_type": "Service",
                "display_name": actor.display_name, "record": actor,
            }),
        })
        .collect()
}

fn display_money(amount: &str, currency: &str) -> String {
    match currency {
        "CNY" => format!("¥{}", amount),
        "USD" => format!("{}{}", "$", amount),
        _ => format!("{} {}", amount, currency),
    }
}

// ---------------------------------------------------------------------------
// Archived-source provenance and Docs health projections (read-only).
//
// Every visible Work source must resolve to an active Document or to explicit
// archived-source history that keeps the document id, title, and lifecycle.
// These projections only read the latest ledger rows; they never write,
// repair, or migrate anything.
// ---------------------------------------------------------------------------

/// Keep in sync with `harness_store::company_os::work_item_is_active`.
fn work_status_is_active(status: WorkItemStatus) -> bool {
    !matches!(
        status,
        WorkItemStatus::Completed
            | WorkItemStatus::Cancelled
            | WorkItemStatus::Archived
            | WorkItemStatus::Draft
    )
}

fn lifecycle_name(status: LifecycleStatus) -> &'static str {
    match status {
        LifecycleStatus::Draft => "draft",
        LifecycleStatus::Active => "active",
        LifecycleStatus::Paused => "paused",
        LifecycleStatus::Completed => "completed",
        LifecycleStatus::Archived => "archived",
    }
}

fn member_status_name(status: MemberStatus) -> &'static str {
    match status {
        MemberStatus::Active => "active",
        MemberStatus::Invited => "invited",
        MemberStatus::Paused => "paused",
        MemberStatus::Ended => "ended",
        MemberStatus::Archived => "archived",
    }
}

struct ActorIndexEntry {
    actor_type: &'static str,
    display_name: String,
    status: MemberStatus,
}

fn actor_index(store: &HarnessStore) -> Result<BTreeMap<String, ActorIndexEntry>, StoreError> {
    Ok(store
        .latest_actors()?
        .into_iter()
        .map(|actor| match actor {
            CompanyActor::Human(member) => (
                member.id.clone(),
                ActorIndexEntry {
                    actor_type: "human",
                    display_name: member.display_name,
                    status: member.status,
                },
            ),
            CompanyActor::Agent(member) => (
                member.id.clone(),
                ActorIndexEntry {
                    actor_type: "agent",
                    display_name: member.display_name,
                    status: member.status,
                },
            ),
            CompanyActor::External(member) => (
                member.id.clone(),
                ActorIndexEntry {
                    actor_type: "external",
                    display_name: member.display_name_or_organization,
                    status: member.status,
                },
            ),
            CompanyActor::Service(member) => (
                member.id.clone(),
                ActorIndexEntry {
                    actor_type: "service",
                    display_name: member.display_name,
                    status: member.status,
                },
            ),
        })
        .collect())
}

fn document_index(store: &HarnessStore) -> Result<BTreeMap<String, Document>, StoreError> {
    Ok(store
        .latest_documents()?
        .into_iter()
        .map(|document| (document.id.clone(), document))
        .collect())
}

/// Resolve one document reference against the latest rows. An archived
/// document keeps its title and lifecycle as explicit archived-source history;
/// a missing document (only possible for legacy or imported rows, since
/// append validation requires the reference) resolves as `missing`.
fn document_ref_resolution(documents: &BTreeMap<String, Document>, document_id: &str) -> Value {
    match documents.get(document_id) {
        Some(document) => json!({
            "document_id": document_id,
            "resolution": lifecycle_name(document.lifecycle_status),
            "title": document.title,
            "lifecycle_status": lifecycle_name(document.lifecycle_status),
            "space_id": document.space_id,
            "updated_at": document.updated_at,
        }),
        None => json!({
            "document_id": document_id,
            "resolution": "missing",
        }),
    }
}

fn actor_ref_resolution(actors: &BTreeMap<String, ActorIndexEntry>, reference: &ActorRef) -> Value {
    match actors.get(&reference.actor_id) {
        Some(actor) => json!({
            "actor_type": actor.actor_type,
            "actor_id": reference.actor_id,
            "resolution": member_status_name(actor.status),
            "display_name": actor.display_name,
            "member_status": member_status_name(actor.status),
        }),
        None => json!({
            "actor_type": reference.actor_type,
            "actor_id": reference.actor_id,
            "resolution": "missing",
        }),
    }
}

/// Read-only provenance for every WorkItem: the source/result Documents and
/// the responsible Organization actors, resolved to active records or explicit
/// archived history. Active Work with an archived or missing source is counted
/// so governance can route a correction instead of degrading to a bare id.
pub fn work_source_provenance(store: &HarnessStore) -> Result<Value, StoreError> {
    let documents = document_index(store)?;
    let actors = actor_index(store)?;
    let work_items = store.latest_work_items()?;
    let mut active = 0_u64;
    let mut active_source_archived = 0_u64;
    let mut active_source_missing = 0_u64;
    let mut archived_actor_references = 0_u64;
    let mut missing_actor_references = 0_u64;
    let items = work_items
        .iter()
        .map(|item| {
            let is_active = work_status_is_active(item.status);
            active += u64::from(is_active);
            let source = document_ref_resolution(&documents, &item.source_document_ref);
            if is_active {
                active_source_archived +=
                    u64::from(source["resolution"].as_str() == Some("archived"));
                active_source_missing +=
                    u64::from(source["resolution"].as_str() == Some("missing"));
            }
            let responsible = std::iter::once(&item.accountable_owner).chain(item.assignees.iter());
            let mut responsible_resolutions = Vec::new();
            for reference in responsible {
                let resolution = actor_ref_resolution(&actors, reference);
                match resolution["resolution"].as_str() {
                    Some("archived" | "ended") => archived_actor_references += 1,
                    Some("missing") => missing_actor_references += 1,
                    _ => {}
                }
                responsible_resolutions.push(resolution);
            }
            let accountable_owner = responsible_resolutions.remove(0);
            json!({
                "work_item_id": item.id,
                "work_item_status": item.status,
                "is_active": is_active,
                "source": source,
                "result": item
                    .result_document_ref
                    .as_deref()
                    .map(|document_id| document_ref_resolution(&documents, document_id)),
                "accountable_owner": accountable_owner,
                "assignees": responsible_resolutions,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "projection_kind": "work_source_provenance",
        "read_only": true,
        "work_items": items,
        "summary": {
            "total": work_items.len(),
            "active": active,
            "active_source_active": active - active_source_archived - active_source_missing,
            "active_source_archived": active_source_archived,
            "active_source_missing": active_source_missing,
            "archived_actor_references": archived_actor_references,
            "missing_actor_references": missing_actor_references,
        },
    }))
}

/// Read-only Organization provenance: every member resolves with its durable
/// member status (archived members stay navigable instead of vanishing), and
/// every Standing Agent maintained-document reference resolves to an active
/// Document or explicit archived-source history.
pub fn organization_source_provenance(store: &HarnessStore) -> Result<Value, StoreError> {
    let documents = document_index(store)?;
    let actors = store.latest_actors()?;
    let mut archived_members = 0_u64;
    let mut maintained_archived = 0_u64;
    let mut maintained_missing = 0_u64;
    let members = actors
        .into_iter()
        .map(|actor| {
            let (id, actor_type, display_name, status, maintained_refs) = match actor {
                CompanyActor::Human(member) => (
                    member.id,
                    "human",
                    member.display_name,
                    member.status,
                    Vec::new(),
                ),
                CompanyActor::Agent(member) => (
                    member.id,
                    "agent",
                    member.display_name,
                    member.status,
                    member.maintained_document_refs,
                ),
                CompanyActor::External(member) => (
                    member.id,
                    "external",
                    member.display_name_or_organization,
                    member.status,
                    Vec::new(),
                ),
                CompanyActor::Service(member) => (
                    member.id,
                    "service",
                    member.display_name,
                    member.status,
                    Vec::new(),
                ),
            };
            archived_members += u64::from(status == MemberStatus::Archived);
            let maintained_documents = maintained_refs
                .iter()
                .map(|document_id| {
                    let resolution = document_ref_resolution(&documents, document_id);
                    maintained_archived +=
                        u64::from(resolution["resolution"].as_str() == Some("archived"));
                    maintained_missing +=
                        u64::from(resolution["resolution"].as_str() == Some("missing"));
                    resolution
                })
                .collect::<Vec<_>>();
            json!({
                "actor_id": id,
                "actor_type": actor_type,
                "display_name": display_name,
                "member_status": member_status_name(status),
                "resolution": member_status_name(status),
                "maintained_documents": maintained_documents,
            })
        })
        .collect::<Vec<_>>();
    let org_units = store
        .latest_org_units()?
        .into_iter()
        .map(|unit| {
            json!({
                "org_unit_id": unit.id,
                "name": unit.name,
                "status": unit.status,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "projection_kind": "organization_source_provenance",
        "read_only": true,
        "members": members,
        "org_units": org_units,
        "summary": {
            "members_total": members.len(),
            "members_archived": archived_members,
            "maintained_documents_archived": maintained_archived,
            "maintained_documents_missing": maintained_missing,
        },
    }))
}

fn relation_is_active(relation: &Relation) -> bool {
    relation.lifecycle_status.as_deref() != Some("archived")
}

fn has_active_relation_between(relations: &[Relation], left: &str, right: &str) -> bool {
    relations.iter().any(|relation| {
        relation_is_active(relation)
            && ((relation.from_ref.id == left && relation.to_ref.id == right)
                || (relation.from_ref.id == right && relation.to_ref.id == left))
    })
}

/// Deterministic Docs health report. It extends the TypedRecord source checks
/// used by `harness company docs health` with Work source provenance findings
/// so an archived or missing source Document is reported with title and
/// lifecycle instead of silently degrading. Finding kinds and severities are
/// kept identical across the CLI, this API, and the Dashboard adapter.
pub fn docs_health_report(store: &HarnessStore) -> Result<Value, StoreError> {
    let documents = store.latest_documents()?;
    let blocks = store.latest_blocks()?;
    let typed_records = store.latest_typed_records()?;
    let relations = store.latest_relations()?;
    let business_modules = store.latest_business_modules()?;
    let work_items = store.latest_work_items()?;
    let document_by_id = documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect::<BTreeMap<_, _>>();

    let mut findings = Vec::new();
    for record in &typed_records {
        match record.source_document_ref.as_deref() {
            None => findings.push(json!({
                "id": format!("missing-source:{}", record.id),
                "kind": "typed_record_missing_source",
                "severity": "warning",
                "subject": {"kind": "typed_record", "id": record.id},
                "recommended_action": "Link the TypedRecord to its originating Document or document the source-less policy."
            })),
            Some(document_id) => match document_by_id.get(document_id) {
                None => findings.push(json!({
                    "id": format!("missing-source-document:{}", record.id),
                    "kind": "typed_record_source_document_missing",
                    "severity": "critical",
                    "subject": {"kind": "typed_record", "id": record.id},
                    "related": {"kind": "document", "id": document_id},
                    "recommended_action": "Restore the source Document or migrate this record through a governed Docs action."
                })),
                Some(document) => {
                    if document.lifecycle_status == LifecycleStatus::Archived {
                        findings.push(json!({
                            "id": format!("archived-source-document:{}", record.id),
                            "kind": "typed_record_source_document_archived",
                            "severity": "warning",
                            "subject": {"kind": "typed_record", "id": record.id},
                            "related": {
                                "kind": "document", "id": document.id,
                                "title": document.title,
                                "lifecycle_status": "archived",
                            },
                            "recommended_action": "The source Document is explicit archived history; keep it read-only or route a successor source through a governed Docs action."
                        }));
                    }
                    if !has_active_relation_between(&relations, &document.id, &record.id) {
                        findings.push(json!({
                            "id": format!("missing-doc-record-relation:{}", record.id),
                            "kind": "missing_document_record_relation",
                            "severity": "warning",
                            "subject": {"kind": "typed_record", "id": record.id},
                            "related": {"kind": "document", "id": document.id},
                            "recommended_action": "Run harness company docs relation link or dispatch a governed relation.append Action."
                        }));
                    }
                }
            },
        }
    }
    for item in &work_items {
        if item.status == WorkItemStatus::Archived {
            continue;
        }
        match document_by_id.get(item.source_document_ref.as_str()) {
            None => findings.push(json!({
                "id": format!("missing-source-document-work:{}", item.id),
                "kind": "work_item_source_document_missing",
                "severity": "critical",
                "subject": {"kind": "work_item", "id": item.id},
                "related": {"kind": "document", "id": item.source_document_ref},
                "recommended_action": "Restore the source Document or migrate this WorkItem to a valid source through a governed Work action."
            })),
            Some(document)
                if document.lifecycle_status == LifecycleStatus::Archived
                    && work_status_is_active(item.status) =>
            {
                findings.push(json!({
                    "id": format!("archived-source-document-work:{}", item.id),
                    "kind": "work_item_source_document_archived",
                    "severity": "warning",
                    "subject": {"kind": "work_item", "id": item.id},
                    "related": {
                        "kind": "document", "id": document.id,
                        "title": document.title,
                        "lifecycle_status": "archived",
                    },
                    "recommended_action": "The source Document is explicit archived history; keep it read-only for provenance or route a successor source through a governed Docs action."
                }));
            }
            Some(_) => {}
        }
    }
    findings.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let critical = findings
        .iter()
        .filter(|finding| finding["severity"].as_str() == Some("critical"))
        .count();
    let warning = findings
        .iter()
        .filter(|finding| finding["severity"].as_str() == Some("warning"))
        .count();
    Ok(json!({
        "projection_kind": "docs_health_report",
        "read_only": true,
        "status": if findings.is_empty() { "pass" } else { "issues" },
        "counts": {
            "documents": documents.len(),
            "blocks": blocks.len(),
            "typed_records": typed_records.len(),
            "relations": relations.len(),
            "business_modules": business_modules.len(),
            "work_items": work_items.len(),
            "findings": findings.len(),
            "critical": critical,
            "warning": warning,
        },
        "findings": findings,
    }))
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use harness_core::{
        AgentTeamRun, DurableAgentMember, DurableAgentMemberStatus, MemberRun, StandingAgent,
    };

    fn standing(id: &str, execution_ref: Option<&str>) -> StandingAgent {
        serde_json::from_value(json!({
            "id": id, "display_name": id, "role": "builder",
            "execution_agent_member_ref": execution_ref,
            "status": "active", "availability": "available",
            "assignment_capacity": 1, "exclusive_assignment_ref": null,
            "membership_refs": [], "responsibility_summary": "Build",
            "capability_refs": [], "system_prompt_ref": null, "tool_refs": [],
            "skill_refs": [], "maintained_document_refs": [],
            "accepted_work_type_refs": [], "escalation_policy_ref": null,
            "permission_policy_refs": [], "runtime_refs": [],
            "native_session_refs": [], "created_at": "1", "updated_at": "1"
        }))
        .unwrap()
    }

    #[test]
    fn snapshot_projects_durable_agent_members_from_the_execution_space() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let company_root = std::env::temp_dir().join(format!("company-snapshot-{nonce}"));
        let execution_root = std::env::temp_dir().join(format!("execution-snapshot-{nonce}"));
        let company_store = HarnessStore::new(&company_root);
        let execution_store = HarnessStore::new(&execution_root);
        company_store.init().unwrap();
        execution_store.init().unwrap();
        execution_store
            .insert_durable_member(&DurableAgentMember {
                id: "root-lead".to_string(),
                name: "Foundation Lead".to_string(),
                description: "Durable-only root Team Lead".to_string(),
                role: "lead".to_string(),
                provider_profile: Some("codex/default".to_string()),
                model: None,
                workspace_policy: None,
                project_binding_id: Some("project-harness".to_string()),
                business_access_ceiling_refs: vec!["company_os.read".to_string()],
                status: DurableAgentMemberStatus::Active,
                created_by_member_id: None,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            })
            .unwrap();

        let projected = snapshot_with_execution(&company_store, &execution_store).unwrap();
        assert_eq!(projected["durable_agent_members"][0]["id"], "root-lead");
        assert_eq!(
            projected["durable_agent_members"][0]["name"],
            "Foundation Lead"
        );
        assert!(
            projected["durable_agent_members"][0]
                .get("runtime_status")
                .is_none(),
            "durable Organization identity must not absorb runtime state"
        );

        let _ = std::fs::remove_dir_all(company_root);
        let _ = std::fs::remove_dir_all(execution_root);
    }

    #[test]
    fn work_execution_chain_requires_exact_links_and_reports_freshness() {
        let assignment = |id: &str, evidence: &str| {
            json!({
                "id": id, "work_item_id": "work-1", "recipient": {"actor_type": "agent", "actor_id": "standing-1"},
                "delivery_state": "acknowledged", "correlation_id": format!("company-{id}"), "delivery_evidence_ref": evidence
            })
        };
        let agents =
            vec![json!({"id": "standing-1", "execution_agent_member_ref": "agent-member-1"})];
        let agent_members = vec![json!({"id": "agent-member-1", "name": "Durable Builder"})];
        let members = vec![
            json!({"id": "member-exact", "team_run_id": "team-1", "agent_member_id": "agent-member-1", "name": "Builder", "status": "active", "native_session": {"native_session_id": "session-1"}}),
            json!({"id": "member-name-only", "name": "Builder", "provider": "codex", "status": "active"}),
        ];
        let works = vec![
            json!({"id": "agent-work-exact", "team_run_id": "team-1", "source_work_item_ref": "work-1", "owner_member_id": "agent-member-1", "active_member_run_id": "member-exact", "status": "review"}),
            json!({"id": "agent-work-wrong-item", "team_run_id": "team-1", "source_work_item_ref": "other-work-item", "owner_member_id": "agent-member-1", "active_member_run_id": "member-exact", "status": "review"}),
            json!({"id": "agent-work-name-only", "team_run_id": "team-1", "source_work_item_ref": "work-1", "owner_member_id": "agent-member-1", "active_member_run_id": "member-name-only", "status": "review"}),
        ];
        let deliveries = vec![
            json!({"id": "delivery-exact", "work_id": "agent-work-exact", "recipient_member_run_id": "member-exact", "status": "provider_received", "attempt": 1, "provider_receipt_id": "receipt-1"}),
        ];
        let messages = vec![
            json!({"id": "message-exact", "kind": "message", "work_id": "agent-work-exact", "from_member_id": "host", "body": "Please clarify the acceptance check.", "created_at": "2026-07-31T00:00:00Z"}),
            json!({"id": "message-other-work", "kind": "message", "work_id": "agent-work-wrong-item", "from_member_id": "host", "body": "Not part of this chain.", "created_at": "2026-07-31T00:00:00Z"}),
            json!({"id": "legacy-assignment-message", "kind": "assignment", "from_member_id": "host", "to_member_ids": ["member-exact"], "body": "Legacy ownership carrier.", "created_at": "2026-07-31T00:00:00Z"}),
            json!({"id": "handoff-1", "kind": "handoff", "work_id": "agent-work-exact", "from_member_id": "member-exact", "body": "RESULT: completed\nDelivery evidence attached.", "created_at": "2026-07-31T00:00:00Z", "evidence_refs": ["check-1"]}),
        ];
        let records = vec![
            json!({"id": "pr-fresh", "record_type": "github_pull_request_ref", "title": "PR", "fields": {"work_item_id": "work-1", "work_id": "agent-work-exact", "repository": "owner/repo", "pull_request_number": "42", "head_ref": "codex/fix", "head_sha": "abc123", "base_ref": "master", "url": "https://example.test/pr/42", "state": "open", "observed_at": "2026-07-31T00:00:00Z", "source_updated_at": "2026-07-30T23:00:00Z", "source_completed_at": "", "observed_unix_ms": "9900", "freshness_ttl_ms": "200"}}),
            json!({"id": "check-stale", "record_type": "github_check_snapshot", "title": "CI", "fields": {"work_item_id": "work-1", "work_id": "agent-work-exact", "observed_unix_ms": "9000", "freshness_ttl_ms": "200"}}),
            json!({"id": "check-unavailable", "record_type": "github_check_snapshot", "title": "Unknown", "fields": {"work_item_id": "work-1", "work_id": "agent-work-exact"}}),
        ];
        let chains = build_work_execution_chains(
            vec![
                assignment("assignment-exact", "agent-work-exact"),
                assignment("assignment-wrong-item", "agent-work-wrong-item"),
                assignment("assignment-name", "agent-work-name-only"),
                assignment("assignment-legacy", "legacy-assignment-message"),
            ],
            agents,
            agent_members,
            members,
            works,
            deliveries,
            messages,
            records,
            10_000,
        );
        assert_eq!(chains[0]["link_status"], "linked");
        assert_eq!(chains[0]["work_id"], "agent-work-exact");
        assert_eq!(chains[0]["work_state"], "review");
        assert_eq!(chains[0]["work_delivery"]["id"], "delivery-exact");
        assert_eq!(
            chains[0]["work_delivery"]["provider_receipt_id"],
            "receipt-1"
        );
        assert_eq!(chains[0]["member_run"]["native_session_id"], "session-1");
        assert_eq!(chains[0]["conversations"][0]["id"], "message-exact");
        assert_eq!(chains[0]["handoffs"][0]["id"], "handoff-1");
        assert_eq!(chains[0]["handoffs"][0]["result"], "completed");
        assert_eq!(
            chains[0]["handoffs"][0]["evidence_refs"],
            json!(["check-1"])
        );
        assert_eq!(chains[0]["external_observations"][0]["freshness"], "fresh");
        assert_eq!(
            chains[0]["external_observations"][0]["repository"],
            "owner/repo"
        );
        assert_eq!(
            chains[0]["external_observations"][0]["pull_request_number"],
            "42"
        );
        assert_eq!(chains[0]["external_observations"][0]["base_ref"], "master");
        assert_eq!(
            chains[0]["external_observations"][0]["url"],
            "https://example.test/pr/42"
        );
        assert_eq!(
            chains[0]["external_observations"][0]["source_updated_at"],
            "2026-07-30T23:00:00Z"
        );
        assert_eq!(chains[0]["external_observations"][1]["freshness"], "stale");
        assert_eq!(
            chains[0]["external_observations"][2]["freshness"],
            "unavailable"
        );
        assert_eq!(chains[1]["link_status"], "mismatch");
        assert!(chains[1]["member_run"].is_null());
        assert_eq!(
            chains[2]["link_status"], "mismatch",
            "matching names/providers must not bind identity"
        );
        assert!(chains[2]["member_run"].is_null());
        assert_eq!(
            chains[3]["link_status"], "unavailable",
            "legacy Assignment Message ids are not Work evidence"
        );
        assert_eq!(chains[3]["conversations"], json!([]));

        let orphan = build_work_execution_chains(
            vec![assignment("assignment-exact", "agent-work-exact")],
            vec![json!({"id": "standing-1", "execution_agent_member_ref": "agent-member-1"})],
            vec![],
            vec![json!({"id": "member-exact", "agent_member_id": "agent-member-1"})],
            vec![
                json!({"id": "agent-work-exact", "team_run_id": "team-1", "source_work_item_ref": "work-1", "owner_member_id": "agent-member-1", "active_member_run_id": "member-exact"}),
            ],
            vec![],
            vec![],
            vec![],
            10_000,
        );
        assert_eq!(
            orphan[0]["link_status"], "unavailable",
            "orphan AgentMember refs must never link"
        );
        assert!(orphan[0]["member_run"].is_null());

        let duplicate_claim = build_work_execution_chains(
            vec![assignment("assignment-exact", "agent-work-exact")],
            vec![
                json!({"id": "standing-1", "execution_agent_member_ref": "agent-member-1"}),
                json!({"id": "standing-duplicate", "execution_agent_member_ref": "agent-member-1"}),
            ],
            vec![json!({"id": "agent-member-1"})],
            vec![
                json!({"id": "member-exact", "team_run_id": "team-1", "agent_member_id": "agent-member-1"}),
            ],
            vec![
                json!({"id": "agent-work-exact", "team_run_id": "team-1", "source_work_item_ref": "work-1", "owner_member_id": "agent-member-1", "active_member_run_id": "member-exact"}),
            ],
            vec![],
            vec![],
            vec![],
            10_000,
        );
        assert_eq!(
            duplicate_claim[0]["link_status"], "mismatch",
            "ambiguous StandingAgent claims must be suppressed"
        );
        assert!(duplicate_claim[0]["member_run"].is_null());

        let duplicate_member = build_work_execution_chains(
            vec![assignment("assignment-exact", "agent-work-exact")],
            vec![json!({"id": "standing-1", "execution_agent_member_ref": "agent-member-1"})],
            vec![
                json!({"id": "agent-member-1", "name": "first"}),
                json!({"id": "agent-member-1", "name": "duplicate"}),
            ],
            vec![
                json!({"id": "member-exact", "team_run_id": "team-1", "agent_member_id": "agent-member-1"}),
            ],
            vec![
                json!({"id": "agent-work-exact", "team_run_id": "team-1", "source_work_item_ref": "work-1", "owner_member_id": "agent-member-1", "active_member_run_id": "member-exact"}),
            ],
            vec![],
            vec![],
            vec![],
            10_000,
        );
        assert_eq!(
            duplicate_member[0]["link_status"], "mismatch",
            "duplicate durable AgentMember identities must be suppressed"
        );
        assert!(duplicate_member[0]["member_run"].is_null());
    }

    #[test]
    fn explicit_projection_is_lossless_and_never_same_id_binds() {
        let root = std::env::temp_dir().join(format!(
            "company-projection-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        store.init().unwrap();
        store
            .append_standing_agent(&standing("standing-linked", Some("member-linked")))
            .unwrap();
        store
            .append_standing_agent(&standing("member-collision", None))
            .unwrap();
        let run: AgentTeamRun = serde_json::from_value(json!({
            "id": "run", "host_surface": "test", "objective": "projection",
            "status": "running", "member_run_ids": ["run-linked", "run-collision"],
            "created_at": "1", "updated_at": "1"
        }))
        .unwrap();
        store.append_team_run(&run).unwrap();
        for (id, agent_member_id) in [
            ("run-linked", "member-linked"),
            ("run-collision", "member-collision"),
        ] {
            let member: MemberRun = serde_json::from_value(json!({
                "id": id, "team_run_id": "run", "agent_member_id": agent_member_id,
                "name": id, "role": "builder", "provider": "codex",
                "status": "idle", "owned_paths": [], "started_at": "1"
            }))
            .unwrap();
            store.append_member_run(&member).unwrap();
        }
        let initial = standing_assignment_projection(&store, &store)
            .unwrap()
            .assignments;
        assert_eq!(initial.len(), 1, "same-id collision must not bind");
        assert_eq!(initial[0]["source_kind"], "agent_team_participation");
        assert_eq!(initial[0]["standing_agent_id"], "standing-linked");

        for (id, created_at) in [("work-1", "2"), ("work-2", "3")] {
            store
                .insert_work(
                    harness_core::Work {
                        id: id.to_string(),
                        team_run_id: "run".to_string(),
                        team_id: None,
                        created_by_member_id: None,
                        parent_work_id: None,
                        source_work_item_ref: None,
                        title: id.to_string(),
                        context_markdown: "projection".to_string(),
                        completion_criteria_markdown: "done".to_string(),
                        status: harness_core::WorkStatus::Open,
                        owner_member_id: Some("member-linked".to_string()),
                        active_member_run_id: Some("run-linked".to_string()),
                        claim_mode: harness_core::WorkClaimMode::HostAssign,
                        eligible_member_ids: Vec::new(),
                        prerequisite_work_ids: Vec::new(),
                        priority: harness_core::WorkPriority::Normal,
                        created_by_actor: harness_core::TeamActorRef {
                            kind: harness_core::TeamActorKind::Host,
                            id: "host".to_string(),
                            display_name: None,
                            authn_source: Some("test".to_string()),
                        },
                        result_summary: None,
                        blocker_reason: None,
                        artifact_refs: vec![format!("evidence-{id}")],
                        check_refs: Vec::new(),
                        version: 0,
                        created_at: String::new(),
                        updated_at: String::new(),
                    },
                    harness_core::WorkCommandContext {
                        event_id: format!("event-{id}"),
                        performed_by_actor: harness_core::TeamActorRef {
                            kind: harness_core::TeamActorKind::Host,
                            id: "host".to_string(),
                            display_name: None,
                            authn_source: Some("test".to_string()),
                        },
                        authority_actor: None,
                        causation_ref: None,
                        idempotency_key: format!("command-{id}"),
                        created_at: created_at.to_string(),
                        duplicate_ok: false,
                    },
                )
                .unwrap();
        }
        let projected = standing_assignment_projection(&store, &store).unwrap();
        let assigned = projected.assignments;
        assert_eq!(assigned.len(), 2);
        assert_eq!(assigned[0]["source_ref"], "work-1");
        assert_eq!(assigned[1]["source_ref"], "work-2");
        assert_eq!(assigned[0]["standing_agent_id"], "standing-linked");
        assert_eq!(
            assigned[1]["evidence_refs"],
            json!(["evidence-work-2"]),
            "each Standing Agent Work card must not absorb evidence from sibling Work"
        );
        assert!(
            projected.conflicts.is_empty(),
            "a healthy store must report an empty conflict list"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    /// Test-only: append a row the governed write path refuses, so the read
    /// path can be exercised against a store that already carries the defect
    /// (legacy import, hand edit, or a racing writer).
    fn force_duplicate_link_row(store: &HarnessStore, agent: &StandingAgent) {
        use std::io::Write as _;

        let path = store.root().join("company_os_standing_agents.jsonl");
        let mut ledger = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open standing agent ledger");
        writeln!(ledger, "{}", serde_json::to_string(agent).unwrap())
            .expect("append duplicate standing agent row");
    }

    #[test]
    fn duplicate_execution_link_degrades_locally_instead_of_failing_the_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "company-duplicate-link-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        store.init().unwrap();
        // The write path refuses to create this state, so reproduce the defect
        // the way a real store reaches it: an already-persisted duplicate pair.
        store
            .append_standing_agent(&standing("standing-healthy", Some("member-healthy")))
            .unwrap();
        store
            .append_standing_agent(&standing("standing-dup-a", Some("member-shared")))
            .unwrap();
        let rejected =
            store.append_standing_agent(&standing("standing-dup-b", Some("member-shared")));
        assert!(
            rejected.is_err(),
            "write path must still reject a new duplicate link"
        );
        force_duplicate_link_row(&store, &standing("standing-dup-b", Some("member-shared")));

        let run: AgentTeamRun = serde_json::from_value(json!({
            "id": "run", "host_surface": "test", "objective": "degrade",
            "status": "running", "member_run_ids": ["run-healthy", "run-shared"],
            "created_at": "1", "updated_at": "1"
        }))
        .unwrap();
        store.append_team_run(&run).unwrap();
        for (id, agent_member_id) in [
            ("run-healthy", "member-healthy"),
            ("run-shared", "member-shared"),
        ] {
            let member: MemberRun = serde_json::from_value(json!({
                "id": id, "team_run_id": "run", "agent_member_id": agent_member_id,
                "name": id, "role": "builder", "provider": "codex",
                "status": "idle", "owned_paths": [], "started_at": "1"
            }))
            .unwrap();
            store.append_member_run(&member).unwrap();
        }

        let projected = standing_assignment_projection(&store, &store)
            .expect("duplicate link must not fail the projection");
        assert_eq!(
            projected.assignments.len(),
            1,
            "the healthy Standing Agent must still project"
        );
        assert_eq!(
            projected.assignments[0]["standing_agent_id"],
            "standing-healthy"
        );
        assert_eq!(projected.conflicts.len(), 1);
        let conflict = &projected.conflicts[0];
        assert_eq!(conflict["kind"], "duplicate_execution_agent_member_ref");
        assert_eq!(conflict["agent_member_id"], "member-shared");
        assert_eq!(
            conflict["standing_agent_ids"],
            json!(["standing-dup-a", "standing-dup-b"]),
            "both claimants must be named; no winner is guessed"
        );
        assert_eq!(
            conflict["affected_member_run_ids"],
            json!(["run-shared"]),
            "withheld participation must stay visible"
        );

        // The whole Company OS snapshot must still succeed.
        let snapshot = snapshot_with_execution(&store, &store)
            .expect("snapshot must survive a duplicate link");
        assert_eq!(
            snapshot["standing_assignment_conflicts"],
            json!(projected.conflicts)
        );
        assert_eq!(
            snapshot["standing_assignments"],
            json!(projected.assignments)
        );
        assert_eq!(snapshot["work_cutover"]["valid"], true);
        let cutover = handle_get(&store, Some(&store), "/v1/company-os/work-cutover").unwrap();
        assert_eq!(cutover.status, "200 OK");
        assert_eq!(cutover.body["result"]["valid"], true);
        let response = handle_get(&store, Some(&store), "/v1/company-os/snapshot").unwrap();
        assert_eq!(
            response.status, "200 OK",
            "a duplicate link must not 409 the entire snapshot endpoint"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

fn read_resource(
    store: &HarnessStore,
    resource: &str,
    id: Option<&str>,
) -> Result<Value, ApiError> {
    let values = match resource {
        "documents" => to_values(store.latest_documents()?)?,
        "blocks" => to_values(store.latest_blocks()?)?,
        "typed-records" => to_values(store.latest_typed_records()?)?,
        "relations" => to_values(store.latest_relations()?)?,
        "views" => to_values(store.latest_views()?)?,
        "business-modules" => to_values(store.latest_business_modules()?)?,
        "actors" => store
            .latest_actors()?
            .into_iter()
            .map(|actor| {
                serde_json::to_value(actor).map_err(|error| ApiError::internal(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        "org-units" => to_values(store.latest_org_units()?)?,
        "memberships" => to_values(store.latest_organization_memberships()?)?,
        "milestones" => to_values(store.latest_milestones()?)?,
        "work-items" => to_values(store.latest_work_items()?)?,
        "assignments" => to_values(store.latest_assignments()?)?,
        "approvals" => to_values(store.latest_approvals()?)?,
        "commitments" => to_values(store.latest_commitments()?)?,
        "payments" => to_values(store.latest_payments()?)?,
        "custom-page-definitions" => to_values(store.latest_custom_page_definitions()?)?,
        "custom-page-packages" => to_values(store.latest_custom_page_packages()?)?,
        "action-policies" => to_values(store.latest_action_policy_definitions()?)?,
        "action-commands" => to_values(store.latest_action_commands()?)?,
        "audit-events" => to_values(store.latest_audit_events()?)?,
        _ => {
            return Err(ApiError::not_found(format!(
                "unknown Company OS resource: {resource}"
            )))
        }
    };
    match id {
        None => Ok(json!({"count": values.len(), "items": values})),
        Some(id) => values
            .into_iter()
            .find(|value| value_id(value) == Some(id))
            .ok_or_else(|| ApiError::not_found(format!("{resource}:{id}"))),
    }
}

fn to_values<T: Serialize>(values: Vec<T>) -> Result<Vec<Value>, ApiError> {
    values
        .into_iter()
        .map(|value| {
            serde_json::to_value(value).map_err(|error| ApiError::internal(error.to_string()))
        })
        .collect()
}

fn value_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).or_else(|| {
        value
            .get("actor")
            .and_then(|actor| actor.get("id"))
            .and_then(Value::as_str)
    })
}

#[derive(Clone, Copy)]
enum AppendMode {
    Direct,
    GovernedAction,
}

const COMPANY_OS_ADMIN_PERMISSION: &str = "company_os.admin";

/// Prove that `authority` may perform an administrative Company OS actor write,
/// without appending anything.
///
/// A relation command that finds nothing to change must still authorize:
/// "no row was written" is a valid success only for an operator who was
/// entitled to attempt the write. Returns the same failure detail a rejected
/// append would surface, so callers report one consistent reason.
pub fn authorize_administrative_actor_write(
    store: &HarnessStore,
    authority: &Value,
) -> Result<(), String> {
    administrative_actor_write_authority(store, authority).map_err(|error| error.detail)
}

fn administrative_actor_write_authority(
    store: &HarnessStore,
    authority: &Value,
) -> Result<(), ApiError> {
    let authority: ActorRef = serde_json::from_value(authority.clone())
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if authority.actor_type != ActorType::Human {
        return Err(ApiError::forbidden(
            "sensitive append authority must be a Human",
        ));
    }
    require_active_actor(store, &authority)?;
    require_permission(store, &authority, COMPANY_OS_ADMIN_PERMISSION)?;
    Ok(())
}

fn authorize_direct_append<'a>(
    store: &HarnessStore,
    resource: &str,
    body: &'a Value,
    mode: AppendMode,
) -> Result<&'a Value, ApiError> {
    if matches!(mode, AppendMode::GovernedAction) {
        return Ok(body);
    }
    if resource == "payments" {
        return Err(ApiError::forbidden(
            "Payment is a governed effect; use a declared payment.append ActionCommand",
        ));
    }

    // The transport token authenticates the local operator. The first Human
    // root is the only write that does not also need an administrative envelope;
    // subsequent authoring requires an active Human Company OS authority.
    if resource == "actors" && store.latest_actors()?.is_empty() {
        let actor: CompanyActor = parse(body)?;
        let bootstrap_ok = matches!(&actor, CompanyActor::Human(human)
            if human.status == MemberStatus::Active
                && (human.permission_policy_refs.iter().any(|value| value == COMPANY_OS_ADMIN_PERMISSION)
                    || human.authority_policy_refs.iter().any(|value| value == COMPANY_OS_ADMIN_PERMISSION)));
        if bootstrap_ok {
            return Ok(body);
        }
        return Err(ApiError::forbidden(
            "the first Company OS actor must be an active Human root with company_os.admin",
        ));
    }

    if body.get("mode").and_then(Value::as_str) != Some("administrative") {
        return Err(ApiError::forbidden(
            "direct append is an administrative import surface; custom pages must dispatch declared Actions",
        ));
    }

    let authority: ActorRef = body
        .get("authority")
        .cloned()
        .ok_or_else(|| ApiError::forbidden("sensitive append requires authority"))
        .and_then(|value| {
            serde_json::from_value(value).map_err(|error| ApiError::bad_request(error.to_string()))
        })?;
    if authority.actor_type != ActorType::Human {
        return Err(ApiError::forbidden(
            "sensitive append authority must be a Human",
        ));
    }
    require_active_actor(store, &authority)?;
    require_permission(store, &authority, COMPANY_OS_ADMIN_PERMISSION)?;
    let record = body
        .get("record")
        .ok_or_else(|| ApiError::bad_request("sensitive append requires record"))?;
    if resource == "approvals" {
        let approval: Approval = parse(record)?;
        if approval.status != ApprovalStatus::Requested {
            return Err(ApiError::forbidden(
                "direct Approval append may only create requested state; decisions use approval.decide",
            ));
        }
    }
    if resource == "commitments" {
        let commitment: Commitment = parse(record)?;
        if commitment.status != CommitmentStatus::Proposed {
            return Err(ApiError::forbidden(
                "direct Commitment append may only create proposed state; transitions use commitment.append",
            ));
        }
    }
    Ok(record)
}

fn append_resource(
    store: &HarnessStore,
    resource: &str,
    body: &Value,
    mode: AppendMode,
) -> Result<Value, ApiError> {
    let body = authorize_direct_append(store, resource, body, mode)?;
    macro_rules! append {
        ($type:ty, $method:ident) => {{
            let record: $type = parse(body)?;
            store.$method(&record)?;
            serde_json::to_value(record).map_err(|error| ApiError::internal(error.to_string()))
        }};
    }
    match resource {
        "documents" => append!(Document, append_document),
        "blocks" => append!(Block, append_block),
        "typed-records" => append!(TypedRecord, append_typed_record),
        "relations" => append!(Relation, append_relation),
        "views" => append!(View, append_view),
        "business-modules" => append!(BusinessModule, append_business_module),
        "actors" => {
            let actor: CompanyActor = parse(body)?;
            store.append_actor(&actor)?;
            serde_json::to_value(actor).map_err(|error| ApiError::internal(error.to_string()))
        }
        "org-units" => append!(OrgUnit, append_org_unit),
        "memberships" => append!(OrganizationMembership, append_organization_membership),
        "milestones" => append!(Milestone, append_milestone),
        "work-items" => append!(WorkItem, append_work_item),
        "assignments" => append!(Assignment, append_assignment),
        "approvals" => append!(Approval, append_approval),
        "commitments" => append!(Commitment, append_commitment),
        "payments" => {
            let payment: Payment = parse(body)?;
            validate_payment_governance(store, &payment)?;
            store.append_payment(&payment)?;
            serde_json::to_value(payment).map_err(|error| ApiError::internal(error.to_string()))
        }
        "custom-page-definitions" => {
            let definition: CustomPageDefinition = parse(body)?;
            let policies = policies_for_definition(&definition)?;
            store.append_custom_page_bundle_atomic(&definition, &policies)?;
            serde_json::to_value(definition).map_err(|error| ApiError::internal(error.to_string()))
        }
        "custom-page-packages" => append!(CustomPagePackage, append_custom_page_package),
        _ => Err(ApiError::not_found(format!(
            "unknown Company OS resource: {resource}"
        ))),
    }
}

fn parse<T: DeserializeOwned>(body: &Value) -> Result<T, ApiError> {
    serde_json::from_value(body.clone()).map_err(|error| ApiError::bad_request(error.to_string()))
}

fn dispatch_action(store: &HarnessStore, body: &Value) -> Result<Value, ApiError> {
    let mut command: ActionCommand = parse(body)?;
    command
        .validate()
        .map_err(|error| ApiError::validation(error.to_string()))?;
    if command.status != ActionCommandStatus::Requested {
        return Err(ApiError::conflict(
            "an ActionCommand dispatch request must start in requested status",
        ));
    }
    if command.audit_event_refs.is_empty() {
        return Err(ApiError::validation(
            "ActionCommand.audit_event_refs must name the durable audit event before dispatch",
        ));
    }
    ensure_audit_refs_do_not_squat_terminal_namespace(&command)?;
    let definition_id = command
        .payload
        .get("definition_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("ActionCommand.payload.definition_id is required"))?
        .to_string();
    let record = command
        .payload
        .get("record")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("ActionCommand.payload.record is required"))?;
    if let Some(existing) = store.latest_action_command(&command.id)? {
        if !same_dispatch_request(&existing, &command) {
            return Err(ApiError::conflict(format!(
                "ActionCommand id {} is already bound to another request",
                command.id
            )));
        }
        match existing.status {
            ActionCommandStatus::Executed => {
                return Ok(json!({
                    "command": existing,
                    "record": record,
                    "idempotent_replay": true,
                    "declaration_id": definition_id,
                }))
            }
            ActionCommandStatus::Authorized => {
                return execute_authorized_action(store, existing, &record, &definition_id, true)
            }
            ActionCommandStatus::Requested => {}
            // A denied command stays denied. Replaying it must repeat the
            // refusal rather than degrade into a generic conflict that hides
            // why the attempt was refused.
            ActionCommandStatus::Rejected => {
                return Err(ApiError::forbidden(format!(
                    "ActionCommand {} was already denied; see AuditEvent {}:rejected",
                    existing.id, existing.id
                )))
            }
            _ => {
                return Err(ApiError::conflict(format!(
                    "ActionCommand {} is already {:?}",
                    existing.id, existing.status
                )))
            }
        }
    }
    if !store.company_entity_exists(&command.subject_ref)? {
        return Err(ApiError::not_found(format!(
            "action subject {:?}:{}",
            command.subject_ref.kind, command.subject_ref.id
        )));
    }
    let declaration = store
        .latest_custom_page_definitions()?
        .into_iter()
        .find(|definition| definition.id == definition_id)
        .ok_or_else(|| ApiError::not_found(format!("CustomPageDefinition:{definition_id}")))?;
    if !declaration
        .action_command_refs
        .contains(&command.command_name)
    {
        return Err(ApiError::forbidden(format!(
            "ActionCommand {} is outside declaration {}",
            command.command_name, declaration.id
        )));
    }
    if !declaration.policy_refs.contains(&command.policy_ref) {
        return Err(ApiError::forbidden(format!(
            "policy {} is outside declaration {}",
            command.policy_ref, declaration.id
        )));
    }
    let (policy, effect) = registered_action_policy(store, &command, &declaration, &record)?;
    command
        .validate_against_policy(&policy, effect)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;
    require_active_actor(store, &command.requested_by)?;
    require_permission(store, &command.requested_by, &policy.required_permission)?;
    if policy.requires_human_approval {
        if commitment_enters_approval_queue(store, &command, &record)? {
            require_requested_human_approval(store, &command)?;
        } else {
            require_human_approval(store, &command)?;
        }
    }
    validate_definition_scope(store, &declaration, &command, &record)?;
    if command.command_name == "approval.decide" {
        validate_approval_decision(store, &command, &record)?;
    }
    if command.command_name == "approval.request" {
        validate_approval_request(store, &command, &record)?;
    }
    if command.command_name == "work_item.transition" {
        validate_work_item_transition(store, &command, &record)?;
    }
    if command.command_name == "work_item.update" {
        validate_work_item_update(store, &command, &record)?;
    }
    if command.command_name == "work_item.append" {
        validate_work_item_create(store, &command, &record)?;
    }
    if command.command_name == "assignment.append" {
        validate_assignment_create(store, &command, &record)?;
    }
    if command.command_name == "commitment.propose" {
        validate_commitment_proposal(store, &command, &record)?;
    }
    if command.command_name == "typed_record.append" {
        validate_typed_record_append(store, &command, &record)?;
    }
    if command.command_name == "relation.append" {
        validate_relation_append(store, &command, &record)?;
    }
    ensure_authorization_audit_ids_available(store, &command, &record)?;
    let audit_reservations = action_audit_reservation_ids(&command);
    match store.claim_action_command_with_audit_reservations(&command, &audit_reservations)? {
        ActionCommandClaimResult::Claimed(_) => {
            command.status = ActionCommandStatus::Authorized;
            let events = build_action_audits(
                &command,
                AuditEventKind::PolicyAuthorized,
                &record,
                &command.audit_event_refs,
            );
            store.authorize_action_command_atomic(&command, &events)?;
        }
        ActionCommandClaimResult::Replay(existing) => {
            if existing.status != ActionCommandStatus::Requested {
                return Err(ApiError::conflict(format!(
                    "ActionCommand {} changed while authorizing",
                    existing.id
                )));
            }
            command.status = ActionCommandStatus::Authorized;
            let events = build_action_audits(
                &command,
                AuditEventKind::PolicyAuthorized,
                &record,
                &command.audit_event_refs,
            );
            store.authorize_action_command_atomic(&command, &events)?;
        }
        ActionCommandClaimResult::Conflict(existing) => {
            return Err(ApiError::conflict(format!(
                "ActionCommand id {} already belongs to {}",
                existing.id, existing.command_name
            )))
        }
    }
    execute_authorized_action(store, command, &record, &declaration.id, false)
}

fn execute_authorized_action(
    store: &HarnessStore,
    mut command: ActionCommand,
    record: &Value,
    declaration_id: &str,
    resuming: bool,
) -> Result<Value, ApiError> {
    let executed_audit_id = format!("{}:executed", command.id);
    let failed_audit_id = format!("{}:failed", command.id);
    store.reserve_action_audit_ids(&command.id, &action_audit_reservation_ids(&command))?;
    ensure_authorization_audit_ids_available(store, &command, record)?;
    ensure_terminal_audit_ids_available(
        store,
        &command,
        record,
        [
            (&executed_audit_id, AuditEventKind::Executed),
            (&failed_audit_id, AuditEventKind::Failed),
        ],
    )?;
    let result = match dispatch_declared_record(store, &command, record, resuming) {
        Ok(result) => result,
        Err(error) => {
            let terminal_ref = vec![failed_audit_id.clone()];
            command.audit_event_refs.push(failed_audit_id);
            command.status = ActionCommandStatus::Failed;
            command.completed_at = Some(now_string());
            let events =
                build_action_audits(&command, AuditEventKind::Failed, record, &terminal_ref);
            store.finish_action_command_atomic(&command, &events)?;
            return Err(error);
        }
    };
    command.audit_event_refs.push(executed_audit_id);
    command.status = ActionCommandStatus::Executed;
    command.completed_at = Some(now_string());
    command
        .validate()
        .map_err(|error| ApiError::validation(error.to_string()))?;
    let terminal_ref = command
        .audit_event_refs
        .last()
        .cloned()
        .into_iter()
        .collect::<Vec<_>>();
    let events = build_action_audits(&command, AuditEventKind::Executed, record, &terminal_ref);
    store.finish_action_command_atomic(&command, &events)?;
    Ok(json!({"command": command, "record": result, "declaration_id": declaration_id}))
}

/// Suffixes the server derives from an owning command id when it writes a
/// terminal audit observation.
const RESERVED_AUDIT_SUFFIXES: [&str; 3] = ["executed", "failed", "rejected"];

/// A command may not declare a terminal audit id belonging to a different
/// command. Without this, any Actor able to dispatch one governed action could
/// pre-bind `<victim>:rejected` and suppress the victim command's denial
/// evidence, degrading a governed refusal into a generic conflict and pointing
/// an auditor at an unrelated command's authorization event.
fn ensure_audit_refs_do_not_squat_terminal_namespace(
    command: &ActionCommand,
) -> Result<(), ApiError> {
    for reference in &command.audit_event_refs {
        let Some((owner, suffix)) = reference.rsplit_once(':') else {
            continue;
        };
        if RESERVED_AUDIT_SUFFIXES.contains(&suffix) && owner != command.id {
            return Err(ApiError::forbidden(format!(
                "ActionCommand {} cannot declare audit event id {reference}: the \
                 :{suffix} namespace belongs to ActionCommand {owner}",
                command.id
            )));
        }
    }
    Ok(())
}

fn action_audit_reservation_ids(command: &ActionCommand) -> Vec<String> {
    let mut ids = command.audit_event_refs.clone();
    ids.push(format!("{}:executed", command.id));
    ids.push(format!("{}:failed", command.id));
    ids
}

fn same_dispatch_request(existing: &ActionCommand, requested: &ActionCommand) -> bool {
    existing.id == requested.id
        && existing.command_name == requested.command_name
        && existing.subject_ref == requested.subject_ref
        && existing.requested_by == requested.requested_by
        && existing.payload == requested.payload
        && existing.required_permission == requested.required_permission
        && existing.policy_ref == requested.policy_ref
        && existing.risk_tier == requested.risk_tier
        && existing.requires_human_approval == requested.requires_human_approval
        && existing.approval_refs == requested.approval_refs
        && existing
            .audit_event_refs
            .starts_with(&requested.audit_event_refs)
        && existing.requested_at == requested.requested_at
}

fn ensure_terminal_audit_ids_available<'a>(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
    ids: impl IntoIterator<Item = (&'a String, AuditEventKind)>,
) -> Result<(), ApiError> {
    let existing = store.latest_audit_events()?;
    for (id, expected_kind) in ids {
        if command.audit_event_refs.contains(id) {
            return Err(ApiError::conflict(format!(
                "audit event id {id} is reserved for terminal Action state"
            )));
        }
        if let Some(event) = existing.iter().find(|event| event.id == *id) {
            if !audit_observation_matches(event, command, expected_kind, record) {
                return Err(ApiError::conflict(format!(
                    "audit event id {id} already belongs to another observation"
                )));
            }
        }
    }
    Ok(())
}

fn ensure_authorization_audit_ids_available(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let existing = store.latest_audit_events()?;
    for id in &command.audit_event_refs {
        if let Some(event) = existing.iter().find(|event| event.id == *id) {
            if !audit_observation_matches(event, command, AuditEventKind::PolicyAuthorized, record)
            {
                return Err(ApiError::conflict(format!(
                    "audit event id {id} already belongs to another observation"
                )));
            }
        }
    }
    Ok(())
}

fn audit_observation_matches(
    event: &AuditEvent,
    command: &ActionCommand,
    event_kind: AuditEventKind,
    record: &Value,
) -> bool {
    let evidence_refs = record
        .get("evidence_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    event.action_command_id == command.id
        && event.event_kind == event_kind
        && event.actor_ref == command.requested_by
        && event.subject_ref == command.subject_ref
        && event.detail
            == json!({
                "command_name": command.command_name,
                "policy_ref": command.policy_ref,
                "target_id": value_id(record),
            })
        && event.evidence_refs == evidence_refs
}

fn build_action_audits(
    command: &ActionCommand,
    event_kind: AuditEventKind,
    record: &Value,
    event_ids: &[String],
) -> Vec<AuditEvent> {
    let evidence_refs = record
        .get("evidence_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    event_ids
        .iter()
        .map(|event_id| AuditEvent {
            id: event_id.clone(),
            action_command_id: command.id.clone(),
            event_kind,
            actor_ref: command.requested_by.clone(),
            subject_ref: command.subject_ref.clone(),
            detail: json!({
                "command_name": command.command_name,
                "policy_ref": command.policy_ref,
                "target_id": value_id(record),
            }),
            evidence_refs: evidence_refs.clone(),
            occurred_at: command.requested_at.clone(),
        })
        .collect()
}

fn registered_action_policy(
    store: &HarnessStore,
    command: &ActionCommand,
    definition: &CustomPageDefinition,
    record: &Value,
) -> Result<(ActionPolicyDefinition, ActionEffect), ApiError> {
    let (_, _, _, _, effect) = server_action_shape(&command.command_name)?;
    if command.command_name == "payment.append" && record.get("related_commitment_refs").is_none() {
        return Err(ApiError::validation(
            "payment.append requires related_commitment_refs",
        ));
    }
    let policy = store
        .latest_action_policy_definitions()?
        .into_iter()
        .find(|policy| policy.id == command.policy_ref)
        .ok_or_else(|| {
            ApiError::not_found(format!("ActionPolicyDefinition:{}", command.policy_ref))
        })?;
    if policy.definition_ref != definition.id || policy.module_ref != definition.module_id {
        return Err(ApiError::forbidden(
            "Action policy is outside the selected definition/module scope",
        ));
    }
    Ok((policy, effect))
}

type ServerActionShape = (&'static str, RiskTier, bool, Vec<ActorType>, ActionEffect);

fn server_action_shape(command_name: &str) -> Result<ServerActionShape, ApiError> {
    Ok(match command_name {
        "typed_record.append" | "view.append" | "work_item.append" | "assignment.append" => (
            "company.records.write",
            RiskTier::R1,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::CreateRecord,
        ),
        "relation.append" => (
            "company.records.write",
            RiskTier::R1,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::CreateRelation,
        ),
        "approval.decide" => (
            "company.approve",
            RiskTier::R2,
            false,
            vec![ActorType::Human],
            ActionEffect::TransitionState,
        ),
        "approval.request" => (
            "company.records.write",
            RiskTier::R1,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::CreateRecord,
        ),
        "work_item.update" => (
            "company.records.write",
            RiskTier::R2,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::UpdateRecord,
        ),
        "work_item.transition" => (
            "company.work.execute",
            RiskTier::R2,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::TransitionState,
        ),
        "commitment.append" => (
            "finance.commitment.write",
            RiskTier::R3,
            true,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::CreateCommitment,
        ),
        "commitment.propose" => (
            "finance.commitment.write",
            RiskTier::R2,
            false,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::CreateCommitment,
        ),
        "payment.append" => (
            "finance.payment.write",
            RiskTier::R3,
            true,
            vec![ActorType::Human, ActorType::Agent],
            ActionEffect::SettlePayment,
        ),
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported declared command: {other}"
            )))
        }
    })
}

fn policies_for_definition(
    definition: &CustomPageDefinition,
) -> Result<Vec<ActionPolicyDefinition>, ApiError> {
    definition
        .action_command_refs
        .iter()
        .map(|command_name| {
            let (permission, risk_tier, requires_human_approval, actor_kinds, effect) =
                server_action_shape(command_name)?;
            let id = format!("{}:{command_name}", definition.id);
            if !definition.policy_refs.contains(&id) {
                return Err(ApiError::validation(format!(
                    "CustomPageDefinition.policy_refs must contain server policy id {id}"
                )));
            }
            Ok(ActionPolicyDefinition {
                id,
                module_ref: definition.module_id.clone(),
                definition_ref: definition.id.clone(),
                command_name: command_name.clone(),
                required_permission: permission.to_string(),
                risk_tier,
                requires_human_approval,
                allowed_actor_kinds: actor_kinds,
                allowed_effects: vec![effect],
            })
        })
        .collect()
}

fn validate_definition_scope(
    store: &HarnessStore,
    definition: &CustomPageDefinition,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    if let Some(module_id) = record.get("module_id").and_then(Value::as_str) {
        if module_id != definition.module_id {
            return Err(ApiError::forbidden(format!(
                "target module {module_id} is outside declaration module {}",
                definition.module_id
            )));
        }
    }
    if command.command_name == "commitment.append"
        && (command.subject_ref.kind != EntityKind::FinancialRecord
            || value_id(record) != Some(command.subject_ref.id.as_str()))
    {
        return Err(ApiError::forbidden(
            "commitment.append subject must be the Commitment being transitioned",
        ));
    }
    if command.command_name == "payment.append" {
        let payment_id = value_id(record)
            .ok_or_else(|| ApiError::validation("payment.append record requires id"))?;
        let payment_exists = store
            .latest_payments()?
            .iter()
            .any(|payment| payment.id == payment_id);
        let create_subject_is_commitment = record
            .get("related_commitment_refs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|value| value.as_str() == Some(command.subject_ref.id.as_str()));
        let valid_subject = command.subject_ref.kind == EntityKind::FinancialRecord
            && if payment_exists {
                command.subject_ref.id == payment_id
            } else {
                create_subject_is_commitment
            };
        if !valid_subject {
            return Err(ApiError::forbidden(
                "payment.append subject must be the existing Payment, or a related Commitment when creating it",
            ));
        }
    }
    let in_scope = match command.command_name.as_str() {
        "typed_record.append" => {
            let module_matches = record
                .get("module_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == definition.module_id);
            let updates_scoped_record = value_id(record).is_some_and(|id| {
                command.subject_ref.kind == EntityKind::TypedRecord && command.subject_ref.id == id
            });
            let creates_from_scoped_document = record
                .get("source_document_ref")
                .and_then(Value::as_str)
                .is_some_and(|id| {
                    command.subject_ref.kind == EntityKind::Document
                        && command.subject_ref.id == id
                        && document_in_module(store, definition, id)
                });
            module_matches && (updates_scoped_record || creates_from_scoped_document)
        }
        "view.append" => record
            .get("module_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == definition.module_id),
        "relation.append" => ["from_ref", "to_ref"].iter().all(|field| {
            record
                .get(*field)
                .cloned()
                .and_then(|value| serde_json::from_value::<harness_core::EntityRef>(value).ok())
                .is_some_and(|reference| entity_in_module(store, definition, &reference, 0))
        }),
        "work_item.append" | "work_item.update" | "work_item.transition" => {
            serde_json::from_value::<WorkItem>(record.clone())
                .ok()
                .is_some_and(|item| {
                    let subject_matches = if command.command_name == "work_item.append" {
                        command.subject_ref.kind == EntityKind::Document
                            && command.subject_ref.id == item.source_document_ref
                    } else {
                        command.subject_ref.kind == EntityKind::WorkItem
                            && command.subject_ref.id == item.id
                    };
                    subject_matches
                        && document_in_module(store, definition, &item.source_document_ref)
                        && item
                            .result_document_ref
                            .as_deref()
                            .is_none_or(|id| document_in_module(store, definition, id))
                        && item.result_record_refs.iter().all(|id| {
                            entity_in_module(
                                store,
                                definition,
                                &harness_core::EntityRef {
                                    kind: EntityKind::TypedRecord,
                                    id: id.clone(),
                                },
                                0,
                            )
                        })
                })
        }
        "assignment.append" => record
            .get("work_item_id")
            .and_then(Value::as_str)
            .is_some_and(|id| {
                command.subject_ref.kind == EntityKind::WorkItem
                    && command.subject_ref.id == id
                    && work_item_in_module(store, definition, id)
            }),
        "approval.request" | "approval.decide" => {
            entity_in_module(store, definition, &command.subject_ref, 0)
        }
        "commitment.propose" => {
            command.subject_ref.kind == EntityKind::WorkItem
                && work_item_in_module(store, definition, &command.subject_ref.id)
                && record
                    .get("source_document_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| document_in_module(store, definition, id))
        }
        "commitment.append" | "payment.append" => record
            .get("source_document_id")
            .and_then(Value::as_str)
            .is_some_and(|id| document_in_module(store, definition, id)),
        // Global organization and permission changes need a dedicated scope
        // model; a business-module page declaration cannot authorize them.
        "actor.append"
        | "org_unit.append"
        | "membership.append"
        | "business_module.append"
        | "custom_page_definition.append"
        | "custom_page_package.append" => false,
        _ => false,
    };
    if !in_scope {
        return Err(ApiError::forbidden(format!(
            "Action target is outside declaration module {}",
            definition.module_id
        )));
    }
    Ok(())
}

fn document_in_module(
    store: &HarnessStore,
    definition: &CustomPageDefinition,
    document_id: &str,
) -> bool {
    let Ok(modules) = store.latest_business_modules() else {
        return false;
    };
    let Some(module) = modules
        .iter()
        .find(|module| module.id == definition.module_id)
    else {
        return false;
    };
    let Ok(documents) = store.latest_documents() else {
        return false;
    };
    let mut current = Some(document_id);
    for _ in 0..64 {
        let Some(id) = current else { return false };
        if id == module.root_document_ref {
            return true;
        }
        current = documents
            .iter()
            .find(|document| document.id == id)
            .and_then(|document| document.parent_document_id.as_deref());
    }
    false
}

fn work_item_in_module(
    store: &HarnessStore,
    definition: &CustomPageDefinition,
    work_item_id: &str,
) -> bool {
    store
        .latest_work_items()
        .ok()
        .and_then(|items| items.into_iter().find(|item| item.id == work_item_id))
        .is_some_and(|item| {
            item.business_module_ref.as_deref() == Some(definition.module_id.as_str())
                || document_in_module(store, definition, &item.source_document_ref)
        })
}

fn entity_in_module(
    store: &HarnessStore,
    definition: &CustomPageDefinition,
    reference: &harness_core::EntityRef,
    depth: usize,
) -> bool {
    if depth > 8 {
        return false;
    }
    match reference.kind {
        EntityKind::Document => document_in_module(store, definition, &reference.id),
        EntityKind::TypedRecord => store
            .latest_typed_records()
            .ok()
            .and_then(|records| records.into_iter().find(|record| record.id == reference.id))
            .is_some_and(|record| record.module_id == definition.module_id),
        EntityKind::BusinessModule => reference.id == definition.module_id,
        EntityKind::Milestone => store
            .latest_milestones()
            .ok()
            .and_then(|milestones| milestones.into_iter().find(|item| item.id == reference.id))
            .is_some_and(|milestone| {
                milestone.business_module_ref.as_deref() == Some(definition.module_id.as_str())
                    || milestone
                        .source_document_ref
                        .as_deref()
                        .is_some_and(|document_id| {
                            document_in_module(store, definition, document_id)
                        })
            }),
        EntityKind::WorkItem => work_item_in_module(store, definition, &reference.id),
        EntityKind::Approval => store
            .latest_approvals()
            .ok()
            .and_then(|approvals| approvals.into_iter().find(|item| item.id == reference.id))
            .is_some_and(|approval| {
                entity_in_module(store, definition, &approval.subject_ref, depth + 1)
            }),
        EntityKind::FinancialRecord => {
            let commitment = store
                .latest_commitments()
                .ok()
                .and_then(|records| records.into_iter().find(|item| item.id == reference.id))
                .is_some_and(|item| {
                    document_in_module(store, definition, &item.source_document_id)
                });
            commitment
                || store
                    .latest_payments()
                    .ok()
                    .and_then(|records| records.into_iter().find(|item| item.id == reference.id))
                    .is_some_and(|item| {
                        document_in_module(store, definition, &item.source_document_id)
                    })
        }
        EntityKind::Actor | EntityKind::Evidence | EntityKind::Execution => false,
    }
}

fn validate_work_item_create(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let item: WorkItem = parse(record)?;
    if command.subject_ref.kind != EntityKind::Document
        || command.subject_ref.id != item.source_document_ref
    {
        return Err(ApiError::forbidden(
            "work_item.append subject must be its source Document",
        ));
    }
    if store
        .latest_work_items()?
        .iter()
        .any(|row| row.id == item.id)
    {
        return Err(ApiError::conflict(format!(
            "WorkItem {} already exists; use work_item.transition",
            item.id
        )));
    }
    if matches!(
        item.status,
        WorkItemStatus::InProgress | WorkItemStatus::InReview | WorkItemStatus::Completed
    ) {
        return Err(ApiError::validation(
            "new WorkItem cannot start as in_progress, in_review, or completed",
        ));
    }
    Ok(())
}

/// Responsibility fields decide who may execute, review, or close a WorkItem.
/// Rewriting one is an authority change, not an ordinary edit, so each is named
/// explicitly rather than inferred from a diff of the whole record.
fn changed_responsibility_fields(previous: &WorkItem, target: &WorkItem) -> Vec<&'static str> {
    let mut changed = Vec::new();
    if previous.accountable_owner != target.accountable_owner {
        changed.push("accountable_owner");
    }
    if previous.assignees != target.assignees {
        changed.push("assignees");
    }
    if previous.contributors != target.contributors {
        changed.push("contributors");
    }
    if previous.reviewer != target.reviewer {
        changed.push("reviewer");
    }
    if previous.approver != target.approver {
        changed.push("approver");
    }
    changed
}

/// Executor standing, using the same actor set as the executor half of
/// `validate_work_item_transition`: the Actor that may drive the WorkItem
/// forward.
fn is_work_item_executor(actor: &ActorRef, item: &WorkItem) -> bool {
    *actor == item.accountable_owner || item.assignees.contains(actor)
}

/// Closer standing, using the same actor set as the closer half of
/// `validate_work_item_transition`: the Actor that may sign the WorkItem off
/// as completed.
fn is_work_item_closer(actor: &ActorRef, item: &WorkItem) -> bool {
    *actor == item.accountable_owner || item.reviewer.as_ref() == Some(actor)
}

/// Any responsibility standing at all. Contributors are deliberately excluded:
/// a contributor can neither execute nor close, so counting one as a
/// controller would reopen the laundering path this gate closes.
fn controls_work_item(actor: &ActorRef, item: &WorkItem) -> bool {
    is_work_item_executor(actor, item) || is_work_item_closer(actor, item)
}

/// The role classes the requesting Actor would gain for itself.
///
/// `validate_work_item_transition` splits responsibility into executor
/// (accountable_owner or assignee) and closer (accountable_owner or reviewer).
/// Gaining a class you did not already hold is self-elevation, and that
/// includes an Actor which already holds the *other* class: letting an
/// assignee take the reviewer seat would let one Actor both do and sign off
/// its own work, which is the separation of duties `work_item.transition`
/// exists to enforce.
///
/// `approver` is intentionally absent. It is gated as responsibility, but it
/// is not a transition role: approvals authorize on
/// `Approval.required_approver_refs`, never on `WorkItem.approver`, so naming
/// yourself approver grants no executor or closer standing.
fn self_elevated_roles(
    actor: &ActorRef,
    previous: &WorkItem,
    target: &WorkItem,
) -> Vec<&'static str> {
    let mut gained = Vec::new();
    if is_work_item_executor(actor, target) && !is_work_item_executor(actor, previous) {
        gained.push("executor");
    }
    if is_work_item_closer(actor, target) && !is_work_item_closer(actor, previous) {
        gained.push("closer");
    }
    gained
}

/// The responsibility fields through which the requesting Actor wrote itself
/// in, used to make the denial name the exact seats it tried to take.
fn self_granted_fields(
    actor: &ActorRef,
    previous: &WorkItem,
    target: &WorkItem,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if target.accountable_owner == *actor && previous.accountable_owner != *actor {
        fields.push("accountable_owner");
    }
    if target.assignees.contains(actor) && !previous.assignees.contains(actor) {
        fields.push("assignees");
    }
    if target.reviewer.as_ref() == Some(actor) && previous.reviewer.as_ref() != Some(actor) {
        fields.push("reviewer");
    }
    fields
}

/// Explicit, policy-named update authority. The Actor ledger must name this
/// exact ActionPolicyDefinition id, or the company admin permission. The
/// blanket `company.records.write` that every records author holds never
/// satisfies this, which is what keeps the exemption declared in policy
/// instead of implied by having any write permission at all.
fn holds_named_update_authority(
    store: &HarnessStore,
    actor_ref: &ActorRef,
    policy_ref: &str,
) -> Result<bool, ApiError> {
    let actor = store
        .latest_actor(actor_ref)?
        .ok_or_else(|| ApiError::not_found(format!("actor:{}", actor_ref.actor_id)))?;
    let names = |refs: &[String]| {
        refs.iter()
            .any(|value| value == policy_ref || value == COMPANY_OS_ADMIN_PERMISSION)
    };
    Ok(match actor {
        CompanyActor::Human(actor) => {
            names(&actor.permission_policy_refs) || names(&actor.authority_policy_refs)
        }
        CompanyActor::Agent(actor) => names(&actor.permission_policy_refs),
        CompanyActor::External(actor) => names(&actor.restricted_permission_refs),
        CompanyActor::Service(actor) => names(&actor.permission_policy_refs),
    })
}

fn responsibility_snapshot(item: &WorkItem) -> Value {
    json!({
        "accountable_owner": item.accountable_owner,
        "assignees": item.assignees,
        "contributors": item.contributors,
        "reviewer": item.reviewer,
        "approver": item.approver,
    })
}

/// Gate every responsibility rewrite on explicit authority, and forbid
/// self-elevation outright.
///
/// Two paths stay open: an Actor that already owns, is assigned to, or reviews
/// the WorkItem may re-route it, and an Actor holding explicit policy-named
/// update authority may perform governed routing.
///
/// Neither path may hand the requesting Actor an executor or closer role class
/// it did not already hold. That check runs first and applies to controllers
/// too, so `work_item.update` can never be used to pass a
/// `work_item.transition` ownership check the Actor would otherwise fail — an
/// assignee cannot take the reviewer seat and then close its own work, and a
/// reviewer cannot take the assignee seat and then execute it.
///
/// Scope boundary, stated rather than implied: the policy-named path is
/// per-definition/module, not per-record. `validate_definition_scope` only
/// requires the WorkItem's `source_document_ref` to sit under the definition's
/// module, so holding `page-X:work_item.update` is responsibility-rewrite
/// authority over *every* WorkItem in module X, and `company_os.admin` is a
/// company-wide exemption. True per-record authority needs the
/// `ScopedPermissionGrant` broker (ADR 0047), which is not implemented.
fn require_work_item_responsibility_authority(
    store: &HarnessStore,
    command: &ActionCommand,
    previous: &WorkItem,
    target: &WorkItem,
) -> Result<(), ApiError> {
    let changed = changed_responsibility_fields(previous, target);
    if changed.is_empty() {
        return Ok(());
    }
    let requester = &command.requested_by;
    let elevated = self_elevated_roles(requester, previous, target);

    // Checked before both authority paths, and applied even to an Actor that
    // already controls the WorkItem, so routing work to someone else stays
    // separable from taking the work.
    if !elevated.is_empty() {
        let seats = self_granted_fields(requester, previous, target);
        return Err(deny_work_item_update(
            store,
            command,
            previous,
            target,
            &changed,
            "authority_laundering",
            format!(
                "Actor {} cannot use work_item.update to grant itself {} standing on \
                 WorkItem {} by writing itself into {}: work_item.update must never \
                 create the ownership that work_item.transition checks, and an Actor \
                 that holds one of the executor/closer roles may not take the other",
                requester.actor_id,
                elevated.join(" and "),
                previous.id,
                seats.join(", ")
            ),
        ));
    }
    if controls_work_item(requester, previous) {
        return Ok(());
    }
    if holds_named_update_authority(store, requester, &command.policy_ref)? {
        return Ok(());
    }
    Err(deny_work_item_update(
        store,
        command,
        previous,
        target,
        &changed,
        "missing_update_authority",
        format!(
            "Actor {} does not own this WorkItem responsibility update: changing {} on \
             WorkItem {} requires being its accountable_owner, an assignee, or its \
             reviewer, or holding explicit update authority for policy {}",
            requester.actor_id,
            changed.join(", "),
            previous.id,
            command.policy_ref
        ),
    ))
}

/// Record a governed denial durably, then return it.
///
/// A refused responsibility rewrite must leave the same reconstructable trail
/// as an executed one, so the attempt is claimed and driven to `Rejected` with
/// a terminal AuditEvent naming the Actor, the WorkItem, the refused fields,
/// and the previous and requested role refs.
fn deny_work_item_update(
    store: &HarnessStore,
    command: &ActionCommand,
    previous: &WorkItem,
    target: &WorkItem,
    changed: &[&'static str],
    denial_kind: &str,
    reason: String,
) -> ApiError {
    let detail = json!({
        "command_name": command.command_name,
        "policy_ref": command.policy_ref,
        "target_id": previous.id,
        "denial_kind": denial_kind,
        "denied_reason": reason,
        "requested_by": command.requested_by,
        "required_permission": command.required_permission,
        "rejected_fields": changed,
        "previous_responsibility": responsibility_snapshot(previous),
        "requested_responsibility": responsibility_snapshot(target),
    });
    match record_action_denial(store, command, &detail) {
        Ok(()) => ApiError::forbidden(reason),
        Err(error) => error,
    }
}

/// Drive one refused ActionCommand to a durable `Rejected` terminal state with
/// its denial AuditEvent. Replay of an already-denied command id is a no-op so
/// the terminal row stays immutable.
///
/// The denial id is reserved in the same claim that records the attempt, and
/// the terminal row plus its evidence are written through
/// `reject_action_command_atomic`, the same all-or-nothing invariant the
/// executed/failed path uses. A refused command therefore cannot become
/// terminal without its denial evidence.
fn record_action_denial(
    store: &HarnessStore,
    command: &ActionCommand,
    detail: &Value,
) -> Result<(), ApiError> {
    let denial_audit_id = format!("{}:rejected", command.id);
    let mut denied = command.clone();
    denied.status = ActionCommandStatus::Requested;
    match store.claim_action_command_with_audit_reservations(
        &denied,
        std::slice::from_ref(&denial_audit_id),
    )? {
        ActionCommandClaimResult::Claimed(_) => {}
        ActionCommandClaimResult::Replay(existing) => {
            if existing.status != ActionCommandStatus::Requested {
                return Ok(());
            }
        }
        ActionCommandClaimResult::Conflict(existing) => {
            return Err(ApiError::conflict(format!(
                "ActionCommand id {} already belongs to {}",
                existing.id, existing.command_name
            )))
        }
    }
    let occurred_at = now_string();
    denied.audit_event_refs.push(denial_audit_id.clone());
    denied.status = ActionCommandStatus::Rejected;
    denied.completed_at = Some(occurred_at.clone());
    let event = AuditEvent {
        id: denial_audit_id,
        action_command_id: command.id.clone(),
        event_kind: AuditEventKind::Failed,
        actor_ref: command.requested_by.clone(),
        subject_ref: command.subject_ref.clone(),
        detail: detail.clone(),
        evidence_refs: Vec::new(),
        occurred_at,
    };
    store.reject_action_command_atomic(&denied, std::slice::from_ref(&event))?;
    Ok(())
}

fn validate_work_item_update(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let target: WorkItem = parse(record)?;
    if command.subject_ref.kind != EntityKind::WorkItem || command.subject_ref.id != target.id {
        return Err(ApiError::forbidden(
            "work_item.update subject must be the WorkItem being updated",
        ));
    }
    let previous = store
        .latest_work_items()?
        .into_iter()
        .find(|candidate| candidate.id == target.id)
        .ok_or_else(|| ApiError::not_found(format!("WorkItem:{}", target.id)))?;
    // Authority is decided before shape. A refused responsibility rewrite must
    // be denied and recorded even when the same payload is malformed in some
    // other way, so the denial evidence is never lost to an earlier 409.
    require_work_item_responsibility_authority(store, command, &previous, &target)?;
    if previous.status != target.status {
        return Err(ApiError::conflict(
            "work_item.update cannot change lifecycle status; use work_item.transition",
        ));
    }
    // `submitted_by` and `requested_by` are source accountability: they record
    // who raised the work and on whose behalf. `work_item.transition` already
    // treats both as immutable, so `work_item.update` must not become the
    // forgery path for them.
    if previous.created_at != target.created_at
        || previous.completed_at != target.completed_at
        || previous.submitted_by != target.submitted_by
        || previous.requested_by != target.requested_by
        || previous.result_document_ref != target.result_document_ref
        || previous.result_record_refs != target.result_record_refs
        || previous.approval_refs != target.approval_refs
        || previous.evidence_refs != target.evidence_refs
        || previous.artifact_refs != target.artifact_refs
        || previous.outcome_summary != target.outcome_summary
        || previous.execution_refs != target.execution_refs
    {
        return Err(ApiError::conflict(
            "work_item.update cannot change request provenance, lifecycle result, approval, evidence, artifact, or execution provenance",
        ));
    }
    Ok(())
}

fn validate_assignment_create(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let assignment: Assignment = parse(record)?;
    if command.subject_ref.kind != EntityKind::WorkItem
        || command.subject_ref.id != assignment.work_item_id
    {
        return Err(ApiError::forbidden(
            "assignment.append subject must be its WorkItem",
        ));
    }
    if store
        .latest_assignments()?
        .iter()
        .any(|row| row.id == assignment.id)
    {
        return Err(ApiError::conflict(format!(
            "Assignment {} already exists",
            assignment.id
        )));
    }
    Ok(())
}

fn validate_commitment_proposal(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let commitment: Commitment = parse(record)?;
    if commitment.status != CommitmentStatus::Proposed {
        return Err(ApiError::validation(
            "commitment.propose must create proposed status",
        ));
    }
    if !commitment.approval_refs.is_empty() {
        return Err(ApiError::validation(
            "a proposed Commitment cannot claim an Approval before approval.request",
        ));
    }
    if store
        .latest_commitments()?
        .iter()
        .any(|row| row.id == commitment.id)
    {
        return Err(ApiError::conflict(format!(
            "Commitment {} already exists; use commitment.append",
            commitment.id
        )));
    }
    let linked_to_work = store.latest_relations()?.iter().any(|relation| {
        commitment.relation_ids.contains(&relation.id)
            && ((relation.from_ref.kind == EntityKind::WorkItem
                && relation.from_ref.id == command.subject_ref.id)
                || (relation.to_ref.kind == EntityKind::WorkItem
                    && relation.to_ref.id == command.subject_ref.id))
    });
    if !linked_to_work {
        return Err(ApiError::validation(
            "commitment.propose requires a Relation linking the Commitment context to its WorkItem",
        ));
    }
    Ok(())
}

fn validate_approval_request(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let approval: Approval = parse(record)?;
    if approval.status != ApprovalStatus::Requested {
        return Err(ApiError::validation(
            "approval.request must create requested status",
        ));
    }
    if approval.subject_ref != command.subject_ref {
        return Err(ApiError::forbidden(
            "approval.request subject must match the Approval subject",
        ));
    }
    if approval.requested_by != command.requested_by {
        return Err(ApiError::forbidden(
            "approval.request requested_by must be the Action requester",
        ));
    }
    if store
        .latest_approvals()?
        .iter()
        .any(|row| row.id == approval.id)
    {
        return Err(ApiError::conflict(format!(
            "Approval {} already exists",
            approval.id
        )));
    }
    Ok(())
}

fn validate_typed_record_append(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let target: TypedRecord = parse(record)?;
    let previous = store
        .latest_typed_records()?
        .into_iter()
        .find(|row| row.id == target.id);
    if let Some(previous) = previous {
        if previous.module_id != target.module_id
            || previous.record_type != target.record_type
            || previous.source_document_ref != target.source_document_ref
            || previous.created_by != target.created_by
            || previous.created_at != target.created_at
        {
            return Err(ApiError::conflict(
                "typed_record.append update cannot change record identity or source",
            ));
        }
        if command.subject_ref.kind != EntityKind::TypedRecord
            || command.subject_ref.id != target.id
        {
            return Err(ApiError::forbidden(
                "typed_record.append update subject must be the existing TypedRecord",
            ));
        }
    } else if target.created_by != command.requested_by {
        return Err(ApiError::forbidden(
            "TypedRecord creator must be the Action requester",
        ));
    }
    if target.updated_by != command.requested_by {
        return Err(ApiError::forbidden(
            "TypedRecord.updated_by must be the Action requester",
        ));
    }
    Ok(())
}

fn validate_relation_append(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let target: Relation = parse(record)?;
    let previous = store
        .latest_relations()?
        .into_iter()
        .find(|row| row.id == target.id);
    if let Some(previous) = previous {
        if previous.from_ref != target.from_ref
            || previous.to_ref != target.to_ref
            || previous.relation_type != target.relation_type
            || previous.provenance_ref != target.provenance_ref
            || previous.created_by != target.created_by
            || previous.created_at != target.created_at
        {
            return Err(ApiError::conflict(
                "relation.append update cannot change relation identity, endpoints, type, provenance, or creation metadata",
            ));
        }
        if command.subject_ref != target.from_ref && command.subject_ref != target.to_ref {
            return Err(ApiError::forbidden(
                "relation.append update subject must be one relation endpoint",
            ));
        }
    } else if target.lifecycle_status.as_deref() == Some("archived") {
        return Err(ApiError::conflict(
            "relation.append cannot create an already archived relation",
        ));
    }
    Ok(())
}

fn validate_approval_decision(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let approval: Approval = parse(record)?;
    if command.subject_ref.kind != EntityKind::Approval || command.subject_ref.id != approval.id {
        return Err(ApiError::forbidden(
            "approval.decide subject must be the Approval being decided",
        ));
    }
    if !matches!(
        approval.status,
        ApprovalStatus::Approved | ApprovalStatus::Rejected
    ) {
        return Err(ApiError::validation(
            "approval.decide must transition to approved or rejected",
        ));
    }
    let previous = store
        .latest_approvals()?
        .into_iter()
        .find(|candidate| candidate.id == approval.id)
        .ok_or_else(|| ApiError::not_found(format!("Approval:{}", approval.id)))?;
    if previous.status != ApprovalStatus::Requested {
        return Err(ApiError::conflict(
            "only a requested Approval may be decided",
        ));
    }
    if previous.subject_ref != approval.subject_ref
        || previous.policy_ref != approval.policy_ref
        || previous.required_approver_refs != approval.required_approver_refs
    {
        return Err(ApiError::conflict(
            "approval decision cannot change subject, policy, or required approvers",
        ));
    }
    if approval
        .expires_at
        .as_deref()
        .is_some_and(timestamp_is_past)
    {
        return Err(ApiError::forbidden("expired Approval cannot be decided"));
    }
    if command.requested_by.actor_type != ActorType::Human
        || !approval.decided_by.contains(&command.requested_by)
        || !approval_has_valid_human_decision(&approval)
    {
        return Err(ApiError::forbidden(
            "approval decision must be made by the named required Human approver",
        ));
    }
    require_approval_authority(store, &command.requested_by, &approval.policy_ref)
}

fn validate_work_item_transition(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<(), ApiError> {
    let target: WorkItem = parse(record)?;
    if command.subject_ref.kind != EntityKind::WorkItem || command.subject_ref.id != target.id {
        return Err(ApiError::forbidden(
            "work_item.transition subject must be the WorkItem being transitioned",
        ));
    }
    let previous = store
        .latest_work_items()?
        .into_iter()
        .find(|candidate| candidate.id == target.id)
        .ok_or_else(|| ApiError::not_found(format!("WorkItem:{}", target.id)))?;
    if previous.status == target.status {
        return Err(ApiError::conflict(
            "work_item.transition must change the WorkItem status",
        ));
    }
    let allowed = matches!(
        (previous.status, target.status),
        (
            WorkItemStatus::Submitted
                | WorkItemStatus::Triaged
                | WorkItemStatus::Accepted
                | WorkItemStatus::Blocked,
            WorkItemStatus::InProgress
        ) | (
            WorkItemStatus::InProgress,
            WorkItemStatus::Blocked | WorkItemStatus::InReview | WorkItemStatus::WaitingForApproval
        ) | (
            WorkItemStatus::InReview,
            WorkItemStatus::InProgress
                | WorkItemStatus::WaitingForApproval
                | WorkItemStatus::Completed
        ) | (
            WorkItemStatus::WaitingForApproval,
            WorkItemStatus::InProgress | WorkItemStatus::Blocked | WorkItemStatus::Completed
        )
    );
    if !allowed {
        return Err(ApiError::conflict(format!(
            "unsupported WorkItem transition {:?} -> {:?}",
            previous.status, target.status
        )));
    }
    let immutable_changed = previous.title != target.title
        || previous.objective != target.objective
        || previous.description != target.description
        || previous.acceptance_criteria != target.acceptance_criteria
        || previous.context_refs != target.context_refs
        || previous.source_document_ref != target.source_document_ref
        || previous.source_record_refs != target.source_record_refs
        || previous.milestone_ref != target.milestone_ref
        || previous.work_type != target.work_type
        || previous.business_module_ref != target.business_module_ref
        || previous.submitted_by != target.submitted_by
        || previous.requested_by != target.requested_by
        || previous.accountable_owner != target.accountable_owner
        || previous.assignees != target.assignees
        || previous.contributors != target.contributors
        || previous.reviewer != target.reviewer
        || previous.approver != target.approver
        || previous.execution_mode != target.execution_mode
        || previous.due_at != target.due_at
        || previous.priority != target.priority
        || previous.risk_level != target.risk_level
        || previous.created_at != target.created_at;
    if immutable_changed {
        return Err(ApiError::conflict(
            "work_item.transition cannot change business context or responsibility",
        ));
    }
    let result_document_preserved = previous.result_document_ref.is_none()
        || previous.result_document_ref == target.result_document_ref;
    let preserves = |old: &[String], next: &[String]| old.iter().all(|item| next.contains(item));
    if !result_document_preserved
        || !preserves(&previous.result_record_refs, &target.result_record_refs)
        || !preserves(&previous.approval_refs, &target.approval_refs)
        || !preserves(&previous.evidence_refs, &target.evidence_refs)
        || !preserves(&previous.artifact_refs, &target.artifact_refs)
        || !previous
            .deliverable_refs
            .iter()
            .all(|item| target.deliverable_refs.contains(item))
        || !target.execution_refs.starts_with(&previous.execution_refs)
    {
        return Err(ApiError::conflict(
            "work_item.transition cannot remove or replace result, evidence, artifact, or execution provenance",
        ));
    }
    let executor = command.requested_by == previous.accountable_owner
        || previous.assignees.contains(&command.requested_by);
    let closer = command.requested_by == previous.accountable_owner
        || previous.reviewer.as_ref() == Some(&command.requested_by);
    if (target.status == WorkItemStatus::Completed && !closer)
        || (target.status != WorkItemStatus::Completed && !executor)
    {
        return Err(ApiError::forbidden(
            "requesting Actor does not own this WorkItem transition",
        ));
    }
    if target.status == WorkItemStatus::InReview {
        let has_result =
            target.result_document_ref.is_some() || !target.result_record_refs.is_empty();
        let has_evidence = !target.evidence_refs.is_empty() || !target.artifact_refs.is_empty();
        let has_summary = target
            .outcome_summary
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        if !has_result || !has_evidence || !has_summary {
            return Err(ApiError::validation(
                "entering in_review requires a durable result destination, evidence or artifacts, and an outcome summary",
            ));
        }
    }
    if target.status == WorkItemStatus::Completed {
        let approvals = store.latest_approvals()?;
        if target.approval_refs.iter().any(|id| {
            !approvals
                .iter()
                .any(|approval| approval.id == *id && approval.status == ApprovalStatus::Approved)
        }) {
            return Err(ApiError::forbidden(
                "completed WorkItem requires every linked Approval to be approved",
            ));
        }
    } else if target.completed_at.is_some() {
        return Err(ApiError::validation(
            "only a completed WorkItem may set completed_at",
        ));
    }
    Ok(())
}

fn dispatch_declared_record(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
    allow_existing_exact: bool,
) -> Result<Value, ApiError> {
    let resource = match command.command_name.as_str() {
        "typed_record.append" => "typed-records",
        "relation.append" => "relations",
        "view.append" => "views",
        "business_module.append" => "business-modules",
        "actor.append" => "actors",
        "org_unit.append" => "org-units",
        "membership.append" => "memberships",
        "work_item.append" | "work_item.update" | "work_item.transition" => "work-items",
        "assignment.append" => "assignments",
        "approval.request" | "approval.decide" => "approvals",
        "commitment.propose" | "commitment.append" => "commitments",
        "payment.append" => "payments",
        "custom_page_definition.append" => "custom-page-definitions",
        "custom_page_package.append" => "custom-page-packages",
        other => {
            return Err(ApiError::bad_request(format!(
                "unsupported declared command: {other}"
            )))
        }
    };
    if allow_existing_exact
        && resource_history(store, resource)?
            .into_iter()
            .any(|existing| existing == *record)
    {
        return Ok(record.clone());
    }
    if resource == "commitments" || resource == "payments" {
        let audit_ids = record
            .get("audit_event_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ApiError::validation("financial action record requires audit_event_ids")
            })?;
        let linked = command.audit_event_refs.iter().any(|event| {
            audit_ids
                .iter()
                .any(|value| value.as_str() == Some(event.as_str()))
        });
        if !linked {
            return Err(ApiError::validation(
                "financial record audit_event_ids must include an ActionCommand audit_event_ref",
            ));
        }
    }
    append_resource(store, resource, record, AppendMode::GovernedAction)
}

fn resource_history(store: &HarnessStore, resource: &str) -> Result<Vec<Value>, ApiError> {
    match resource {
        "documents" => to_values(store.documents()?),
        "blocks" => to_values(store.blocks()?),
        "typed-records" => to_values(store.typed_records()?),
        "relations" => to_values(store.relations()?),
        "views" => to_values(store.views()?),
        "business-modules" => to_values(store.business_modules()?),
        "actors" => store
            .actors()?
            .into_iter()
            .map(|value| {
                serde_json::to_value(value).map_err(|error| ApiError::internal(error.to_string()))
            })
            .collect(),
        "org-units" => to_values(store.org_units()?),
        "memberships" => to_values(store.organization_memberships()?),
        "milestones" => to_values(store.milestones()?),
        "work-items" => to_values(store.work_items()?),
        "assignments" => to_values(store.assignments()?),
        "approvals" => to_values(store.approvals()?),
        "commitments" => to_values(store.commitments()?),
        "payments" => to_values(store.payments()?),
        "custom-page-definitions" => to_values(store.custom_page_definitions()?),
        "custom-page-packages" => to_values(store.custom_page_packages()?),
        _ => Err(ApiError::not_found(format!(
            "unknown Company OS resource: {resource}"
        ))),
    }
}

fn require_permission(
    store: &HarnessStore,
    actor_ref: &ActorRef,
    required_permission: &str,
) -> Result<(), ApiError> {
    let actor = store
        .latest_actor(actor_ref)?
        .ok_or_else(|| ApiError::not_found(format!("actor:{}", actor_ref.actor_id)))?;
    let permission = required_permission.to_string();
    let permitted = match actor {
        CompanyActor::Human(actor) => {
            actor.permission_policy_refs.contains(&permission)
                || actor.authority_policy_refs.contains(&permission)
        }
        CompanyActor::Agent(actor) => actor.permission_policy_refs.contains(&permission),
        CompanyActor::External(actor) => actor.restricted_permission_refs.contains(&permission),
        CompanyActor::Service(actor) => actor.permission_policy_refs.contains(&permission),
    };
    if !permitted {
        return Err(ApiError::forbidden(format!(
            "actor {} lacks permission {}",
            actor_ref.actor_id, required_permission
        )));
    }
    Ok(())
}

fn require_active_actor(store: &HarnessStore, actor_ref: &ActorRef) -> Result<(), ApiError> {
    let actor = store
        .latest_actor(actor_ref)?
        .ok_or_else(|| ApiError::not_found(format!("actor:{}", actor_ref.actor_id)))?;
    let active = match actor {
        CompanyActor::Human(actor) => actor.status == MemberStatus::Active,
        CompanyActor::Agent(actor) => actor.status == MemberStatus::Active,
        CompanyActor::External(actor) => {
            actor.status == MemberStatus::Active && !timestamp_is_past(&actor.access_expires_at)
        }
        CompanyActor::Service(actor) => actor.status == MemberStatus::Active,
    };
    if !active {
        return Err(ApiError::forbidden(format!(
            "actor {} is inactive or expired",
            actor_ref.actor_id
        )));
    }
    Ok(())
}

fn require_approval_authority(
    store: &HarnessStore,
    actor_ref: &ActorRef,
    policy_ref: &str,
) -> Result<(), ApiError> {
    require_active_actor(store, actor_ref)?;
    let actor = store
        .latest_actor(actor_ref)?
        .ok_or_else(|| ApiError::not_found(format!("actor:{}", actor_ref.actor_id)))?;
    let CompanyActor::Human(human) = actor else {
        return Err(ApiError::forbidden("approval authority must be Human"));
    };
    if !human.authority_policy_refs.iter().any(|value| {
        value == policy_ref || value == "company.approve" || value == COMPANY_OS_ADMIN_PERMISSION
    }) {
        return Err(ApiError::forbidden(format!(
            "Human {} lacks authority for policy {}",
            actor_ref.actor_id, policy_ref
        )));
    }
    Ok(())
}

fn require_human_approval(store: &HarnessStore, command: &ActionCommand) -> Result<(), ApiError> {
    let approvals = store.latest_approvals()?;
    for approval in approvals.iter().filter(|approval| {
        command.approval_refs.contains(&approval.id)
            && approval.status == ApprovalStatus::Approved
            && approval.subject_ref == command.subject_ref
            && approval.policy_ref == command.policy_ref
            && approval.action_summary.contains(&command.command_name)
            && !approval.evidence_refs.is_empty()
            && !approval
                .expires_at
                .as_deref()
                .is_some_and(timestamp_is_past)
            && approval_has_valid_human_decision(approval)
    }) {
        if approval
            .decided_by
            .iter()
            .any(|actor| require_approval_authority(store, actor, &approval.policy_ref).is_ok())
        {
            return Ok(());
        }
    }
    Err(ApiError::forbidden(
        "action requires an in-scope, unexpired, evidence-backed decision by the named Human authority",
    ))
}

fn commitment_enters_approval_queue(
    store: &HarnessStore,
    command: &ActionCommand,
    record: &Value,
) -> Result<bool, ApiError> {
    if command.command_name != "commitment.append" {
        return Ok(false);
    }
    let target: Commitment = parse(record)?;
    if target.status != CommitmentStatus::PendingApproval {
        return Ok(false);
    }
    Ok(store
        .latest_commitments()?
        .into_iter()
        .find(|item| item.id == target.id)
        .is_some_and(|previous| previous.status == CommitmentStatus::Proposed))
}

fn require_requested_human_approval(
    store: &HarnessStore,
    command: &ActionCommand,
) -> Result<(), ApiError> {
    for approval in store.latest_approvals()?.iter().filter(|approval| {
        command.approval_refs.contains(&approval.id)
            && matches!(
                approval.status,
                ApprovalStatus::Requested | ApprovalStatus::Approved
            )
            && approval.subject_ref == command.subject_ref
            && approval.policy_ref == command.policy_ref
            && approval.action_summary.contains(&command.command_name)
            && !approval.evidence_refs.is_empty()
            && !approval
                .expires_at
                .as_deref()
                .is_some_and(timestamp_is_past)
            && approval.required_actor_type == Some(ActorType::Human)
            && approval
                .required_approver_refs
                .iter()
                .any(|actor| actor.actor_type == ActorType::Human)
    }) {
        if approval
            .required_approver_refs
            .iter()
            .any(|actor| require_approval_authority(store, actor, &approval.policy_ref).is_ok())
        {
            return Ok(());
        }
    }
    Err(ApiError::forbidden(
        "entering pending_approval requires a matching requested Human Approval with evidence and named authority",
    ))
}

fn validate_payment_governance(store: &HarnessStore, payment: &Payment) -> Result<(), ApiError> {
    payment
        .validate()
        .map_err(|error| ApiError::validation(error.to_string()))?;
    if payment.related_commitment_refs.is_empty() {
        return Err(ApiError::validation(
            "Payment.related_commitment_refs must contain an existing Commitment",
        ));
    }
    if payment.evidence_refs.is_empty() {
        return Err(ApiError::validation(
            "Payment.evidence_refs must contain execution evidence",
        ));
    }
    if payment.approval_refs.is_empty() {
        return Err(ApiError::validation(
            "Payment.approval_refs must contain a Human approval",
        ));
    }
    let commitments = store.latest_commitments()?;
    for commitment_id in &payment.related_commitment_refs {
        let commitment = commitments
            .iter()
            .find(|commitment| commitment.id == *commitment_id)
            .ok_or_else(|| ApiError::not_found(format!("Commitment:{commitment_id}")))?;
        if !matches!(
            commitment.status,
            CommitmentStatus::Approved | CommitmentStatus::Fulfilled
        ) {
            return Err(ApiError::forbidden(format!(
                "Commitment {commitment_id} is not approved"
            )));
        }
        if commitment.amount != payment.amount
            || commitment.source_document_id != payment.source_document_id
            || commitment.accountable_owner != payment.accountable_owner
        {
            return Err(ApiError::conflict(
                "Payment amount, currency, source, and owner must match its Commitment",
            ));
        }
    }
    let approvals = store.latest_approvals()?;
    let valid_human_approval = payment.approval_refs.iter().any(|id| {
        approvals.iter().any(|approval| {
            let governs_payment_context = (approval.subject_ref.kind
                == EntityKind::FinancialRecord
                && (payment
                    .related_commitment_refs
                    .contains(&approval.subject_ref.id)
                    || approval.subject_ref.id == payment.id))
                || (approval.subject_ref.kind == EntityKind::Document
                    && approval.subject_ref.id == payment.source_document_id);
            approval.id == *id
                && approval.status == ApprovalStatus::Approved
                && approval.action_summary.contains("payment.append")
                && !approval.evidence_refs.is_empty()
                && !approval
                    .expires_at
                    .as_deref()
                    .is_some_and(timestamp_is_past)
                && approval_has_valid_human_decision(approval)
                && approval.decided_by.iter().any(|actor| {
                    require_approval_authority(store, actor, &approval.policy_ref).is_ok()
                })
                && governs_payment_context
        })
    });
    if !valid_human_approval {
        return Err(ApiError::forbidden(
            "Payment requires an approved, evidence-backed Human approval",
        ));
    }
    Ok(())
}

fn approval_has_valid_human_decision(approval: &Approval) -> bool {
    approval.decided_by.iter().any(|decider| {
        decider.actor_type == ActorType::Human
            && approval
                .required_approver_refs
                .iter()
                .any(|required| required == decider)
    })
}

fn timestamp_is_past(value: &str) -> bool {
    if let Some(raw) = value.strip_prefix("unix-ms:") {
        return raw
            .parse::<u128>()
            .ok()
            .is_none_or(|millis| millis < now_unix_millis());
    }
    rfc3339_epoch_seconds(value).is_none_or(|seconds| seconds < (now_unix_millis() / 1_000) as i64)
}

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn rfc3339_epoch_seconds(value: &str) -> Option<i64> {
    let (date, time_and_zone) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (time, offset_seconds) = if let Some(time) = time_and_zone.strip_suffix('Z') {
        (time, 0_i64)
    } else {
        let zone_index = time_and_zone
            .char_indices()
            .rfind(|(index, character)| *index > 0 && matches!(character, '+' | '-'))?
            .0;
        let (time, zone) = time_and_zone.split_at(zone_index);
        let sign = if zone.starts_with('+') { 1_i64 } else { -1_i64 };
        let (hours, minutes) = zone[1..].split_once(':')?;
        let hours = hours.parse::<i64>().ok()?;
        let minutes = minutes.parse::<i64>().ok()?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        (time, sign * (hours * 3_600 + minutes * 60))
    };
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second = time_parts.next()?.split('.').next()?.parse::<i64>().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    Some(days_since_epoch * 86_400 + hour * 3_600 + minute * 60 + second - offset_seconds)
}

fn now_string() -> String {
    format!("unix-ms:{}", now_unix_millis())
}

// ---------------------------------------------------------------------------
// AI-first Docs v2 endpoints (ADR 0054). Same contract as the CLI page
// commands: scoped reads with fragment honesty, whole-page revisions,
// expected-revision optimistic concurrency, and idempotent replay.
// ---------------------------------------------------------------------------

const DOCS_V2_PAGES: &str = "/v1/company-os/docs-v2/pages";

fn docs_v2_error(error: crate::CliError) -> ApiError {
    match error {
        crate::CliError::Usage(message) => {
            if message.starts_with("REVISION_CONFLICT")
                || message.starts_with("IDEMPOTENCY_CONFLICT")
            {
                ApiError::conflict(message)
            } else if message.starts_with("document not found")
                || message.starts_with("block not found")
                || message.starts_with("anchor block not found")
                || message.contains("not found for document")
            {
                ApiError::not_found(message)
            } else {
                ApiError::bad_request(message)
            }
        }
        crate::CliError::Store(store_error) => ApiError::from(store_error),
        other => ApiError::internal(other.to_string()),
    }
}

fn docs_v2_actor(body: &Value) -> Result<ActorRef, ApiError> {
    let actor = body
        .get("actor")
        .ok_or_else(|| ApiError::bad_request("actor is required"))?;
    let (actor_type, actor_id) = match actor {
        Value::String(raw) => {
            let (kind, id) = raw
                .split_once(':')
                .ok_or_else(|| ApiError::bad_request("actor string must be <kind>:<id>"))?;
            (kind.to_string(), id.to_string())
        }
        Value::Object(_) => {
            let kind = actor
                .get("actor_type")
                .and_then(Value::as_str)
                .unwrap_or("agent")
                .to_string();
            let id = actor
                .get("actor_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            (kind, id)
        }
        _ => {
            return Err(ApiError::bad_request(
                "actor must be an object or <kind>:<id> string",
            ))
        }
    };
    if actor_id.trim().is_empty() {
        return Err(ApiError::bad_request("actor id must be non-empty"));
    }
    let actor_type = match actor_type.as_str() {
        "human" => ActorType::Human,
        "agent" => ActorType::Agent,
        "external" => ActorType::External,
        "service" => ActorType::Service,
        other => {
            return Err(ApiError::bad_request(format!(
                "actor_type must be human|agent|external|service, got {other}"
            )))
        }
    };
    Ok(ActorRef {
        actor_type,
        actor_id,
    })
}

fn docs_v2_body_string<'a>(
    body: &'a Value,
    field: &str,
    required: bool,
) -> Result<Option<&'a str>, ApiError> {
    match body.get(field).and_then(Value::as_str) {
        Some(value) => Ok(Some(value)),
        None if required => Err(ApiError::bad_request(format!("{field} is required"))),
        None => Ok(None),
    }
}

fn docs_v2_pages_index(store: &HarnessStore) -> Result<Value, ApiError> {
    let documents = store.latest_documents().map_err(ApiError::from)?;
    let mut items = Vec::new();
    for document in documents
        .iter()
        .filter(|d| matches!(d.kind, DocumentKind::Page))
    {
        let history = store
            .document_revision_history(&document.id)
            .map_err(ApiError::from)?;
        let latest = history.iter().max_by_key(|r| r.revision_number);
        items.push(json!({
            "document_id": document.id,
            "title": document.title,
            "space_id": document.space_id,
            "parent_document_id": document.parent_document_id,
            "lifecycle_status": document.lifecycle_status,
            "block_count": document.block_ids.len(),
            "revision_number": latest.map(|r| r.revision_number).unwrap_or(0),
            "content_digest": latest.map(|r| r.content_digest.clone()),
            "updated_at": document.updated_at,
        }));
    }
    Ok(json!({ "count": items.len(), "items": items }))
}

fn docs_v2_get(store: &HarnessStore, path: &str) -> Option<ApiResponse> {
    if path == DOCS_V2_PAGES {
        return Some(finish(docs_v2_pages_index(store)));
    }
    let rest = path.strip_prefix(&format!("{DOCS_V2_PAGES}/"))?;
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [document_id] => {
            let options = crate::docs_v2_page::PageReadOptions {
                detail: "full".to_string(),
                scope: "full".to_string(),
                ..Default::default()
            };
            Some(finish(
                crate::docs_v2_page::read_page_value(store, document_id, &options)
                    .map_err(docs_v2_error)
                    .and_then(|mut page| {
                        let resolved = resolve_entity_embed_refs(store, &page)?;
                        page["resolved_embeds"] = resolved;
                        Ok(page)
                    }),
            ))
        }
        [document_id, "revisions"] => Some(finish(
            store
                .document_revision_history(document_id)
                .map_err(ApiError::from)
                .map(|history| {
                    let items: Vec<Value> = history
                        .iter()
                        .map(|revision| {
                            json!({
                                "revision_id": revision.id,
                                "revision_number": revision.revision_number,
                                "parent_revision_id": revision.parent_revision_id,
                                "content_digest": revision.content_digest,
                                "change_summary": revision.change_summary,
                                "authored_by": revision.authored_by,
                                "action_command_id": revision.action_command_id,
                                "created_at": revision.created_at,
                            })
                        })
                        .collect();
                    json!({ "count": items.len(), "items": items })
                }),
        )),
        _ => None,
    }
}

fn docs_v2_post(store: &HarnessStore, path: &str, body: &Value) -> Option<ApiResponse> {
    if path == DOCS_V2_PAGES {
        return Some(finish((|| {
            let title = docs_v2_body_string(body, "title", true)?.unwrap_or("");
            let markdown = docs_v2_body_string(body, "markdown", false)?.unwrap_or("");
            let actor = docs_v2_actor(body)?;
            let space = docs_v2_body_string(body, "space", false)?.unwrap_or("company");
            let parent = docs_v2_body_string(body, "parent", false)?;
            let summary = docs_v2_body_string(body, "summary", false)?.unwrap_or("page create");
            let action_id =
                docs_v2_body_string(body, "action_command_id", false)?.map(str::to_string);
            let slug: String = title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_string();
            let document_id = docs_v2_body_string(body, "id", false)?
                .map(str::to_string)
                .unwrap_or_else(|| format!("document-api-{slug}"));
            crate::docs_v2_page::create_page_value(
                store,
                &document_id,
                title,
                markdown,
                space,
                parent,
                actor,
                summary,
                action_id,
            )
            .map_err(docs_v2_error)
        })()));
    }
    let rest = path.strip_prefix(&format!("{DOCS_V2_PAGES}/"))?;
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        [document_id, "write"] => Some(finish((|| {
            let markdown = docs_v2_body_string(body, "markdown", true)?.unwrap_or("");
            let expected_raw = body
                .get("expected_revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| ApiError::bad_request("expected_revision is required"))?;
            let actor = docs_v2_actor(body)?;
            let title = docs_v2_body_string(body, "title", false)?;
            let summary = docs_v2_body_string(body, "summary", false)?.unwrap_or("page write");
            let action_id =
                docs_v2_body_string(body, "action_command_id", false)?.map(str::to_string);
            crate::docs_v2_page::write_page_value(
                store,
                document_id,
                markdown,
                expected_raw,
                title,
                actor,
                summary,
                action_id,
            )
            .map_err(docs_v2_error)
        })())),
        [document_id, "append"] => Some(finish((|| {
            let markdown = docs_v2_body_string(body, "markdown", true)?.unwrap_or("");
            let actor = docs_v2_actor(body)?;
            let after = docs_v2_body_string(body, "after", false)?;
            let expected = body.get("expected_revision").and_then(Value::as_u64);
            let summary = docs_v2_body_string(body, "summary", false)?.unwrap_or("page append");
            let action_id =
                docs_v2_body_string(body, "action_command_id", false)?.map(str::to_string);
            crate::docs_v2_page::append_page_value(
                store,
                document_id,
                markdown,
                after,
                expected,
                actor,
                summary,
                action_id,
            )
            .map_err(docs_v2_error)
        })())),
        _ => None,
    }
}

/// F4: resolve entity_embed targets live from their owning ledgers so embed
/// cards can show real titles instead of bare refs. Missing targets resolve
/// to an explicit `found: false` entry rather than disappearing.
fn resolve_entity_embed_refs(store: &HarnessStore, page: &Value) -> Result<Value, ApiError> {
    let mut targets: Vec<(String, String)> = Vec::new();
    if let Some(blocks) = page.get("blocks").and_then(Value::as_array) {
        for block in blocks {
            if block.get("kind").and_then(Value::as_str) != Some("entity_embed") {
                continue;
            }
            let target = block.get("content").and_then(|c| c.get("target"));
            let kind = target
                .and_then(|t| t.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let id = target
                .and_then(|t| t.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !kind.is_empty() && !id.is_empty() {
                targets.push((kind, id));
            }
        }
    }
    if targets.is_empty() {
        return Ok(json!({}));
    }
    let typed_records = store.latest_typed_records().map_err(ApiError::from)?;
    let views = store.latest_views().map_err(ApiError::from)?;
    let work_items = store.latest_work_items().map_err(ApiError::from)?;
    let mut resolved = serde_json::Map::new();
    for (kind, id) in targets {
        let entry = match kind.as_str() {
            "typed_record" => typed_records
                .iter()
                .find(|record| record.id == id)
                .map(|record| {
                    json!({
                        "kind": "typed_record",
                        "found": true,
                        "title": record.title,
                        "record_type": record.record_type,
                        "lifecycle_status": record.lifecycle_status,
                    })
                }),
            "view" => views.iter().find(|view| view.id == id).map(|view| {
                json!({
                    "kind": "view",
                    "found": true,
                    "title": view.title,
                    "mode": serde_json::to_value(view.mode).unwrap_or(json!(null)),
                })
            }),
            "work_item" => work_items.iter().find(|item| item.id == id).map(|item| {
                json!({
                    "kind": "work_item",
                    "found": true,
                    "title": item.title,
                    "status": serde_json::to_value(item.status).unwrap_or(json!(null)),
                })
            }),
            _ => None,
        };
        resolved.insert(
            format!("{kind}:{id}"),
            entry.unwrap_or_else(|| json!({ "kind": kind, "found": false })),
        );
    }
    Ok(Value::Object(resolved))
}
