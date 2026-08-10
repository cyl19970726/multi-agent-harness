//! HTTP read projection and governed mutation surface for Company OS.
//!
//! All durable writes go through HarnessStore. Custom pages may read the
//! projection and dispatch declared ActionCommands, but never receive a generic
//! store-write primitive.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::{
    ActionCommand, ActionCommandStatus, ActionEffect, ActionPolicyDefinition, ActorRef, ActorType,
    Approval, ApprovalStatus, AuditEvent, AuditEventKind, Block, BusinessModule, Commitment,
    CommitmentStatus, CustomPageDefinition, CustomPagePackage, Document, DocumentKind, EntityKind,
    LifecycleStatus, MemberStatus, Milestone, OrgUnit, OrganizationMembership, Payment,
    Relation, RiskTier, TypedRecord, ValidateCompanyOs, View, Work,
    WorkCondition, WorkPhase, WorkResolution,
};
use harness_store::{ActionCommandClaimResult, CompanyActor, HarnessStore, StoreError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
    execution_spaces: Option<&[(String, HarnessStore)]>,
    path: &str,
) -> Option<ApiResponse> {
    if path == "/v1/company-os/snapshot" {
        return Some(finish(
            snapshot_with_execution_spaces(
                store,
                execution_store.unwrap_or(store),
                execution_spaces.unwrap_or(&[]),
            )
            .map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/work-projection" {
        return Some(finish(
            company_work_projection_from_spaces(
                execution_store.unwrap_or(store),
                execution_spaces.unwrap_or(&[]),
                &CompanyWorkQuery::default(),
            )
            .map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/works" {
        return Some(finish(
            company_work_projection_from_spaces(
                execution_store.unwrap_or(store),
                execution_spaces.unwrap_or(&[]),
                &CompanyWorkQuery::default(),
            )
            .map_err(ApiError::from),
        ));
    }
    if let Some(work_id) = path.strip_prefix("/v1/company-os/works/") {
        if work_id.is_empty() || work_id.contains('/') {
            return None;
        }
        return Some(finish(
            resolve_unique_company_work(
                execution_store.unwrap_or(store),
                execution_spaces.unwrap_or(&[]),
                work_id,
            )
            .and_then(|resolved| {
                resolved
                    .map(|(work, _)| serde_json::to_value(work))
                    .transpose()
                    .map_err(|error| ApiError::internal(error.to_string()))?
                    .ok_or_else(|| ApiError::not_found(format!("Work:{work_id}")))
            }),
        ));
    }
    // Read-only archived-source provenance and Docs health projections. They
    // resolve the latest ledger rows only; they never write or migrate rows.
    if path == "/v1/company-os/organization-provenance" {
        return Some(finish(
            organization_source_provenance(store).map_err(ApiError::from),
        ));
    }
    if path == "/v1/company-os/docs-health" {
        return Some(finish(docs_health_report(store).map_err(ApiError::from)));
    }
    if let Some(response) = docs_v2_get(
        store,
        execution_store.unwrap_or(store),
        execution_spaces.unwrap_or(&[]),
        path,
    ) {
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
    handle_post_with_execution(store, None, None, path, body, transport_token)
}

/// Handle a Company OS POST path while keeping Company Store writes separate
/// from the Execution Spaces that own authoritative Work records.
pub fn handle_post_with_execution(
    store: &HarnessStore,
    execution_store: Option<&HarnessStore>,
    execution_spaces: Option<&[(String, HarnessStore)]>,
    path: &str,
    body: &Value,
    transport_token: Option<&str>,
) -> Option<ApiResponse> {
    if !path.starts_with("/v1/company-os/") {
        return None;
    }
    // Company Work is a read-only filter over authoritative TeamWorks. It does
    // not create or mutate a second Company task object.
    if path == "/v1/company-os/work-query" {
        return Some(finish(parse::<CompanyWorkQuery>(body).and_then(|query| {
            company_work_projection_from_spaces(
                execution_store.unwrap_or(store),
                execution_spaces.unwrap_or(&[]),
                &query,
            )
            .map_err(ApiError::from)
        })));
    }
    if let Err(error) = authenticate_write_transport(transport_token) {
        return Some(error.response());
    }
    if path == "/v1/company-os/actions/dispatch" {
        return Some(finish(dispatch_action(
            store,
            execution_store.unwrap_or(store),
            execution_spaces.unwrap_or(&[]),
            body,
        )));
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
    let expected = std::env::var("FIRM_COMPANY_OS_TOKEN")
        .or_else(|_| std::env::var("HARNESS_COMPANY_OS_TOKEN"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::forbidden(
                "Company OS writes are disabled until FIRM_COMPANY_OS_TOKEN is configured",
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

/// Build Company OS truth while projecting canonical AgentMember identity
/// from the independently selected Execution Space. Company owns no runtime
/// identity payload and performs no execution-assignment join.
pub fn snapshot_with_execution(
    store: &HarnessStore,
    execution_store: &HarnessStore,
) -> Result<Value, StoreError> {
    snapshot_with_execution_spaces(store, execution_store, &[])
}

pub fn snapshot_with_execution_spaces(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
) -> Result<Value, StoreError> {
    let actors = normalized_actors(store.latest_actors()?);
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
        "agent_members": execution_store.all_trust_agent_members()?,
        "milestones": store.latest_milestones()?,
        "works": company_work_records_from_spaces(execution_store, execution_spaces)?,
        "work": company_work_projection_from_spaces(
            execution_store,
            execution_spaces,
            &CompanyWorkQuery::default(),
        )?,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct CompanyWorkQuery {
    team_ids: Vec<String>,
    team_run_ids: Vec<String>,
    phases: Vec<WorkPhase>,
    conditions: Vec<WorkCondition>,
    resolutions: Vec<WorkResolution>,
    owner_member_ids: Vec<String>,
}

/// Resolve a Work through its unique owning Execution Space. When the
/// aggregate contains the same Work id in more than one space, no caller may
/// guess an owner.
fn resolve_unique_company_work(
    selected_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    work_id: &str,
) -> Result<Option<(Work, Option<String>)>, ApiError> {
    if execution_spaces.is_empty() {
        return Ok(selected_store
            .latest_works()?
            .into_iter()
            .find(|work| work.id == work_id)
            .map(|work| (work, None)));
    }
    let mut matches = Vec::new();
    for (space_id, store) in execution_spaces {
        if let Some(work) = store
            .latest_works()?
            .into_iter()
            .find(|work| work.id == work_id)
        {
            matches.push((work, Some(space_id.clone())));
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(ApiError::conflict(format!(
            "Work:{work_id} has duplicate owners across Execution Spaces: {}",
            matches
                .iter()
                .filter_map(|(_, space_id)| space_id.as_deref())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn company_entity_exists_with_execution(
    company_store: &HarnessStore,
    selected_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    reference: &harness_core::EntityRef,
) -> Result<bool, ApiError> {
    if reference.kind == EntityKind::Work {
        return Ok(
            resolve_unique_company_work(selected_store, execution_spaces, &reference.id)?.is_some(),
        );
    }
    company_store
        .company_entity_exists(reference)
        .map_err(ApiError::from)
}

fn company_work_records_from_spaces(
    selected_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
) -> Result<Vec<Work>, StoreError> {
    let mut works = if execution_spaces.is_empty() {
        selected_store.latest_works()?
    } else {
        let mut rows = Vec::new();
        for (_, store) in execution_spaces {
            rows.extend(store.latest_works()?);
        }
        rows
    };
    works.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then(left.version.cmp(&right.version))
    });
    Ok(works)
}

fn company_work_projection_from_spaces(
    selected_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    query: &CompanyWorkQuery,
) -> Result<Value, StoreError> {
    let works = company_work_records_from_spaces(selected_store, execution_spaces)?
        .into_iter()
        .filter(|work| {
            (query.team_ids.is_empty()
                || work
                    .team_id
                    .as_ref()
                    .is_some_and(|id| query.team_ids.contains(id)))
                && (query.team_run_ids.is_empty() || query.team_run_ids.contains(&work.team_run_id))
                && (query.phases.is_empty() || query.phases.contains(&work.phase))
                && (query.conditions.is_empty() || query.conditions.contains(&work.condition))
                && (query.resolutions.is_empty()
                    || work
                        .resolution
                        .is_some_and(|resolution| query.resolutions.contains(&resolution)))
                && (query.owner_member_ids.is_empty()
                    || work
                        .owner_member_id
                        .as_ref()
                        .is_some_and(|id| query.owner_member_ids.contains(id)))
        })
        .collect::<Vec<_>>();
    let mut routes = serde_json::Map::new();
    let mut conflicts = Vec::new();
    if execution_spaces.is_empty() {
        for work in &works {
            routes.insert(
                work.id.clone(),
                json!({"execution_space_id": null, "command": "team-run work"}),
            );
        }
    } else {
        let mut owners = BTreeMap::<String, Vec<String>>::new();
        for (space_id, store) in execution_spaces {
            for work in store.latest_works()? {
                if works.iter().any(|candidate| candidate.id == work.id) {
                    owners.entry(work.id).or_default().push(space_id.clone());
                }
            }
        }
        for (work_id, space_ids) in owners {
            if space_ids.len() == 1 {
                routes.insert(
                    work_id,
                    json!({"execution_space_id": space_ids[0], "command": "team-run work"}),
                );
            } else {
                conflicts.push(json!({
                    "kind": "duplicate_work_id_across_execution_spaces",
                    "work_id": work_id,
                    "execution_space_ids": space_ids,
                }));
            }
        }
    }
    let count =
        |predicate: &dyn Fn(&Work) -> bool| works.iter().filter(|work| predicate(work)).count();
    let mut board = BTreeMap::<String, Vec<String>>::new();
    for work in &works {
        let key = format!("{:?}", work.phase).to_lowercase();
        board.entry(key).or_default().push(work.id.clone());
    }
    Ok(json!({
        "projection_kind": "company_work_aggregate",
        "authority": "team_work",
        "read_only": true,
        "scope": if execution_spaces.is_empty() { "selected_execution_space" } else { "all_execution_spaces" },
        "query": query,
        "summary": {
            "total": works.len(),
            "open": count(&|work| work.phase == WorkPhase::Open),
            "active": count(&|work| work.phase == WorkPhase::Active),
            "review": count(&|work| work.phase == WorkPhase::Review),
            "closed": count(&|work| work.phase == WorkPhase::Closed),
            "blocked": count(&|work| work.condition == WorkCondition::Blocked),
            "on_hold": count(&|work| work.condition == WorkCondition::OnHold),
        },
        "board": board,
        "works": works,
        "routes": routes,
        "conflicts": conflicts,
    }))
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
                "id": actor.id, "actor_type": "Agent Membership",
                "display_name": actor.id, "record": actor,
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

/// Read-only Organization provenance: every member resolves with its durable
/// member status (archived members stay navigable instead of vanishing), and
/// every Company actor remains navigable without copying AgentMember payload.
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
                    Vec::<String>::new(),
                ),
                CompanyActor::Agent(member) => (
                    member.id.clone(),
                    "agent",
                    member.id,
                    member.status,
                    Vec::<String>::new(),
                ),
                CompanyActor::External(member) => (
                    member.id,
                    "external",
                    member.display_name_or_organization,
                    member.status,
                    Vec::<String>::new(),
                ),
                CompanyActor::Service(member) => (
                    member.id,
                    "service",
                    member.display_name,
                    member.status,
                    Vec::<String>::new(),
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
        agentfirm_api::{
            ActorKind, ActorRef, AgentMember, AgentMemberOrganizationStatus, MutationContext,
            PermissionCeiling,
        },
        AgentMembership, AgentTeam, AgentTeamRun, AgentTeamStatus, ExecutionNode,
        ExecutionNodeStatus, MemberRun, Mission, MissionStatus,
    };

    fn insert_projection_team(store: &HarnessStore, team_id: &str, mission_id: &str) {
        const NODE_ID: &str = "00000000-0000-4000-8000-000000000001";
        store
            .append_mission(&Mission {
                id: mission_id.to_string(),
                title: "Projection mission".to_string(),
                objective: "Exercise Company projection joins".to_string(),
                context: String::new(),
                desired_outcome: None,
                status: MissionStatus::Planned,
                wave_ids: Vec::new(),
                outcome_summary: None,
                completed_by: None,
                created_at: "1".to_string(),
                updated_at: "1".to_string(),
                completed_at: None,
            })
            .unwrap();
        store
            .insert_execution_node(&ExecutionNode {
                id: NODE_ID.to_string(),
                display_name: "Projection node".to_string(),
                status: ExecutionNodeStatus::Active,
                created_at: "1".to_string(),
                updated_at: "1".to_string(),
            })
            .unwrap();
        store
            .insert_agent_team_with_unique_mission(&AgentTeam {
                id: team_id.to_string(),
                name: "Projection Team".to_string(),
                description: "Canonical Team join fixture".to_string(),
                mission_id: mission_id.to_string(),
                host_agent_id: "host".to_string(),
                node_id: NODE_ID.to_string(),
                status: AgentTeamStatus::Active,
                member_ids: Vec::new(),
                created_at: "1".to_string(),
                updated_at: "1".to_string(),
            })
            .unwrap();
    }

    fn standing(id: &str, execution_ref: Option<&str>) -> AgentMembership {
        serde_json::from_value(json!({
            "id": id, "display_name": id, "role": "builder",
            "agent_member_ref": execution_ref,
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

    fn insert_test_work(store: &HarnessStore, id: &str, team_run_id: &str, event_id: &str) {
        store
            .insert_work(
                harness_core::Work {
                    id: id.to_string(),
                    team_run_id: team_run_id.to_string(),
                    team_id: None,
                    created_by_member_id: None,
                    parent_work_id: None,
                    title: id.to_string(),
                    context_markdown: "projection".to_string(),
                    completion_criteria_markdown: "done".to_string(),
                    phase: harness_core::WorkPhase::Open,
                    condition: harness_core::WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: None,
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
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                harness_core::WorkCommandContext {
                    event_id: event_id.to_string(),
                    performed_by_actor: harness_core::TeamActorRef {
                        kind: harness_core::TeamActorKind::Host,
                        id: "host".to_string(),
                        display_name: None,
                        authn_source: Some("test".to_string()),
                    },
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("command-{event_id}"),
                    created_at: "1".to_string(),
                    duplicate_ok: false,
                },
            )
            .unwrap();
    }

    #[test]
    fn work_query_post_aggregates_all_execution_spaces_and_reports_duplicate_ids() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let company_store = HarnessStore::new(
            std::env::temp_dir().join(format!("company-work-query-company-{nonce}")),
        );
        let first_store = HarnessStore::new(
            std::env::temp_dir().join(format!("company-work-query-first-{nonce}")),
        );
        let second_store = HarnessStore::new(
            std::env::temp_dir().join(format!("company-work-query-second-{nonce}")),
        );
        for store in [&company_store, &first_store, &second_store] {
            store.init().unwrap();
        }
        for (store, run_id) in [(&first_store, "run-a"), (&second_store, "run-b")] {
            store
                .append_team_run(
                    &serde_json::from_value(json!({
                        "id": run_id,
                        "agent_team_id": format!("team-{run_id}"),
                        "execution_node_id": "00000000-0000-4000-8000-000000000001",
                        "project_binding_id": "projection-project",
                        "host_surface": "test",
                        "objective": "cross-space Work projection",
                        "status": "running",
                        "member_run_ids": [],
                        "created_at": "1",
                        "updated_at": "1"
                    }))
                    .unwrap(),
                )
                .unwrap();
        }
        insert_test_work(&first_store, "work-first", "run-a", "event-first");
        insert_test_work(&second_store, "work-second", "run-b", "event-second");
        insert_test_work(&first_store, "work-duplicate", "run-a", "event-duplicate-a");
        insert_test_work(
            &second_store,
            "work-duplicate",
            "run-b",
            "event-duplicate-b",
        );
        let spaces = vec![
            ("space-a".to_string(), first_store.clone()),
            ("space-b".to_string(), second_store.clone()),
        ];

        let response = handle_post_with_execution(
            &company_store,
            Some(&first_store),
            Some(&spaces),
            "/v1/company-os/work-query",
            &json!({"team_run_ids": ["run-b"]}),
            None,
        )
        .unwrap();
        assert_eq!(response.status, "200 OK");
        assert_eq!(response.body["result"]["summary"]["total"], 2);
        assert_eq!(response.body["result"]["works"][0]["team_run_id"], "run-b");
        assert_eq!(
            response.body["result"]["routes"]["work-second"]["execution_space_id"],
            "space-b"
        );
        assert_eq!(
            response.body["result"]["conflicts"][0]["work_id"],
            "work-duplicate"
        );
        assert!(response.body["result"]["routes"]
            .get("work-duplicate")
            .is_none());

        assert!(company_store.latest_works().unwrap().is_empty());
        let work_ref = harness_core::EntityRef {
            kind: EntityKind::Work,
            id: "work-first".to_string(),
        };
        assert!(company_entity_exists_with_execution(
            &company_store,
            &first_store,
            &spaces,
            &work_ref,
        )
        .unwrap());
        let missing_ref = harness_core::EntityRef {
            kind: EntityKind::Work,
            id: "work-missing".to_string(),
        };
        assert!(!company_entity_exists_with_execution(
            &company_store,
            &first_store,
            &spaces,
            &missing_ref,
        )
        .unwrap());
        let duplicate_ref = harness_core::EntityRef {
            kind: EntityKind::Work,
            id: "work-duplicate".to_string(),
        };
        assert!(company_entity_exists_with_execution(
            &company_store,
            &first_store,
            &spaces,
            &duplicate_ref,
        )
        .is_err());

        let canonical_list = handle_get(
            &company_store,
            Some(&first_store),
            Some(&spaces),
            "/v1/company-os/works",
        )
        .unwrap();
        assert_eq!(canonical_list.status, "200 OK");
        assert_eq!(canonical_list.body["result"]["authority"], "team_work");
        let canonical_item = handle_get(
            &company_store,
            Some(&first_store),
            Some(&spaces),
            "/v1/company-os/works/work-first",
        )
        .unwrap();
        assert_eq!(canonical_item.status, "200 OK");
        assert_eq!(canonical_item.body["result"]["id"], "work-first");
        let missing_item = handle_get(
            &company_store,
            Some(&first_store),
            Some(&spaces),
            "/v1/company-os/works/work-missing",
        )
        .unwrap();
        assert_eq!(missing_item.status, "404 Not Found");
        let duplicate_item = handle_get(
            &company_store,
            Some(&first_store),
            Some(&spaces),
            "/v1/company-os/works/work-duplicate",
        )
        .unwrap();
        assert_eq!(duplicate_item.status, "409 Conflict");

        for store in [&company_store, &first_store, &second_store] {
            let _ = std::fs::remove_dir_all(store.root());
        }
    }

    #[test]
    fn snapshot_projects_canonical_agent_members_from_the_execution_space() {
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
        let actor = ActorRef {
            kind: ActorKind::Human,
            id: "operator".to_string(),
        };
        execution_store
            .create_trust_agent_member(
                &MutationContext {
                    execution_space_id: "space-test".to_string(),
                    authenticated_actor: actor.clone(),
                    authority_actor: None,
                    command_name: "agent_member.create".to_string(),
                    idempotency_key: "root-lead".to_string(),
                    expected_version: 0,
                },
                AgentMember {
                    id: "root-lead".to_string(),
                    name: "Foundation Lead".to_string(),
                    description: "Durable-only root Team Lead".to_string(),
                    role: "lead".to_string(),
                    capabilities: vec!["company_os.read".to_string()],
                    skill_refs: Vec::new(),
                    provider_profile_ref: Some("codex/default".to_string()),
                    model_preference: None,
                    workspace_policy: "managed-worktree".to_string(),
                    permission_ceiling: PermissionCeiling::ReadOnly,
                    organization_status: AgentMemberOrganizationStatus::Active,
                    version: 1,
                    created_by: actor,
                    created_at: "unix-ms:1".to_string(),
                    updated_at: "unix-ms:1".to_string(),
                },
            )
            .unwrap();

        let projected = snapshot_with_execution(&company_store, &execution_store).unwrap();
        assert_eq!(projected["agent_members"][0]["id"], "root-lead");
        assert_eq!(projected["agent_members"][0]["name"], "Foundation Lead");
        assert!(
            projected["agent_members"][0]
                .get("runtime_status")
                .is_none(),
            "durable Organization identity must not absorb runtime state"
        );

        let _ = std::fs::remove_dir_all(company_root);
        let _ = std::fs::remove_dir_all(execution_root);
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

fn dispatch_action(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    body: &Value,
) -> Result<Value, ApiError> {
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
    if !company_entity_exists_with_execution(
        store,
        execution_store,
        execution_spaces,
        &command.subject_ref,
    )? {
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
    validate_definition_scope(
        store,
        execution_store,
        execution_spaces,
        &declaration,
        &command,
        &record,
    )?;
    if command.command_name == "approval.decide" {
        validate_approval_decision(store, &command, &record)?;
    }
    if command.command_name == "approval.request" {
        validate_approval_request(store, &command, &record)?;
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
        "typed_record.append" | "view.append" => (
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
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
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
        "relation.append" => {
            let references = ["from_ref", "to_ref"]
                .iter()
                .map(|field| {
                    record
                        .get(*field)
                        .cloned()
                        .and_then(|value| {
                            serde_json::from_value::<harness_core::EntityRef>(value).ok()
                        })
                        .ok_or_else(|| {
                            ApiError::validation(format!("relation.append record requires {field}"))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut in_scope = true;
            for reference in references {
                in_scope &= entity_in_module(
                    store,
                    execution_store,
                    execution_spaces,
                    definition,
                    &reference,
                    0,
                )?;
            }
            in_scope
        }
        "approval.request" | "approval.decide" => entity_in_module(
            store,
            execution_store,
            execution_spaces,
            definition,
            &command.subject_ref,
            0,
        )?,
        "commitment.propose" => {
            command.subject_ref.kind == EntityKind::Work
                && work_in_module(
                    store,
                    execution_store,
                    execution_spaces,
                    definition,
                    &command.subject_ref.id,
                )?
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

fn work_in_module(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    definition: &CustomPageDefinition,
    work_id: &str,
) -> Result<bool, ApiError> {
    if resolve_unique_company_work(execution_store, execution_spaces, work_id)?.is_none() {
        return Ok(false);
    }
    Ok(store.latest_milestones()?.into_iter().any(|milestone| {
        milestone.business_module_ref.as_deref() == Some(definition.module_id.as_str())
            && milestone
                .work_refs
                .iter()
                .any(|reference| reference == work_id)
    }))
}

fn entity_in_module(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    definition: &CustomPageDefinition,
    reference: &harness_core::EntityRef,
    depth: usize,
) -> Result<bool, ApiError> {
    if depth > 8 {
        return Ok(false);
    }
    Ok(match reference.kind {
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
        EntityKind::Work => work_in_module(
            store,
            execution_store,
            execution_spaces,
            definition,
            &reference.id,
        )?,
        EntityKind::Approval => {
            if let Some(approval) = store
                .latest_approvals()?
                .into_iter()
                .find(|item| item.id == reference.id)
            {
                entity_in_module(
                    store,
                    execution_store,
                    execution_spaces,
                    definition,
                    &approval.subject_ref,
                    depth + 1,
                )?
            } else {
                false
            }
        }
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
    })
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
            && ((relation.from_ref.kind == EntityKind::Work
                && relation.from_ref.id == command.subject_ref.id)
                || (relation.to_ref.kind == EntityKind::Work
                    && relation.to_ref.id == command.subject_ref.id))
    });
    if !linked_to_work {
        return Err(ApiError::validation(
            "commitment.propose requires a Relation linking the Commitment context to its Work",
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

fn docs_v2_get(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    path: &str,
) -> Option<ApiResponse> {
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
                        let resolved = resolve_entity_embed_refs(
                            store,
                            execution_store,
                            execution_spaces,
                            &page,
                        )?;
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
fn resolve_entity_embed_refs(
    store: &HarnessStore,
    execution_store: &HarnessStore,
    execution_spaces: &[(String, HarnessStore)],
    page: &Value,
) -> Result<Value, ApiError> {
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
            "work" => resolve_unique_company_work(execution_store, execution_spaces, &id)?.map(
                |(work, _)| {
                    json!({
                        "kind": "work",
                        "found": true,
                        "title": work.title,
                        "phase": work.phase,
                        "condition": work.condition,
                        "resolution": work.resolution,
                    })
                },
            ),
            _ => None,
        };
        resolved.insert(
            format!("{kind}:{id}"),
            entry.unwrap_or_else(|| json!({ "kind": kind, "found": false })),
        );
    }
    Ok(Value::Object(resolved))
}
