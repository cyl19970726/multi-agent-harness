//! HTTP read projection and governed mutation surface for Company OS.
//!
//! All durable writes go through HarnessStore. Custom pages may read the
//! projection and dispatch declared ActionCommands, but never receive a generic
//! store-write primitive.

use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::{
    ActionCommand, ActionCommandStatus, ActionEffect, ActionPolicyDefinition, ActorRef, ActorType,
    Approval, ApprovalStatus, Assignment, AuditEvent, AuditEventKind, Block, BusinessModule,
    Commitment, CommitmentStatus, CustomPageDefinition, CustomPagePackage, Document, EntityKind,
    MemberStatus, OrgUnit, OrganizationMembership, Payment, Relation, RiskTier, TypedRecord,
    ValidateCompanyOs, View, WorkItem,
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
pub fn handle_get(store: &HarnessStore, path: &str) -> Option<ApiResponse> {
    if path == "/v1/company-os/snapshot" {
        return Some(finish(snapshot(store).map_err(ApiError::from)));
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
    if let Err(error) = authenticate_write_transport(transport_token) {
        return Some(error.response());
    }
    if path == "/v1/company-os/actions/dispatch" {
        return Some(finish(dispatch_action(store, body)));
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
        "work_items": store.latest_work_items()?,
        "assignments": store.latest_assignments()?,
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
        "document.append"
        | "block.append"
        | "typed_record.append"
        | "view.append"
        | "work_item.append"
        | "assignment.append" => (
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
        "commitment.append" => (
            "finance.commitment.write",
            RiskTier::R3,
            true,
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
        "document.append" => record
            .get("parent_document_id")
            .and_then(Value::as_str)
            .is_some_and(|id| document_in_module(store, definition, id)),
        "block.append" => record
            .get("document_id")
            .and_then(Value::as_str)
            .is_some_and(|id| document_in_module(store, definition, id)),
        "typed_record.append" | "view.append" => record
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
        "work_item.append" => record
            .get("source_document_ref")
            .and_then(Value::as_str)
            .is_some_and(|id| document_in_module(store, definition, id)),
        "assignment.append" => record
            .get("work_item_id")
            .and_then(Value::as_str)
            .is_some_and(|id| work_item_in_module(store, definition, id)),
        "approval.decide" => entity_in_module(store, definition, &command.subject_ref, 0),
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
        .is_some_and(|item| document_in_module(store, definition, &item.source_document_ref))
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
        "document.append" => "documents",
        "block.append" => "blocks",
        "typed_record.append" => "typed-records",
        "relation.append" => "relations",
        "view.append" => "views",
        "business_module.append" => "business-modules",
        "actor.append" => "actors",
        "org_unit.append" => "org-units",
        "membership.append" => "memberships",
        "work_item.append" => "work-items",
        "assignment.append" => "assignments",
        "approval.decide" => "approvals",
        "commitment.append" => "commitments",
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
