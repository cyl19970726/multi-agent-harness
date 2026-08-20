//! Server-built, read-only RoleViews for the local AgentFirm product loop.
//!
//! The browser consumes these bounded projections and never folds ledgers or
//! invents lifecycle state. All writes remain on the canonical Mission Log
//! mutation service shipped by the historical Wave 4A development batch.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::agentfirm_api::{ActorKind, ActorRef};
use harness_core::{
    AgentTeam, AgentTeamRun, HostControlMode, NativeSessionRef, Work, WorkCondition, WorkPhase,
};
use harness_store::HarnessStore;
use serde_json::{json, Value};

pub(crate) const SCHEMA_VERSION: &str = "agentfirm.role_views.v1";

pub(crate) struct HttpResponse {
    pub status: &'static str,
    pub body: Value,
}

pub(crate) struct ReadIdentity {
    pub actor: ActorRef,
    pub authority_actors: Vec<ActorRef>,
}

#[derive(Default)]
struct Query {
    values: BTreeMap<String, Vec<String>>,
    delegated: Option<bool>,
    limit: usize,
    cursor: Option<String>,
    company: Option<String>,
}

impl Query {
    fn parse(target: &str) -> Result<Self, String> {
        let allowed = BTreeSet::from([
            "team_id",
            "mission_id",
            "node_id",
            "host_id",
            "member_id",
            "agent_id",
            "assignee_membership_id",
            "assignee_kind",
            "phase",
            "condition",
            "resolution",
            "priority",
            "module_id",
            "gate_state",
            "delegated",
            "updated_after",
            "updated_before",
            "limit",
            "cursor",
            "project",
            "space",
            "company",
        ]);
        let mut parsed = Self {
            limit: 50,
            ..Self::default()
        };
        if let Some(raw) = target.split_once('?').map(|(_, query)| query) {
            for pair in raw.split('&').filter(|pair| !pair.is_empty()) {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                if !allowed.contains(key) {
                    return Err(format!("unsupported query field: {key}"));
                }
                match key {
                    "limit" => {
                        parsed.limit = value
                            .parse::<usize>()
                            .map_err(|_| "limit must be an integer")?;
                        if parsed.limit == 0 || parsed.limit > 200 {
                            return Err("limit must be between 1 and 200".into());
                        }
                    }
                    "cursor" => parsed.cursor = Some(value.to_string()),
                    "delegated" => {
                        parsed.delegated = Some(match value {
                            "true" | "1" => true,
                            "false" | "0" => false,
                            _ => return Err("delegated must be true or false".into()),
                        })
                    }
                    "company" => parsed.company = Some(value.to_string()),
                    "project" | "space" => {}
                    _ => parsed
                        .values
                        .entry(key.to_string())
                        .or_default()
                        .push(value.to_string()),
                }
            }
        }
        Ok(parsed)
    }

    fn matches(&self, field: &str, value: Option<&str>) -> bool {
        self.values.get(field).is_none_or(|wanted| {
            value.is_some_and(|actual| wanted.iter().any(|item| item == actual))
        })
    }
}

struct Facts {
    space_id: String,
    store_identity: String,
    sequence: u64,
    work_sequence: u64,
    team_sequence: u64,
    run_sequence: u64,
    team_revisions: BTreeMap<String, u64>,
    run_revisions: BTreeMap<String, u64>,
    canonical_versions: BTreeMap<(String, String), u64>,
    teams: Vec<AgentTeam>,
    runs: Vec<AgentTeamRun>,
    works: Vec<Work>,
    members: Vec<Value>,
    member_runs: Vec<Value>,
    provider_runtime_projections: Vec<Value>,
    messages: Vec<Value>,
    message_deliveries: Vec<Value>,
    agent_identities: Vec<Value>,
    agent_sessions: Vec<Value>,
    team_memberships: Vec<Value>,
    message_subscriptions: Vec<Value>,
    work_execution_bindings: Vec<Value>,
    canonical_messages: Vec<Value>,
    canonical_message_deliveries: Vec<Value>,
    runtime_commands: Vec<Value>,
    work_deliveries: Vec<Value>,
    work_events: Vec<Value>,
    side: Vec<Value>,
    /// Read-only legacy-context annotations for tolerated DOC-108 pre-cutover
    /// references (same shape as the #488 snapshot integrity_annotations).
    integrity_annotations: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct SnapshotPoint {
    execution_space_id: String,
    store_identity: String,
    trust_store_sequence: u64,
    work_operation_count: u64,
    team_row_count: u64,
    team_run_row_count: u64,
}

fn side_record_kind(value: &Value) -> Option<&'static str> {
    if value.get("canonical_root").is_some() && value.get("member_run_id").is_some() {
        Some("workspace_binding")
    } else if value.get("requirement_set_fingerprint").is_some() {
        Some("gate_requirement")
    } else if value.get("verdict").is_some() && value.get("requirement_id").is_some() {
        Some("gate_evaluation")
    } else if value.get("authority_actor").is_some() && value.get("requirement_id").is_some() {
        Some("gate_waiver")
    } else if value.get("report_revision").is_some() && value.get("authored_by").is_some() {
        Some("work_report")
    } else if value.get("detail_markdown").is_some() && value.get("reported_by").is_some() {
        Some("work_finding")
    } else if value.get("observed_failure").is_some() && value.get("reported_by").is_some() {
        Some("failure_analysis")
    } else if value.get("module_id").is_some() && value.get("attached_by").is_some() {
        Some("work_module_binding")
    } else if value.get("source_work_ref").is_some() && value.get("target_work_ref").is_some() {
        Some("work_delegation")
    } else if value.get("message_id").is_some() && value.get("recipient_member_run_id").is_some() {
        Some("message_delivery")
    } else if value.get("work_id").is_some()
        && value.get("recipient_member_run_id").is_some()
        && value.get("work_revision").is_some()
    {
        Some("work_delivery")
    } else if value.get("agent_member_id").is_some() && value.get("runtime_generation").is_some() {
        Some("member_run")
    } else {
        None
    }
}

fn fold_side_record(
    latest: &mut BTreeMap<(String, String), Value>,
    unkeyed: &mut Vec<Value>,
    kind: Option<&str>,
    value: Value,
) -> Result<(), String> {
    let kind = kind.or_else(|| side_record_kind(&value));
    let id = value.get("id").and_then(Value::as_str);
    let (Some(kind), Some(id)) = (kind, id) else {
        unkeyed.push(value);
        return Ok(());
    };
    let key = (kind.to_string(), id.to_string());
    if let Some(existing) = latest.get(&key) {
        match (
            existing.get("version").and_then(Value::as_u64),
            value.get("version").and_then(Value::as_u64),
        ) {
            (Some(current_version), Some(next_version)) if next_version < current_version => {
                return Ok(())
            }
            (Some(current_version), Some(next_version))
                if next_version == current_version && existing != &value =>
            {
                return Err(format!(
                    "IDENTITY_CONFLICT: {kind} {id} has two different projections at version {next_version}"
                ));
            }
            // Versionless immutable records are ordered by their containing
            // CanonicalOperation. Later append wins; no revision is invented.
            _ => {}
        }
    }
    latest.insert(key, value);
    Ok(())
}

impl Facts {
    fn read(space_id: &str, store: &HarnessStore) -> Result<Self, String> {
        let work_operations = store.work_operations().map_err(|error| error.to_string())?;
        let operations = store
            .canonical_operations_for_space(space_id)
            .map_err(|error| error.to_string())?;
        let sequence = operations
            .iter()
            .map(|op| op.event.store_sequence)
            .max()
            .unwrap_or(0);
        let canonical_versions = operations.iter().fold(
            BTreeMap::<(String, String), u64>::new(),
            |mut versions, operation| {
                versions.insert(
                    (
                        operation.event.aggregate_kind.clone(),
                        operation.event.aggregate_id.clone(),
                    ),
                    operation.event.resulting_version,
                );
                versions
            },
        );
        let mut works = store
            .latest_works()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|work| (work.id.clone(), work))
            .collect::<BTreeMap<_, _>>();
        let mut latest_side = BTreeMap::new();
        let mut unkeyed_side = Vec::new();
        for operation in &operations {
            if operation.event.aggregate_kind == "work" {
                if let Ok(work) =
                    serde_json::from_value::<Work>(operation.resulting_projection.clone())
                {
                    works.insert(work.id.clone(), work);
                }
            }
            if operation.event.aggregate_kind != "work" {
                fold_side_record(
                    &mut latest_side,
                    &mut unkeyed_side,
                    Some(&operation.event.aggregate_kind),
                    operation.resulting_projection.clone(),
                )?;
            }
            for value in operation.immutable_side_records.clone() {
                if let Ok(work) = serde_json::from_value::<Work>(value.clone()) {
                    works.insert(work.id.clone(), work);
                } else {
                    fold_side_record(&mut latest_side, &mut unkeyed_side, None, value)?;
                }
            }
        }
        let mut side = latest_side.into_values().collect::<Vec<_>>();
        side.extend(unkeyed_side);
        let team_rows = store.teams().map_err(|error| error.to_string())?;
        let run_rows = store.team_runs().map_err(|error| error.to_string())?;
        let team_revisions = team_rows
            .iter()
            .fold(BTreeMap::new(), |mut revisions, team| {
                *revisions.entry(team.id.clone()).or_insert(0) += 1;
                revisions
            });
        let mut all_latest_runs = BTreeMap::new();
        for run in &run_rows {
            all_latest_runs.insert(run.id.clone(), run.clone());
        }
        let mut latest_runs = BTreeMap::new();
        let mut integrity_annotations = Vec::new();
        for (id, run) in all_latest_runs {
            // DOC-108 pre-cutover tolerance (#488 doctrine): a TeamRun whose
            // AgentTeam exists only in the retired legacy teams.jsonl ledger
            // may declare MemberRuns that never materialized canonically. The
            // read-only resolver reports those refs instead of failing the
            // view; they are rendered as read-only legacy context with an
            // integrity annotation. A post-cutover TeamRun still fails closed.
            let resolution = store
                .current_team_run_execution_space_readonly(&run)
                .map_err(|error| error.to_string())?;
            for member_run_id in &resolution.tolerated_legacy_member_run_ids {
                integrity_annotations.push(serde_json::json!({
                    "kind": "pre_cutover_unmaterialized_member_run_ref",
                    "team_run_id": id,
                    "member_run_id": member_run_id,
                    "annotation": harness_store::PRE_CUTOVER_UNMATERIALIZED_MEMBER_RUN_ANNOTATION,
                }));
            }
            if resolution.execution_space_id == space_id {
                latest_runs.insert(id, run);
            }
        }
        let run_revisions = run_rows.iter().fold(BTreeMap::new(), |mut revisions, run| {
            if latest_runs.contains_key(&run.id) {
                *revisions.entry(run.id.clone()).or_insert(0) += 1;
            }
            revisions
        });
        let store_identity = std::fs::canonicalize(store.root())
            .unwrap_or_else(|_| store.root().to_path_buf())
            .display()
            .to_string();
        let team_memberships = store
            .fabric_team_memberships(space_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        ensure_active_membership_cardinality(&team_memberships)?;
        Ok(Self {
            space_id: space_id.to_string(),
            store_identity,
            sequence,
            work_sequence: work_operations.len() as u64,
            team_sequence: team_rows.len() as u64,
            run_sequence: run_revisions.values().sum(),
            team_revisions,
            run_revisions,
            canonical_versions,
            teams: store
                .latest_teams()
                .map_err(|error| error.to_string())?
                .into_values()
                .collect(),
            runs: latest_runs.into_values().collect(),
            works: works.into_values().collect(),
            members: store
                .trust_agent_members(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            member_runs: store
                .trust_member_runs(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            provider_runtime_projections: store
                .member_runs()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            messages: store
                .fabric_messages(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            message_deliveries: store
                .fabric_message_deliveries(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            agent_identities: store
                .fabric_agent_identities(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            agent_sessions: store
                .fabric_agent_sessions(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            team_memberships,
            message_subscriptions: store
                .fabric_message_subscriptions(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            work_execution_bindings: store
                .fabric_work_execution_bindings(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            canonical_messages: store
                .fabric_messages(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            canonical_message_deliveries: store
                .fabric_message_deliveries(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            runtime_commands: store
                .runtime_commands(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            work_deliveries: store
                .trust_work_deliveries(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            work_events: work_operations
                .iter()
                .map(|operation| serde_json::to_value(&operation.event).unwrap_or(Value::Null))
                .collect(),
            side,
            integrity_annotations,
        })
    }

    fn snapshot_point(&self) -> SnapshotPoint {
        SnapshotPoint {
            execution_space_id: self.space_id.clone(),
            store_identity: self.store_identity.clone(),
            trust_store_sequence: self.sequence,
            work_operation_count: self.work_sequence,
            team_row_count: self.team_sequence,
            team_run_row_count: self.run_sequence,
        }
    }

    fn latest_run(&self, team_id: &str) -> Option<&AgentTeamRun> {
        self.runs
            .iter()
            .filter(|run| run.agent_team_id == team_id)
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)))
    }
}

fn ensure_active_membership_cardinality(team_memberships: &[Value]) -> Result<(), String> {
    let mut active_membership_keys = BTreeSet::new();
    for membership in team_memberships
        .iter()
        .filter(|membership| membership["state"] == "active")
    {
        let key = (
            membership["team_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            membership["agent_member_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
        if !active_membership_keys.insert(key.clone()) {
            return Err(format!(
                "IDENTITY_CONFLICT: Team {} and AgentIdentity {} have multiple active TeamMembership generations",
                key.0, key.1
            ));
        }
    }
    Ok(())
}

pub(crate) fn now() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let seconds = ms.div_euclid(1_000);
    let millis = ms.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn enum_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn records(facts: &Facts, predicate: impl Fn(&Value) -> bool) -> Vec<Value> {
    facts
        .side
        .iter()
        .filter(|value| predicate(value))
        .cloned()
        .collect()
}

/// Project an actor reference with its server-resolved display label. The
/// browser never stitches identity lookups: a canonical AgentMember id (or a
/// MemberRun id bound to one) carries its current durable name; anything else
/// keeps a null label and the raw id stays the secondary display.
fn role_actor_ref(facts: &Facts, value: &Value) -> Value {
    match (value.get("kind"), value.get("id")) {
        (Some(kind), Some(id)) if kind.is_string() && id.is_string() => {
            let display_name = id.as_str().and_then(|id| {
                let member = facts
                    .members
                    .iter()
                    .find(|member| member["id"].as_str() == Some(id));
                let member_run_owner = || {
                    facts
                        .member_runs
                        .iter()
                        .find(|run| run["id"].as_str() == Some(id))
                        .and_then(|run| run["agent_member_id"].as_str())
                        .and_then(|owner| {
                            facts
                                .members
                                .iter()
                                .find(|member| member["id"].as_str() == Some(owner))
                        })
                };
                member
                    .or_else(member_run_owner)
                    .and_then(|member| member["name"].as_str())
            });
            json!({"kind":kind,"id":id,"display_name":display_name})
        }
        _ => Value::Null,
    }
}

/// Facts-free actor projection for record summaries; the activity assembler
/// re-resolves these through `role_actor_ref` to attach display labels.
fn role_actor_ref_unresolved(value: &Value) -> Value {
    match (value.get("kind"), value.get("id")) {
        (Some(kind), Some(id)) if kind.is_string() && id.is_string() => {
            json!({"kind":kind,"id":id})
        }
        _ => Value::Null,
    }
}

/// Project canonical records into one closed, display-safe RoleView summary.
/// RoleViews deliberately do not leak evolving ledger wire objects into the
/// browser: every field below is stable, bounded, and schema-validated.
fn record_summary(kind: &str, value: &Value) -> Value {
    let first_string = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(Value::as_str))
    };
    let actor_ref = [
        "authored_by",
        "reported_by",
        "performed_by",
        "performed_by_actor",
        "authority_actor",
        "evaluator_ref",
    ]
    .iter()
    .find_map(|key| value.get(*key))
    .map(role_actor_ref_unresolved)
    .filter(|actor| !actor.is_null());
    let source_id = value
        .get("source_work_ref")
        .and_then(|reference| reference.get("work_id"))
        .and_then(Value::as_str);
    let target_id = value
        .get("target_work_ref")
        .and_then(|reference| reference.get("work_id"))
        .and_then(Value::as_str);
    json!({
        "kind":kind,
        "id":first_string(&["id","message_id","work_id"]).unwrap_or("unknown"),
        "work_id":first_string(&["work_id"]),
        "member_run_id":first_string(&["member_run_id","recipient_member_run_id"]),
        "requirement_id":first_string(&["requirement_id"]),
        "status":first_string(&["state","status","lifecycle","verdict","runtime_status"]),
        "version":value.get("version").and_then(Value::as_u64),
        "actor_ref":actor_ref,
        "summary":first_string(&["summary","summary_markdown","detail_markdown","observed_failure","reason"]),
        "created_at":first_string(&["created_at","evaluated_at","updated_at"]),
        "source_id":source_id,
        "target_id":target_id,
        "locator":first_string(&["canonical_root","project_root","store_root"]),
    })
}

fn record_summaries(kind: &str, values: Vec<Value>) -> Vec<Value> {
    values
        .iter()
        .map(|value| record_summary(kind, value))
        .collect()
}

const ACTIVITY_LIMIT: usize = 100;

fn display_text(value: Option<&str>) -> Option<String> {
    value.map(|text| text.chars().take(500).collect())
}

fn team_activity(
    facts: &Facts,
    team_work_ids: &BTreeSet<&str>,
    run_id: Option<&str>,
) -> (Vec<Value>, bool) {
    let mut rows = Vec::new();
    for event in facts.work_events.iter().filter(|event| {
        event["work_id"]
            .as_str()
            .is_some_and(|id| team_work_ids.contains(id))
    }) {
        rows.push(json!({"source":"work_event","id":event["id"],"work_id":event["work_id"],"actor_ref":role_actor_ref(facts,&event["performed_by_actor"]),"status":event["kind"],"summary":display_text(event["payload"].get("summary").and_then(Value::as_str)),"created_at":event["created_at"]}));
    }
    for delivery in facts.work_deliveries.iter().filter(|delivery| {
        delivery["work_id"]
            .as_str()
            .is_some_and(|id| team_work_ids.contains(id))
    }) {
        rows.push(json!({"source":"work_delivery","id":delivery["id"],"work_id":delivery["work_id"],"actor_ref":null,"status":delivery["status"],"summary":display_text(delivery["failure_detail"].as_str()),"created_at":delivery["updated_at"]}));
    }
    for message in facts
        .messages
        .iter()
        .filter(|message| run_id.is_some_and(|id| message["team_run_id"] == id))
    {
        rows.push(json!({"source":"message","id":message["id"],"work_id":message["work_id"],"actor_ref":role_actor_ref(facts,&message["sender_actor_ref"]),"status":message["kind"],"summary":display_text(message["body"].as_str()),"created_at":message["created_at"]}));
    }
    let team_message_ids = facts
        .messages
        .iter()
        .filter(|message| run_id.is_some_and(|id| message["team_run_id"] == id))
        .filter_map(|message| message["id"].as_str())
        .collect::<BTreeSet<_>>();
    // A delivery fact inherits its authored TeamMessage's Work link and
    // recipient identity. The link is copied only when the parent Message
    // canonically carries one; an unlinked Message stays honestly unlinked.
    let message_work_links = facts
        .messages
        .iter()
        .filter(|message| run_id.is_some_and(|id| message["team_run_id"] == id))
        .map(|message| {
            (
                message["id"].as_str().unwrap_or_default(),
                message["work_id"].clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for delivery in facts.message_deliveries.iter().filter(|delivery| {
        delivery["message_id"]
            .as_str()
            .is_some_and(|id| team_message_ids.contains(id))
    }) {
        let work_id = delivery["message_id"]
            .as_str()
            .and_then(|id| message_work_links.get(id))
            .cloned()
            .unwrap_or(Value::Null);
        rows.push(json!({"source":"message_delivery","id":delivery["id"],"message_id":delivery["message_id"],"work_id":work_id,"actor_ref":role_actor_ref(facts,&json!({"kind":"agent_member","id":delivery["recipient_agent_member_id"]})),"status":delivery["status"],"summary":display_text(delivery["failure_detail"].as_str()),"created_at":delivery["updated_at"]}));
    }
    for value in facts.side.iter().filter(|value| {
        value["work_id"]
            .as_str()
            .is_some_and(|id| team_work_ids.contains(id))
    }) {
        let Some(source) = side_record_kind(value).filter(|kind| {
            matches!(
                *kind,
                "work_report"
                    | "work_finding"
                    | "failure_analysis"
                    | "gate_requirement"
                    | "gate_evaluation"
                    | "gate_waiver"
            )
        }) else {
            continue;
        };
        let summary = record_summary(source, value);
        rows.push(json!({"source":source,"id":summary["id"],"work_id":summary["work_id"],"actor_ref":role_actor_ref(facts,&summary["actor_ref"]),"status":summary["status"],"summary":display_text(summary["summary"].as_str()),"created_at":summary["created_at"]}));
    }
    for command in facts.runtime_commands.iter().filter(|command| {
        command["source_record_id"]
            .as_str()
            .is_some_and(|id| team_work_ids.contains(id) || team_message_ids.contains(id))
    }) {
        rows.push(json!({"source":"runtime_command","id":command["id"],"work_id":command["source_record_id"].as_str().filter(|id|team_work_ids.contains(id)),"actor_ref":role_actor_ref(facts,&command["authenticated_actor"]),"status":command["status"],"summary":display_text(command["failure_code"].as_str()),"created_at":command["updated_at"]}));
    }
    rows.sort_by(|left, right| {
        right["created_at"]
            .as_str()
            .cmp(&left["created_at"].as_str())
            .then_with(|| right["id"].as_str().cmp(&left["id"].as_str()))
    });
    let truncated = rows.len() > ACTIVITY_LIMIT;
    rows.truncate(ACTIVITY_LIMIT);
    (rows, truncated)
}

fn member_run_summary(value: &Value) -> Value {
    json!({
        "id":value["id"],
        "agent_member_id":value["agent_member_id"],
        "team_run_id":value["team_run_id"],
        "coordination_status":value["coordination_status"],
        "runtime_status":value["runtime_status"],
        "runtime_generation":value["runtime_generation"],
        "native_session_health":value["native_session"].get("availability").cloned().unwrap_or(json!("unknown")),
    })
}

fn agent_member_summary(value: &Value) -> Value {
    json!({
        "id":value["id"],
        "role":value["role"],
        "organization_status":value["organization_status"],
    })
}

/// One server-computed presentation of a Message's per-recipient delivery
/// truth. The browser renders this single field instead of stitching raw
/// delivery rows: every canonical state maps to exactly one label and an
/// undelivered Message is honestly "unsettled".
fn message_delivery_state(deliveries: &[&Value]) -> &'static str {
    fn status(delivery: &Value) -> &str {
        delivery["status"].as_str().unwrap_or_default()
    }
    if deliveries.is_empty() {
        return "unsettled";
    }
    if deliveries
        .iter()
        .any(|delivery| matches!(status(delivery), "failed" | "expired" | "invalidated"))
    {
        return "failed";
    }
    if deliveries
        .iter()
        .all(|delivery| status(delivery) == "acknowledged")
    {
        return "acknowledged";
    }
    if deliveries
        .iter()
        .any(|delivery| matches!(status(delivery), "claimed" | "provider_received"))
    {
        return "delivered";
    }
    "queued"
}

fn message_summary(facts: &Facts, value: &Value) -> Value {
    let message_id = &value["id"];
    let deliveries = facts
        .message_deliveries
        .iter()
        .filter(|delivery| delivery["message_id"] == *message_id)
        .collect::<Vec<_>>();
    json!({
        "message_id":message_id,
        "work_id":value["work_id"],
        "sender":role_actor_ref(facts,&value["sender_actor_ref"]),
        "recipients":value["recipients"].as_array().map(|recipients|recipients.iter().map(|recipient|role_actor_ref(facts,recipient)).collect::<Vec<_>>()).unwrap_or_default(),
        "body":value["body"],
        "kind":value["kind"],
        "correlation_id":value["correlation_id"],
        "causation_id":value["causation_id"],
        "response_intent":value["response_intent"],
        "reply_eligible":value["response_intent"] == "response_required",
        "created_at":value["created_at"],
        "delivery_state":message_delivery_state(&deliveries),
        "deliveries":deliveries.iter().map(|delivery|json!({
            "id":delivery["id"],
            "recipient_member_run_id":delivery["recipient_member_run_id"],
            "recipient_identity_id":delivery["recipient_agent_member_id"],
            "recipient_display_name":delivery["recipient_agent_member_id"].as_str().and_then(|id|facts.members.iter().find(|member|member["id"].as_str()==Some(id))).and_then(|member|member["name"].as_str()),
            "status":delivery["status"],
            "version":delivery["version"],
            "provider_receipt_id":delivery["provider_receipt_id"],
            "updated_at":delivery["updated_at"],
        })).collect::<Vec<_>>(),
    })
}

fn delivery_requires_team_reconcile(delivery: &Value, team_work_ids: &BTreeSet<&str>) -> bool {
    matches!(delivery["status"].as_str(), Some("failed" | "expired"))
        && delivery["work_id"]
            .as_str()
            .is_some_and(|id| team_work_ids.contains(id))
}

fn latest_record_ref(facts: &Facts, work_id: &str, kind: &str) -> Option<String> {
    records(facts, |value| {
        value.get("work_id").and_then(Value::as_str) == Some(work_id)
            && match kind {
                "report" => value.get("report_revision").is_some(),
                "finding" => value.get("detail_markdown").is_some(),
                "failure" => value.get("observed_failure").is_some(),
                _ => false,
            }
    })
    .into_iter()
    .max_by(|left, right| {
        left.get("report_revision")
            .and_then(Value::as_u64)
            .cmp(&right.get("report_revision").and_then(Value::as_u64))
            .then_with(|| {
                left.get("updated_at")
                    .or_else(|| left.get("created_at"))
                    .and_then(Value::as_str)
                    .cmp(
                        &right
                            .get("updated_at")
                            .or_else(|| right.get("created_at"))
                            .and_then(Value::as_str),
                    )
            })
    })
    .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
}

fn current_workspace<'a>(facts: &'a Facts, member_run_id: &str) -> Option<&'a Value> {
    facts
        .side
        .iter()
        .filter(|value| {
            value["member_run_id"] == member_run_id && value.get("canonical_root").is_some()
        })
        .max_by(|left, right| {
            left["updated_at"]
                .as_str()
                .cmp(&right["updated_at"].as_str())
                .then_with(|| left["version"].as_u64().cmp(&right["version"].as_u64()))
                .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
        })
        .filter(|value| !matches!(value["lifecycle"].as_str(), Some("archived" | "removed")))
}

/// DOC-106 responsibility projection: the assignee is one TeamMembership of
/// the accountable Team (Host/Member), or Unassigned. Legacy rows assigned
/// before the membership cutover still resolve through `owner_member_id` and
/// are honestly marked by `membership_id: null`; runtime/MemberRun state never
/// feeds this projection.
fn assignee_projection(facts: &Facts, team: &AgentTeam, work: &Work) -> (String, Value) {
    let display_name = |member_id: Option<&str>| {
        member_id.and_then(|id| {
            facts
                .members
                .iter()
                .find(|member| member["id"].as_str() == Some(id))
                .and_then(|member| member["name"].as_str())
        })
    };
    if let Some(membership_id) = work.assignee_membership_id.as_deref() {
        let membership = facts
            .team_memberships
            .iter()
            .find(|m| m["id"].as_str() == Some(membership_id));
        let member_id = membership
            .and_then(|m| m["agent_member_id"].as_str())
            .or(work.owner_member_id.as_deref());
        let role = membership
            .and_then(|m| m["role"].as_str())
            .unwrap_or("member");
        let kind = if role == "host" { "host" } else { "member" };
        return (
            kind.to_string(),
            json!({
                "kind": kind,
                "membership_id": membership_id,
                "membership_state": membership.and_then(|m| m["state"].as_str()),
                "agent_member_id": member_id,
                "display_name": display_name(member_id),
            }),
        );
    }
    if let Some(owner) = work.owner_member_id.as_deref() {
        let kind = if owner == team.host_agent_id {
            "host"
        } else {
            "member"
        };
        return (
            kind.to_string(),
            json!({
                "kind": kind,
                "membership_id": null,
                "membership_state": null,
                "agent_member_id": owner,
                "display_name": display_name(Some(owner)),
            }),
        );
    }
    (
        "unassigned".to_string(),
        json!({
            "kind": "unassigned",
            "membership_id": null,
            "membership_state": null,
            "agent_member_id": null,
            "display_name": null,
        }),
    )
}

fn work_summary(facts: &Facts, team: &AgentTeam, work: &Work) -> Value {
    let latest_event = facts
        .work_events
        .iter()
        .filter(|event| event["work_id"] == work.id)
        .max_by_key(|event| event["sequence"].as_u64().unwrap_or(0))
        .map(|event| {
            json!({
                "id":event["id"],
                "kind":event["kind"],
                "actor_ref":role_actor_ref(facts,&event["performed_by_actor"]),
                "created_at":event["created_at"],
            })
        });
    let current_run = work
        .active_member_run_id
        .as_deref()
        .and_then(|id| facts.member_runs.iter().find(|run| run["id"] == id));
    let module_refs = records(facts, |value| {
        value.get("work_id").and_then(Value::as_str) == Some(&work.id)
            && value.get("module_id").is_some()
    })
    .into_iter()
    .filter_map(|v| {
        v.get("module_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
    .collect::<Vec<_>>();
    let requirements = records(facts, |v| {
        v.get("work_id").and_then(Value::as_str) == Some(&work.id)
            && v.get("requirement_set_fingerprint").is_some()
    });
    let evaluations = records(facts, |v| {
        v.get("work_id").and_then(Value::as_str) == Some(&work.id)
            && v.get("verdict").is_some()
            && v.get("requirement_id").is_some()
    });
    let waivers = records(facts, |v| {
        v.get("work_id").and_then(Value::as_str) == Some(&work.id)
            && v.get("authority_actor").is_some()
            && v.get("requirement_id").is_some()
    });
    let current_requirements = requirements
        .iter()
        .filter(|requirement| requirement["work_revision"].as_u64() == Some(work.version))
        .collect::<Vec<_>>();
    let stale_requirements = requirements
        .len()
        .saturating_sub(current_requirements.len());
    let satisfied_evaluation = |requirement: &&Value, verdict: &str| {
        evaluations.iter().any(|evaluation| {
            evaluation["requirement_id"] == requirement["id"]
                && evaluation["work_revision"] == requirement["work_revision"]
                && evaluation["work_report_id"] == requirement["work_report_id"]
                && evaluation["candidate_fingerprint"] == requirement["candidate_fingerprint"]
                && evaluation["config_fingerprint"] == requirement["config_fingerprint"]
                && evaluation["evaluator_fingerprint"] == requirement["evaluator_fingerprint"]
                && evaluation["verdict"] == verdict
        })
    };
    let active_waiver = |requirement: &&Value| {
        waivers.iter().any(|waiver| {
            waiver["requirement_id"] == requirement["id"]
                && waiver["work_revision"] == requirement["work_revision"]
                && waiver["candidate_fingerprint"] == requirement["candidate_fingerprint"]
                && waiver["state"] == "active"
        })
    };
    let passed = current_requirements
        .iter()
        .filter(|requirement| satisfied_evaluation(requirement, "passed"))
        .count();
    let failed = current_requirements
        .iter()
        .filter(|requirement| satisfied_evaluation(requirement, "failed"))
        .count();
    let waived = current_requirements
        .iter()
        .filter(|requirement| active_waiver(requirement))
        .count();
    let pending = current_requirements
        .len()
        .saturating_sub(passed + failed + waived);
    let deliveries = facts
        .work_deliveries
        .iter()
        .filter(|d| d["work_id"] == work.id)
        .collect::<Vec<_>>();
    let count_status = |status: &str| deliveries.iter().filter(|d| d["status"] == status).count();
    let workspace = work
        .active_member_run_id
        .as_deref()
        .and_then(|id| current_workspace(facts, id));
    let incoming = facts
        .side
        .iter()
        .filter(|v| {
            v.get("target_work_ref")
                .and_then(|r| r.get("work_id"))
                .and_then(Value::as_str)
                == Some(&work.id)
        })
        .count();
    let outgoing = facts
        .side
        .iter()
        .filter(|v| {
            v.get("source_work_ref")
                .and_then(|r| r.get("work_id"))
                .and_then(Value::as_str)
                == Some(&work.id)
        })
        .count();
    let (assignee_kind, assignee_ref) = assignee_projection(facts, team, work);
    json!({
        "work_id": work.id, "work_revision": work.version, "team_id": team.id, "mission_id": team.mission_id,
        "accountable_team_id": work.accountable_team_id,
        "assignee_membership_id": work.assignee_membership_id,
        "assignee_kind": assignee_kind,
        "assignee_ref": assignee_ref,
        "migration_state": if work.accountable_team_id.is_some() { "canonical" } else { "legacy_team_run_scoped" },
        "title":work.title,
        "context_markdown":work.context_markdown,
        "completion_criteria_markdown":work.completion_criteria_markdown,
        "claim_mode":enum_string(&work.claim_mode),
        "eligible_member_ids":work.eligible_member_ids,
        "prerequisite_work_ids":work.prerequisite_work_ids,
        "parent_work_id":work.parent_work_id,
        "blocker_reason":work.blocker_reason,
        "result_summary":work.result_summary,
        "artifact_refs":work.artifact_refs,
        "check_refs":work.check_refs,
        "latest_event":latest_event,
        "owner_actor_ref": work.owner_member_id.as_ref().map(|id| json!({"kind":"agent_member","id":id})),
        "current_member_run_ref": work.active_member_run_id,
        "phase": enum_string(&work.phase), "condition": enum_string(&work.condition),
        "resolution": work.resolution.as_ref().map(enum_string), "priority": enum_string(&work.priority),
        "module_refs": module_refs,
        "gate_summary": {"required": current_requirements.len(), "passed": passed, "failed": failed, "pending": pending, "waived": waived, "stale": stale_requirements},
        "latest_report_ref": latest_record_ref(facts, &work.id, "report"),
        "latest_finding_refs": latest_record_ref(facts, &work.id, "finding").into_iter().collect::<Vec<_>>(),
        "latest_failure_ref": latest_record_ref(facts, &work.id, "failure"),
        "delivery_summary": {"queued":count_status("queued"),"claimed":count_status("claimed"),"provider_received":count_status("provider_received"),"failed":count_status("failed"),"expired":count_status("expired"),"invalidated":count_status("invalidated"),"recovery_class":if deliveries.iter().any(|d| d["status"] == "failed") {"required"} else {"none"}},
        "runtime_summary": {"state":current_run.and_then(|r|r["runtime_status"].as_str()).unwrap_or("unknown"),"generation":current_run.and_then(|r|r["runtime_generation"].as_u64()),"freshness":if current_run.is_some(){"current"}else{"unknown"}},
        "workspace_summary": {"binding_id":workspace.and_then(|v|v["id"].as_str()),"lifecycle":workspace.and_then(|v|v["lifecycle"].as_str()).unwrap_or("unavailable"),"safety":workspace.map(|v| if v["lifecycle"]=="ready"{"safe"}else{"attention"}).unwrap_or("unknown")},
        "delegation_summary":{"incoming":incoming,"outgoing":outgoing,"attention":false},
        "updated_at":work.updated_at,
    })
}

fn envelope(
    kind: &str,
    facts: &Facts,
    mut data: Value,
    attention: Vec<Value>,
    actions: Vec<Value>,
) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "runtime_fabric".into(),
            json!({
                "agent_identities": record_summaries("agent_identity", facts.agent_identities.clone()),
                "agent_sessions": record_summaries("agent_session", facts.agent_sessions.clone()),
                "team_memberships": record_summaries("team_membership", facts.team_memberships.clone()),
                "work_execution_bindings": record_summaries("work_execution_binding", facts.work_execution_bindings.clone()),
                "messages": record_summaries("message", facts.canonical_messages.clone()),
                "message_deliveries": record_summaries("canonical_message_delivery", facts.canonical_message_deliveries.clone()),
            }),
        );
    }
    json!({"view_kind":kind,"schema_version":SCHEMA_VERSION,"source_execution_space_id":facts.space_id,
        "source_store_identity":facts.store_identity,"as_of_event_sequence":facts.sequence,"generated_at":now(),
        "freshness":"current","data":data,"attention":attention,"allowed_actions":actions})
}

fn action(
    kind: &str,
    target_kind: &str,
    target_id: &str,
    version: u64,
    disabled: Option<&str>,
) -> Value {
    json!({"kind":kind,"target_ref":{"kind":target_kind,"id":target_id},"required_version":version,"disabled_reason":disabled})
}

fn member_run_has_active_provider_capability(
    provider_runtime_projections: &[Value],
    member_run: &Value,
    capability: &str,
) -> bool {
    let Some(member_run_id) = member_run["id"].as_str() else {
        return false;
    };
    let Some(runtime_generation) = member_run["runtime_generation"].as_u64() else {
        return false;
    };
    provider_runtime_projections
        .iter()
        .rev()
        .find(|projection| {
            projection["id"] == member_run_id
                && projection["runtime_generation"].as_u64() == Some(runtime_generation)
        })
        .and_then(|projection| projection["provider_profile"]["capability_bindings"].as_array())
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding["capability"] == capability
                    && binding["status"] == "verified"
                    && binding["admission"] == "active"
            })
        })
}

fn provider_core_capability_admission(profile: Option<&Value>) -> (&'static str, Option<String>) {
    const REQUIRED: [&str; 3] = ["open_or_resume", "start_cycle", "observe"];
    let Some(bindings) = profile.and_then(|profile| profile["capability_bindings"].as_array())
    else {
        return (
            "unknown",
            Some("no exact provider capability snapshot is available".to_string()),
        );
    };
    let mut review_required = Vec::new();
    let mut unavailable = Vec::new();
    for capability in REQUIRED {
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding["capability"] == capability)
        else {
            unavailable.push(capability);
            continue;
        };
        match (binding["status"].as_str(), binding["admission"].as_str()) {
            (Some("verified"), Some("active")) => {}
            (Some("review_required"), Some("pending_dependency")) => {
                review_required.push(capability)
            }
            _ => unavailable.push(capability),
        }
    }
    if !unavailable.is_empty() {
        return (
            "unavailable",
            Some(format!(
                "required provider capabilities are unavailable: {}",
                unavailable.join(", ")
            )),
        );
    }
    if !review_required.is_empty() {
        return (
            "review_required",
            Some(format!(
                "required provider capabilities still need exact live evidence: {}",
                review_required.join(", ")
            )),
        );
    }
    (
        "active",
        Some("open/resume, start-cycle, and observe bindings are active and verified".to_string()),
    )
}

fn message_fabric_disabled(
    facts: &Facts,
    store: &HarnessStore,
    team: &AgentTeam,
) -> Option<String> {
    let daemon_is_current = store
        .latest_node_daemon_lease(&team.node_id)
        .ok()
        .flatten()
        .is_some_and(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > crate::current_unix_ms_u64()
        });
    if !daemon_is_current {
        return Some(
            "canonical Message authoring requires the Team machine's current NodeDaemon".into(),
        );
    }
    let identities = team
        .member_ids
        .iter()
        .chain(std::iter::once(&team.host_agent_id))
        .collect::<BTreeSet<_>>();
    let routable = identities.iter().all(|identity_id| {
        let memberships = facts
            .team_memberships
            .iter()
            .filter(|membership| {
                membership["team_id"] == team.id
                    && membership["agent_member_id"] == identity_id.as_str()
                    && membership["node_id"] == team.node_id
                    && membership["state"] == "active"
            })
            .collect::<Vec<_>>();
        memberships.len() == 1
            && facts.message_subscriptions.iter().any(|subscription| {
                subscription["subscriber_kind"] == "agent_member"
                    && subscription["subscriber_ref"] == identity_id.as_str()
                    && subscription["membership_ref"] == memberships[0]["id"]
                    && subscription["status"] == "active"
            })
    });
    (!routable)
        .then(|| "canonical Team membership and Message subscription fabric is not ready".into())
}

fn error(status: &'static str, code: &str, detail: impl Into<String>) -> HttpResponse {
    HttpResponse {
        status,
        body: json!({"ok":false,"error":{"code":code,"message":detail.into()}}),
    }
}

pub(crate) fn handle_get(
    current: &HarnessStore,
    spaces: &[(String, HarnessStore)],
    current_space_id: &str,
    path: &str,
    target: &str,
    build_sha: &str,
    identity: Option<&ReadIdentity>,
) -> Option<HttpResponse> {
    if !path.starts_with("/v1/views/") {
        return None;
    }
    let query = match Query::parse(target) {
        Ok(value) => value,
        Err(detail) => return Some(error("400 Bad Request", "INVALID_QUERY", detail)),
    };
    let result = if path == "/v1/views/global-work" {
        global_work_view(spaces, &query)
    } else if let Some(team_id) = path.strip_prefix("/v1/views/team-workspace/") {
        team_view(
            current_space_id,
            current,
            team_id,
            false,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(team_id) = path.strip_prefix("/v1/views/host-console/") {
        team_view(
            current_space_id,
            current,
            team_id,
            true,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(team_id) = path.strip_prefix("/v1/views/team-inbox/") {
        team_inbox_view(current_space_id, current, team_id, &query, identity)
    } else if let Some(route_ref) = path.strip_prefix("/v1/views/agent-workspace/") {
        agent_workspace_view(current_space_id, current, route_ref, &query, identity)
    } else if let Some(member_run_id) = path.strip_prefix("/v1/views/member-workbench/") {
        member_view(
            current_space_id,
            current,
            member_run_id,
            identity,
            query.company.as_deref(),
        )
    } else if let Some(node_id) = path.strip_prefix("/v1/views/operator/") {
        operator_view(
            current_space_id,
            current,
            node_id,
            build_sha,
            identity,
            query.company.as_deref(),
        )
    } else {
        return Some(error(
            "404 Not Found",
            "ROLE_VIEW_NOT_FOUND",
            "unknown role view",
        ));
    };
    Some(match result {
        Ok(body) => HttpResponse {
            status: "200 OK",
            body,
        },
        Err((status, code, detail)) => error(status, code, detail),
    })
}

type ViewResult = Result<Value, (&'static str, &'static str, String)>;

/// In-process Global Work read for the CLI (`harness work list`). This is the
/// identical projection served at `/v1/views/global-work`; there is no second
/// aggregate implementation or writer.
pub(crate) fn global_work_view_json(
    spaces: &[(String, HarnessStore)],
    target: &str,
) -> Result<Value, String> {
    let query = Query::parse(target)?;
    global_work_view(spaces, &query).map_err(|(_, code, detail)| format!("{code}: {detail}"))
}

/// The one Global Work read projection (DOC-106): every canonical Work across
/// the provided Execution Space stores, keyed by durable Team/TeamMembership
/// identifiers, failing closed on cross-store Work id collisions. It never
/// writes and never folds a second ledger.
fn global_work_view(spaces: &[(String, HarnessStore)], query: &Query) -> ViewResult {
    let mut all = Vec::new();
    let mut max_sequence = 0;
    let mut identities = Vec::new();
    let mut snapshot_vector = Vec::new();
    let mut work_sources = BTreeMap::<String, String>::new();
    let mut facet_nodes = BTreeSet::new();
    let mut facet_hosts = BTreeSet::new();
    let mut facet_members = BTreeSet::new();
    let mut pending_migration = Vec::new();
    let mut ordered_spaces = spaces.iter().collect::<Vec<_>>();
    ordered_spaces.sort_by(|left, right| left.0.cmp(&right.0));
    for (space_id, store) in &ordered_spaces {
        let facts = Facts::read(space_id, store)
            .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
        max_sequence = max_sequence.max(facts.sequence);
        identities.push(facts.store_identity.clone());
        snapshot_vector.push(facts.snapshot_point());
        for work in &facts.works {
            if let Some(previous_space) = work_sources.insert(work.id.clone(), (*space_id).clone())
            {
                if previous_space != **space_id {
                    return Err((
                        "409 Conflict",
                        "WORK_ID_CONFLICT",
                        format!(
                            "Work {} exists in both {previous_space} and {space_id}",
                            work.id
                        ),
                    ));
                }
            }
            let Some(team) = work
                .accountable_team_id
                .as_deref()
                .and_then(|id| facts.teams.iter().find(|team| team.id == id))
                .or_else(|| {
                    facts
                        .runs
                        .iter()
                        .find(|run| run.id == work.team_run_id)
                        .and_then(|run| {
                            facts.teams.iter().find(|team| team.id == run.agent_team_id)
                        })
                })
            else {
                // A Work with no resolvable accountable Team is a pre-cutover
                // legacy row. It is never hidden silently: it surfaces in the
                // view's pending-migration list and skips item projection until
                // responsibility migration binds it to one durable Team.
                pending_migration.push(work.id.clone());
                continue;
            };
            let summary = work_summary(&facts, team, work);
            if !query.matches("team_id", Some(&team.id))
                || !query.matches("mission_id", Some(&team.mission_id))
                || !query.matches("node_id", Some(&team.node_id))
                || !query.matches("host_id", Some(&team.host_agent_id))
                || !query.matches("member_id", work.owner_member_id.as_deref())
                || !query.matches(
                    "assignee_membership_id",
                    work.assignee_membership_id.as_deref(),
                )
                || !query.matches("assignee_kind", summary["assignee_kind"].as_str())
                || !query.matches("phase", Some(&enum_string(&work.phase)))
                || !query.matches("condition", Some(&enum_string(&work.condition)))
                || !query.matches(
                    "resolution",
                    work.resolution.as_ref().map(enum_string).as_deref(),
                )
                || !query.matches("priority", Some(&enum_string(&work.priority)))
            {
                continue;
            }
            if !query.matches(
                "module_id",
                summary["module_refs"]
                    .as_array()
                    .and_then(|values| values.iter().find_map(Value::as_str)),
            ) && query.values.contains_key("module_id")
            {
                let wanted = &query.values["module_id"];
                if !summary["module_refs"].as_array().is_some_and(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|value| wanted.iter().any(|item| item == value))
                }) {
                    continue;
                }
            }
            let gate_states = ["passed", "failed", "pending", "waived", "stale"]
                .into_iter()
                .filter(|state| summary["gate_summary"][*state].as_u64().unwrap_or(0) > 0)
                .collect::<Vec<_>>();
            if let Some(wanted) = query.values.get("gate_state") {
                if !gate_states
                    .iter()
                    .any(|state| wanted.iter().any(|item| item == state))
                {
                    continue;
                }
            }
            let delegated = summary["delegation_summary"]["incoming"]
                .as_u64()
                .unwrap_or(0)
                > 0
                || summary["delegation_summary"]["outgoing"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0;
            if query.delegated.is_some_and(|wanted| wanted != delegated) {
                continue;
            }
            let updated = work.updated_at.as_str();
            if query
                .values
                .get("updated_after")
                .is_some_and(|values| values.iter().all(|after| updated < after.as_str()))
                || query
                    .values
                    .get("updated_before")
                    .is_some_and(|values| values.iter().all(|before| updated >= before.as_str()))
            {
                continue;
            }
            facet_nodes.insert(team.node_id.clone());
            facet_hosts.insert(team.host_agent_id.clone());
            if let Some(member) = &work.owner_member_id {
                facet_members.insert(member.clone());
            }
            all.push(summary);
        }
    }
    all.sort_by(|a, b| {
        b["updated_at"]
            .as_str()
            .cmp(&a["updated_at"].as_str())
            .then_with(|| a["work_id"].as_str().cmp(&b["work_id"].as_str()))
    });
    let stable_hash = |value: &Value| {
        serde_json::to_vec(value)
            .unwrap_or_default()
            .into_iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    };
    let query_fingerprint = stable_hash(&json!({
        "schema": SCHEMA_VERSION,
        "filters": query.values,
        "delegated": query.delegated,
        "sort": "updated_at:desc,work_id:asc",
        "limit": query.limit,
    }));
    let snapshot_fingerprint =
        stable_hash(&serde_json::to_value(&snapshot_vector).map_err(|error| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                error.to_string(),
            )
        })?);
    let offset = if let Some(cursor) = &query.cursor {
        let parts = cursor.split(':').collect::<Vec<_>>();
        if parts.len() != 4
            || parts[0] != "rv1"
            || u64::from_str_radix(parts[1], 16).ok() != Some(query_fingerprint)
            || u64::from_str_radix(parts[2], 16).ok() != Some(snapshot_fingerprint)
        {
            return Err((
                "400 Bad Request",
                "INVALID_CURSOR",
                "cursor does not belong to this filter/sort/sequence".into(),
            ));
        }
        parts[3].parse::<usize>().map_err(|_| {
            (
                "400 Bad Request",
                "INVALID_CURSOR",
                "cursor offset is invalid".into(),
            )
        })?
    } else {
        0
    };
    let page_items = all
        .iter()
        .skip(offset)
        .take(query.limit)
        .cloned()
        .collect::<Vec<_>>();
    let next = (offset + page_items.len() < all.len()).then(|| {
        format!(
            "rv1:{query_fingerprint:016x}:{snapshot_fingerprint:016x}:{}",
            offset + page_items.len()
        )
    });
    let mut after_vector = Vec::new();
    for (space_id, store) in &ordered_spaces {
        after_vector.push(
            Facts::read(space_id, store)
                .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?
                .snapshot_point(),
        );
    }
    if snapshot_vector != after_vector {
        return Err((
            "503 Service Unavailable",
            "SNAPSHOT_UNSTABLE",
            "Global Work sources changed during projection; retry the read".into(),
        ));
    }
    let facets = |field: &str| {
        let mut values = all
            .iter()
            .filter_map(|v| v.get(field).and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values
    };
    let facts = Facts {
        space_id: "global".into(),
        store_identity: identities.join("|"),
        sequence: max_sequence,
        work_sequence: 0,
        team_sequence: 0,
        run_sequence: 0,
        team_revisions: BTreeMap::new(),
        run_revisions: BTreeMap::new(),
        canonical_versions: BTreeMap::new(),
        teams: vec![],
        runs: vec![],
        works: vec![],
        members: vec![],
        member_runs: vec![],
        provider_runtime_projections: vec![],
        messages: vec![],
        message_deliveries: vec![],
        agent_identities: vec![],
        agent_sessions: vec![],
        team_memberships: vec![],
        message_subscriptions: vec![],
        work_execution_bindings: vec![],
        canonical_messages: vec![],
        canonical_message_deliveries: vec![],
        runtime_commands: vec![],
        work_deliveries: vec![],
        work_events: vec![],
        side: vec![],
        integrity_annotations: vec![],
    };
    pending_migration.sort();
    pending_migration.dedup();
    let migration_attention = if pending_migration.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "kind":"legacy_work_pending_migration",
            "severity":"warning",
            "source_ref":{"kind":"work","id":pending_migration.first().cloned().unwrap_or_default()},
            "reason_code":"work_missing_accountable_team",
            "first_seen_at":now(),
            "last_seen_at":now(),
            "recommended_action":"Run `harness team-run work migrate-responsibility` to bind legacy TeamRun-scoped Work to one durable Team; ambiguous rows fail closed for manual reconciliation",
        })]
    };
    Ok(envelope(
        "global_work",
        &facts,
        json!({"query":query.values,"sort":[{"field":"updated_at","direction":"desc"},{"field":"work_id","direction":"asc"}],"items":page_items,"page":{"as_of_event_sequence":max_sequence,"item_count":all.len(),"next_cursor":next,"snapshot_vector":snapshot_vector},"pending_migration_work_ids":pending_migration,"facets":{"teams":facets("team_id"),"missions":facets("mission_id"),"nodes":facet_nodes,"hosts":facet_hosts,"members":facet_members,"phases":facets("phase"),"conditions":facets("condition"),"resolutions":facets("resolution"),"modules":all.iter().flat_map(|v|v["module_refs"].as_array().into_iter().flatten()).filter_map(Value::as_str).collect::<BTreeSet<_>>(),"gate_states":["passed","failed","pending","waived","stale"]}}),
        migration_attention,
        vec![],
    ))
}

fn list_team_collaboration_delegations(
    store: &HarnessStore,
    company_id: &str,
    team_id: &str,
) -> Result<(u64, Vec<harness_core::collaboration::WorkDelegationV1>), String> {
    let source_filter = harness_store::CollaborationDelegationFilter {
        source_team_id: Some(team_id.to_string()),
        target_team_id: None,
        node_id: None,
        state: None,
    };
    let mut source_page = store
        .list_collaboration_delegations(company_id, &source_filter, None, 500)
        .map_err(|error| error.to_string())?;
    let as_of_store_sequence = source_page.as_of_store_sequence;
    let mut by_id = BTreeMap::new();
    loop {
        for delegation in source_page.items {
            by_id.insert(delegation.id.clone(), delegation);
        }
        let Some(cursor) = source_page.next_cursor else {
            break;
        };
        source_page = store
            .list_collaboration_delegations(company_id, &source_filter, Some(cursor), 500)
            .map_err(|error| error.to_string())?;
    }
    let target_filter = harness_store::CollaborationDelegationFilter {
        source_team_id: None,
        target_team_id: Some(team_id.to_string()),
        node_id: None,
        state: None,
    };
    let mut target_page = store
        .list_collaboration_delegations(
            company_id,
            &target_filter,
            Some(harness_store::CollaborationCursor {
                as_of_store_sequence,
                offset: 0,
            }),
            500,
        )
        .map_err(|error| error.to_string())?;
    loop {
        for delegation in target_page.items {
            by_id.insert(delegation.id.clone(), delegation);
        }
        let Some(cursor) = target_page.next_cursor else {
            break;
        };
        target_page = store
            .list_collaboration_delegations(company_id, &target_filter, Some(cursor), 500)
            .map_err(|error| error.to_string())?;
    }
    Ok((as_of_store_sequence, by_id.into_values().collect()))
}

fn collaboration_projection(
    company_id: Option<&str>,
    team_id: &str,
    member_work_ids: Option<&BTreeSet<String>>,
) -> Value {
    let Some(company_id) = company_id else {
        return json!({"state":"unavailable","reason":"Company scope is required"});
    };
    let result = (|| -> Result<Value, String> {
        let home = crate::execution_space::firm_home().map_err(|error| error.to_string())?;
        let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
            .map_err(|error| error.to_string())?;
        let root = layout
            .collaboration_root(company_id)
            .map_err(|error| error.to_string())?;
        if !root.exists() {
            return Ok(json!({
                "company_id":company_id,
                "team_id":team_id,
                "state":"unavailable",
                "reason":"Company collaboration projection is not present on this server",
            }));
        }
        let store = HarnessStore::new(root);
        let (as_of_store_sequence, all_team_delegations) =
            list_team_collaboration_delegations(&store, company_id, team_id)?;
        let mut delegations = all_team_delegations
            .into_iter()
            .filter(|delegation| {
                member_work_ids.is_none_or(|work_ids| {
                    delegation.state == harness_core::collaboration::DelegationState::Active
                        && (work_ids.contains(&delegation.source_work_ref.work_id)
                            || delegation
                                .target_work_ref
                                .as_ref()
                                .is_some_and(|target| work_ids.contains(&target.work_id)))
                })
            })
            .collect::<Vec<_>>();
        delegations.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let pending_cancellations = delegations
            .iter()
            .map(|delegation| store.collaboration_cancellation_requests(company_id, &delegation.id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
            .into_iter()
            .flatten()
            .filter(|request| {
                request.state == harness_core::collaboration::CancellationRequestState::Pending
                    && delegations
                        .iter()
                        .any(|delegation| delegation.id == request.delegation_id)
            })
            .collect::<Vec<_>>();
        let publication_count = delegations
            .iter()
            .map(|delegation| {
                store
                    .collaboration_publications(company_id, &delegation.id)
                    .map(|items| items.len())
                    .unwrap_or_default()
            })
            .sum::<usize>();
        let attention_count = delegations
            .iter()
            .filter(|delegation| {
                matches!(
                    delegation.state,
                    harness_core::collaboration::DelegationState::AwaitingTargetDecision
                        | harness_core::collaboration::DelegationState::ProvisioningTargetWork
                        | harness_core::collaboration::DelegationState::CancellationRequested
                )
            })
            .count()
            + pending_cancellations.len();
        Ok(json!({
            "company_id":company_id,
            "team_id":team_id,
            "state":"observed",
            "as_of_store_sequence":as_of_store_sequence,
            "delegation_count":delegations.len(),
            "delegations":delegations,
            "pending_cancellations":pending_cancellations,
            "publication_count":publication_count,
            "attention_count":attention_count,
        }))
    })();
    result.unwrap_or_else(|reason| {
        json!({
            "company_id":company_id,
            "team_id":team_id,
            "state":"unavailable",
            "reason":reason,
        })
    })
}

fn team_view(
    space_id: &str,
    store: &HarnessStore,
    team_id: &str,
    host: bool,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let route_run = facts.runs.iter().find(|run| run.id == team_id);
    let resolved_team_id = route_run
        .map(|run| run.agent_team_id.as_str())
        .unwrap_or(team_id);
    let team = facts
        .teams
        .iter()
        .find(|team| team.id == resolved_team_id)
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", team_id.to_string()))?;
    let exact_host_identity = identity.is_some_and(|identity| {
        (identity.actor.kind == ActorKind::AgentMember && identity.actor.id == team.host_agent_id)
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == team.host_agent_id)
    });
    let team_member_identity = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember
            && (identity.actor.id == team.host_agent_id
                || team.member_ids.contains(&identity.actor.id))
    }) || exact_host_identity;
    if (host && !exact_host_identity) || (!host && !team_member_identity) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            if host {
                "HostConsole requires this Team's exact Host authority"
            } else {
                "TeamWorkspace requires a Team-scoped AgentMember identity"
            }
            .into(),
        ));
    }
    let run = route_run.or_else(|| facts.latest_run(resolved_team_id));
    let run_id = run.map(|r| r.id.as_str());
    let works = facts
        .works
        .iter()
        .filter(|w| {
            w.accountable_team_id.as_deref() == Some(resolved_team_id)
                || run_id == Some(w.team_run_id.as_str())
        })
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let team_work_ids = works
        .iter()
        .filter_map(|work| work["work_id"].as_str())
        .collect::<BTreeSet<_>>();
    let (activity, activity_truncated) = team_activity(&facts, &team_work_ids, run_id);
    // Team creation retains the Host identity for messaging, but that does
    // not fabricate an executing member. Show the Host in member capacity
    // only when this exact TeamRun has an explicit Host MemberRun.
    let host_has_member_run = run_id.is_some_and(|selected_run_id| {
        facts.member_runs.iter().any(|member_run| {
            member_run["team_run_id"] == selected_run_id
                && member_run["agent_member_id"] == team.host_agent_id
        })
    });
    let team_member_ids = team
        .member_ids
        .iter()
        .filter(|member_id| member_id.as_str() != team.host_agent_id || host_has_member_run)
        .collect::<BTreeSet<_>>();
    let members=facts.members.iter().filter(|m|m["id"].as_str().is_some_and(|id|team_member_ids.iter().any(|member|member.as_str()==id))).map(|member|{
        let member_id=member["id"].as_str().unwrap_or_default();
        let active=facts.member_runs.iter().filter(|r|r["agent_member_id"]==member_id&&run_id.is_some_and(|id|r["team_run_id"]==id)&&r["coordination_status"]=="active").collect::<Vec<_>>();
        let current=if active.len()==1 { Some(active[0]) } else { None };
        let assigned=works.iter().filter(|work|work["owner_actor_ref"]["id"]==member_id).collect::<Vec<_>>();
        let count_phase=|phase:&str|assigned.iter().filter(|work|work["phase"]==phase).count();
        let latest_action_summary=current.and_then(|run|run["native_session"]["native_session_id"].as_str()).and_then(|session_id|facts.runtime_commands.iter().filter(|command|command["target_session_id"]==session_id).max_by(|a,b|a["updated_at"].as_str().cmp(&b["updated_at"].as_str())).map(|command|record_summary("runtime_command",command)));
        // Adapter review state is a separate fact from runtime availability:
        // an idle member on an unreviewed provider tuple is *not* Ready. The
        // trust MemberRun carries only a profile ref, so the concrete tuple is
        // joined from the runtime-layer projection of the same run.
        let runtime_profile=current.and_then(|r|facts.provider_runtime_projections.iter().filter(|projection|projection["id"]==r["id"]).max_by_key(|projection|projection["runtime_generation"].as_u64().unwrap_or_default())).map(|projection|&projection["provider_profile"]);
        let provider_compatibility=runtime_profile.and_then(|profile|profile["compatibility_status"].as_str());
        let (provider_capability_admission,provider_capability_note)=provider_core_capability_admission(runtime_profile);
        json!({
            "agent_member_ref":{"kind":"agent_member","id":member_id},
            "display_name":member["name"],
            "role":member["role"],
            "organization_status":member["organization_status"],
            "coordination_status":current.map(|r|r["coordination_status"].clone()),
            "provider":current.and_then(|r|r["native_session"]["provider"].as_str()).or_else(||current.and_then(|r|r["provider_profile_snapshot"].as_str())).or_else(||member["provider_profile_ref"].as_str()),
            "model":member["model_preference"],
            "native_session_health":current.and_then(|r|r["native_session"]["availability"].as_str()),
            "current_member_run_ref":current.and_then(|r|r["id"].as_str()),
            "runtime_state":current.and_then(|r|r["runtime_status"].as_str()),
            "runtime_generation":current.and_then(|r|r["runtime_generation"].as_u64()),
            "capacity":match current.and_then(|r|r["runtime_status"].as_str()){Some("running")|Some("queued")=>"busy",Some("idle")|Some("waiting")=>"available",_=>"unknown"},
            "provider_compatibility":provider_compatibility,
            "provider_compatibility_note":runtime_profile.and_then(|profile|profile["compatibility_note"].as_str()),
            "provider_version":runtime_profile.and_then(|profile|profile["provider_version"].as_str()),
            "provider_capability_admission":provider_capability_admission,
            "provider_capability_note":provider_capability_note,
            "active_work_count":count_phase("active"),
            "queued_work_count":count_phase("open"),
            "review_work_count":count_phase("review"),
            "blocked_work_count":assigned.iter().filter(|work|work["condition"]=="blocked").count(),
            "latest_action":latest_action_summary,
        })
    }).collect::<Vec<_>>();
    let messages = facts
        .messages
        .iter()
        .filter(|m| run_id.is_some_and(|id| m["team_run_id"] == id))
        .map(|m| message_summary(&facts, m))
        .collect::<Vec<_>>();
    let pressure_summary = json!({
        "active_turns": members.iter().filter(|member| member["runtime_state"] == "running").count(),
        "ready_members": members.iter().filter(|member| member["capacity"] == "available" && member["provider_compatibility"] == "current" && member["provider_capability_admission"] == "active").count(),
        "total_members": members.len(),
        "ready_work": works.iter().filter(|work| work["phase"] == "open" && work["condition"] == "normal").count(),
        "review_work": works.iter().filter(|work| work["phase"] == "review").count(),
        "blocked_work": works.iter().filter(|work| work["condition"] == "blocked").count(),
    });
    let identity_attention = team_member_ids
        .iter()
        .filter(|member_id| {
            facts
                .member_runs
                .iter()
                .filter(|member_run| {
                    member_run["agent_member_id"] == member_id.as_str()
                        && run_id.is_some_and(|id| member_run["team_run_id"] == id)
                        && member_run["coordination_status"] == "active"
                })
                .count()
                > 1
        })
        .map(|member_id| {
            let observed_at = now();
            json!({"kind":"identity_conflict","severity":"critical","source_ref":{"kind":"agent_member","id":member_id},"reason_code":"multiple_active_member_runs","first_seen_at":observed_at,"last_seen_at":observed_at,"recommended_action":"Host must reconcile duplicate active MemberRuns before assigning or delivering Work"})
        })
        .collect::<Vec<_>>();
    let team_member_run_ids = facts
        .member_runs
        .iter()
        .filter(|run| run_id.is_some_and(|id| run["team_run_id"] == id))
        .filter_map(|run| run["id"].as_str())
        .collect::<BTreeSet<_>>();
    let belongs_to_team_work = |value: &Value| {
        value
            .get("work_id")
            .and_then(Value::as_str)
            .is_some_and(|id| team_work_ids.contains(id))
    };
    let raw_reports = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("report_revision").is_some()
    });
    let raw_findings = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("detail_markdown").is_some()
    });
    let raw_failures = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("observed_failure").is_some()
    });
    let raw_requirements = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("requirement_set_fingerprint").is_some()
            && facts.works.iter().any(|work| {
                v["work_id"] == work.id && v["work_revision"].as_u64() == Some(work.version)
            })
    });
    let matches_current_requirement = |candidate: &Value| {
        raw_requirements.iter().any(|requirement| {
            candidate["requirement_id"] == requirement["id"]
                && candidate["work_revision"] == requirement["work_revision"]
                && candidate["candidate_fingerprint"] == requirement["candidate_fingerprint"]
        })
    };
    let raw_evaluations = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("verdict").is_some()
            && v.get("requirement_id").is_some()
            && matches_current_requirement(v)
            && raw_requirements.iter().any(|requirement| {
                v["requirement_id"] == requirement["id"]
                    && v["work_report_id"] == requirement["work_report_id"]
                    && v["config_fingerprint"] == requirement["config_fingerprint"]
                    && v["evaluator_fingerprint"] == requirement["evaluator_fingerprint"]
            })
    });
    let raw_waivers = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("authority_actor").is_some()
            && v.get("requirement_id").is_some()
            && v["state"] == "active"
            && matches_current_requirement(v)
    });
    let raw_workspace_attention = team_member_run_ids
        .iter()
        .filter_map(|member_run_id| current_workspace(&facts, member_run_id))
        .filter(|value| {
            matches!(
                value["lifecycle"].as_str(),
                Some("dirty" | "conflicted" | "missing" | "cleanup_blocked")
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let raw_delegations = facts
        .side
        .iter()
        .filter(|v| {
            v.get("source_work_ref")
                .and_then(|reference| reference.get("work_id"))
                .and_then(Value::as_str)
                .is_some_and(|id| team_work_ids.contains(id))
                || v.get("target_work_ref")
                    .and_then(|reference| reference.get("work_id"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| team_work_ids.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let reports = record_summaries("work_report", raw_reports);
    let findings = record_summaries("work_finding", raw_findings);
    let failures = record_summaries("failure_analysis", raw_failures);
    let requirements = record_summaries("gate_requirement", raw_requirements.clone());
    let evaluations = record_summaries("gate_evaluation", raw_evaluations);
    let waivers = record_summaries("gate_waiver", raw_waivers.clone());
    let workspace_attention =
        record_summaries("workspace_binding", raw_workspace_attention.clone());
    let delegations = record_summaries("work_delegation", raw_delegations);
    let team_revision = facts.team_revisions.get(&team.id).copied().ok_or((
        "409 Conflict",
        "PROJECTION_CONFLICT",
        "selected Team has no durable revision".to_string(),
    ))?;
    let collaboration = collaboration_projection(company_id, &team.id, None);
    if !host {
        let latest_run = run.map(|run| {
            let mut card = json!({"id":run.id,"status":enum_string(&run.status),"previous_run_id":run.previous_run_id,"execution_node_id":run.execution_node_id,"project_binding_id":run.project_binding_id,"execution_root":run.execution_root,"created_at":run.created_at,"completed_at":run.completed_at});
            // DOC-108 pre-cutover tolerance: when this exact TeamRun carries
            // tolerated MemberRun refs (no canonical materialization), mark
            // the run card so the frontend renders it as read-only legacy
            // context instead of a live coordination row.
            if facts.integrity_annotations.iter().any(|annotation| {
                annotation["team_run_id"] == run.id.as_str()
            }) {
                if let Some(object) = card.as_object_mut() {
                    object.insert(
                        "integrity_annotation".to_string(),
                        harness_store::PRE_CUTOVER_UNMATERIALIZED_MEMBER_RUN_ANNOTATION.into(),
                    );
                }
            }
            card
        });
        let team_integrity_annotations = facts
            .integrity_annotations
            .iter()
            .filter(|annotation| {
                annotation["team_run_id"].as_str().is_some_and(|run_id| {
                    facts
                        .runs
                        .iter()
                        .any(|run| run.id == run_id && run.agent_team_id == team.id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let data = json!({"team":{"team_id":team.id,"display_name":team.name,"team_revision":team_revision,"mission_id":team.mission_id,"host_agent_id":team.host_agent_id,"viewer_role":if exact_host_identity{"host"}else{"member"},"node_id":team.node_id,"placement_generation":run.and_then(|run|facts.run_revisions.get(&run.id).copied()),"status":enum_string(&team.status),"latest_run":latest_run},"pressure_summary":pressure_summary,"works":works,"members":members,"messages":messages,"activity":activity,"activity_truncated":activity_truncated,"reports":reports,"findings":findings,"failures":failures,"gate_requirements":requirements,"gate_evaluations":evaluations,"gate_waivers":waivers,"workspace_attention":workspace_attention,"delegation_provenance":delegations,"collaboration":collaboration,"integrity_annotations":team_integrity_annotations,"page":{"as_of_event_sequence":facts.sequence,"item_count":works.len(),"next_cursor":null}});
        return Ok(envelope(
            "team_workspace",
            &facts,
            data,
            identity_attention,
            vec![],
        ));
    }
    let by_phase = |phase: &str| {
        works
            .iter()
            .filter(|w| w["phase"] == phase)
            .cloned()
            .collect::<Vec<_>>()
    };
    let host_authorized = exact_host_identity;
    let identity_conflicted = !identity_attention.is_empty();
    let disabled =
        (!host_authorized).then_some("authenticated actor is not this Team's exact Host");
    let message_disabled = disabled
        .map(str::to_string)
        .or_else(|| message_fabric_disabled(&facts, store, team));
    let mut actions = Vec::new();
    if let Some(run_id) = run_id {
        actions.push(action("create_work", "team_run", run_id, 0, disabled));
        actions.push(action(
            "send_message",
            "team_run",
            run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "reply_message",
            "team_run",
            run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
    }
    for w in &works {
        let id = w["work_id"].as_str().unwrap_or_default();
        let Some(version) = w["work_revision"].as_u64() else {
            continue;
        };
        let phase = w["phase"].as_str().unwrap_or("unknown");
        let condition = w["condition"].as_str().unwrap_or("unknown");
        let assigned = !w["owner_actor_ref"].is_null();
        if phase == "open" && condition == "normal" && !assigned {
            actions.push(action("assign_work", "work", id, version, disabled));
        }
        if matches!(phase, "open" | "active") && assigned {
            actions.push(action("rebind_work", "work", id, version, disabled));
            actions.push(action("release_work", "work", id, version, disabled));
        }
        if phase == "review" && condition == "normal" {
            actions.push(action("request_changes", "work", id, version, disabled));
            if !w["latest_report_ref"].is_null() {
                actions.push(action(
                    "request_gate_evaluation",
                    "work",
                    id,
                    version,
                    disabled,
                ));
            }
            let gates = &w["gate_summary"];
            let gates_satisfied = gates["failed"].as_u64() == Some(0)
                && gates["pending"].as_u64() == Some(0)
                && gates["required"].as_u64()
                    == Some(
                        gates["passed"].as_u64().unwrap_or(0)
                            + gates["waived"].as_u64().unwrap_or(0),
                    );
            if !w["latest_report_ref"].is_null() && gates_satisfied {
                actions.push(action("accept_work", "work", id, version, disabled));
            }
        }
        if phase != "closed" {
            actions.push(action("cancel_work", "work", id, version, disabled));
        }
    }
    if let Some(run_id) = run_id {
        for member_run in facts
            .member_runs
            .iter()
            .filter(|value| value["team_run_id"] == run_id)
        {
            let Some(member_run_id) = member_run["id"].as_str() else {
                continue;
            };
            let Some(version) = member_run["version"].as_u64() else {
                continue;
            };
            match member_run["coordination_status"].as_str() {
                Some("active") => {
                    if member_run["runtime_status"] == "running" {
                        let interrupt_disabled = disabled.map(str::to_string).or_else(|| {
                            (!member_run_has_active_provider_capability(
                                &facts.provider_runtime_projections,
                                member_run,
                                "interrupt_current_cycle",
                            ))
                            .then(|| {
                                "the exact provider tuple has no active verified interrupt binding"
                                    .to_string()
                            })
                        });
                        actions.push(action(
                            "interrupt_member_run",
                            "member_run",
                            member_run_id,
                            version,
                            interrupt_disabled.as_deref(),
                        ));
                    }
                    actions.push(action(
                        "close_member_run",
                        "member_run",
                        member_run_id,
                        version,
                        disabled,
                    ));
                }
                Some("closed") => actions.push(action(
                    "reopen_member_run",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                )),
                _ => {}
            }
            if member_run["coordination_status"] != "retired" {
                actions.push(action(
                    "retire_member_run",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
            if member_run["coordination_status"] == "active"
                && matches!(
                    member_run["runtime_status"].as_str(),
                    Some("disconnected" | "failed" | "stopped")
                )
            {
                actions.push(action(
                    "resume_native_session",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
            let binding = facts
                .side
                .iter()
                .filter(|value| {
                    value["member_run_id"] == member_run_id && value.get("canonical_root").is_some()
                })
                .max_by_key(|value| value["version"].as_u64().unwrap_or(0));
            if let Some(binding) = binding {
                let binding_version = binding["version"].as_u64().unwrap_or(0);
                match binding["lifecycle"].as_str() {
                    Some("ready") => actions.push(action(
                        "attach_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("attached" | "dirty" | "conflicted") => actions.push(action(
                        "archive_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("cleanup_blocked") => actions.push(action(
                        "archive_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("archived") => actions.push(action(
                        "cleanup_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    _ => {}
                }
            } else {
                actions.push(action(
                    "provision_workspace",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
        }
    }
    for requirement in raw_requirements.iter() {
        let Some(requirement_id) = requirement["id"].as_str() else {
            continue;
        };
        let Some(version) = requirement["version"].as_u64() else {
            continue;
        };
        if identity.is_some_and(|identity| {
            requirement["evaluator_ref"]["kind"] == enum_string(&identity.actor.kind)
                && requirement["evaluator_ref"]["id"] == identity.actor.id
        }) {
            actions.push(action(
                "evaluate_gate",
                "gate_requirement",
                requirement_id,
                version,
                disabled,
            ));
        }
        if identity.is_some_and(|identity| !identity.authority_actors.is_empty()) {
            actions.push(action(
                "waive_gate",
                "gate_requirement",
                requirement_id,
                version,
                disabled,
            ));
        }
    }
    for waiver in raw_waivers
        .iter()
        .filter(|waiver| waiver["state"] == "active")
    {
        if let (Some(id), Some(version), Some(identity)) =
            (waiver["id"].as_str(), waiver["version"].as_u64(), identity)
        {
            let actor_matches = waiver["performed_by_actor"]["kind"]
                == enum_string(&identity.actor.kind)
                && waiver["performed_by_actor"]["id"] == identity.actor.id;
            let authority_matches = identity.authority_actors.iter().any(|authority| {
                waiver["authority_actor"]["kind"] == enum_string(&authority.kind)
                    && waiver["authority_actor"]["id"] == authority.id
            });
            if !actor_matches || !authority_matches {
                continue;
            }
            actions.push(action(
                "revoke_waiver",
                "gate_waiver",
                id,
                version,
                disabled,
            ));
        }
    }
    if identity_conflicted {
        actions.clear();
    }
    let mission = store
        .latest_missions()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .find(|mission| mission.id == team.mission_id);
    let mission_log = store
        .mission_log_tail(&team.mission_id, 20)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e.to_string()))?
        .into_iter()
        .map(|entry| json!({"id":entry.id,"revision":entry.revision,"kind":enum_string(&entry.kind),"body":entry.body,"actor":entry.actor,"created_at":entry.created_at}))
        .collect::<Vec<_>>();
    let mission_context = mission.map(|mission| json!({"id":mission.id,"title":mission.title,"objective":mission.objective,"context":mission.context,"desired_outcome":mission.desired_outcome,"status":enum_string(&mission.status),"outcome_summary":mission.outcome_summary,"created_at":mission.created_at,"updated_at":mission.updated_at,"completed_at":mission.completed_at,"log":mission_log}));
    let supervisor = run.and_then(|run| store.latest_team_supervisor_lease(&run.id).ok().flatten()).map(|lease| {
        let current = enum_string(&lease.status) == "active" && lease.expires_unix_ms > SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        json!({"team_run_id":lease.team_run_id,"supervisor_id":lease.supervisor_id,"generation":lease.generation,"current":current,"heartbeat_unix_ms":lease.heartbeat_unix_ms,"expires_unix_ms":lease.expires_unix_ms,"owner_locator":lease.owner_locator,"node_daemon_generation":lease.node_daemon_generation,"status":enum_string(&lease.status)})
    });
    let host_reply_causations = facts
        .messages
        .iter()
        .filter(|message| {
            run_id.is_some_and(|id| message["team_run_id"] == id)
                && message["sender_actor_ref"]["id"] == team.host_agent_id
        })
        .filter_map(|message| message["causation_id"].as_str())
        .collect::<BTreeSet<_>>();
    let mut host_inbox = facts
        .messages
        .iter()
        .filter(|message| {
            run_id.is_some_and(|id| message["team_run_id"] == id)
                && message["sender_actor_ref"]["id"] != team.host_agent_id
                && message["response_intent"] == "response_required"
                && !message["id"]
                    .as_str()
                    .is_some_and(|id| host_reply_causations.contains(id))
                && (message["recipients"].as_array().is_some_and(|recipients| {
                    recipients
                        .iter()
                        .any(|recipient| recipient["id"] == team.host_agent_id)
                }) || message["target_ref"]["id"] == team.id)
        })
        .map(|message| message_summary(&facts, message))
        .collect::<Vec<_>>();
    host_inbox.sort_by(|left, right| {
        right["created_at"]
            .as_str()
            .cmp(&left["created_at"].as_str())
            .then_with(|| {
                right["message_id"]
                    .as_str()
                    .cmp(&left["message_id"].as_str())
            })
    });
    host_inbox.truncate(50);
    let team_message_ids = facts
        .messages
        .iter()
        .filter(|message| run_id.is_some_and(|id| message["team_run_id"] == id))
        .filter_map(|message| message["id"].as_str())
        .collect::<BTreeSet<_>>();
    let runtime_recovery = record_summaries(
        "runtime_command",
        facts
            .runtime_commands
            .iter()
            .filter(|command| {
                command["status"] == "recovery_required"
                    && command["source_record_id"].as_str().is_some_and(|id| {
                        team_work_ids.contains(id) || team_message_ids.contains(id)
                    })
            })
            .cloned()
            .collect(),
    );
    Ok(envelope(
        "host_console",
        &facts,
        json!({"team_ref":team.id,"mission_ref":team.mission_id,"mission_context":mission_context,"team_supervisor":supervisor,"host_inbox":host_inbox,"member_runtime":members,"runtime_recovery":runtime_recovery,"pressure_summary":pressure_summary,"all_works":works,"work_queues":{"ready":works.iter().filter(|w|w["phase"]=="open"&&w["condition"]=="normal").cloned().collect::<Vec<_>>(),"unassigned":works.iter().filter(|w|w["owner_actor_ref"].is_null()).cloned().collect::<Vec<_>>(),"blocked":works.iter().filter(|w|w["condition"]=="blocked").cloned().collect::<Vec<_>>(),"review":by_phase("review"),"integration":works.iter().filter(|w|w["module_refs"].as_array().is_some_and(|a|a.iter().any(|m|m=="integration-plan"))).cloned().collect::<Vec<_>>()},"member_capacity":members,"convergence_plans":[],"reusable_findings":findings,"workspace_conflicts":record_summaries("workspace_binding",raw_workspace_attention),"provider_capacity_attention":[{"state":"not_modeled","reason":"Provider account quota is not modeled in this RoleView."}],"deliveries_requiring_reconcile":record_summaries("work_delivery",facts.work_deliveries.iter().filter(|delivery|delivery_requires_team_reconcile(delivery,&team_work_ids)).cloned().collect()),"gate_attention":requirements,"daemon_summary":{"node_id":team.node_id,"lease_status":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|enum_string(&lease.status)),"generation":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|lease.generation)},"collaboration":collaboration}),
        identity_attention,
        actions,
    ))
}

fn unavailable_session_event_projection(reason: &str) -> Value {
    json!({
        "schema_version":"agentfirm.provider_observation.v1",
        "agent_session_id":null,
        "agent_session_generation":null,
        "source_snapshot_fingerprint":null,
        "episodes":[],
        "truncated":false,
        "disabled_reason":reason,
    })
}

fn normalized_provider(provider: &str) -> &str {
    match provider {
        "codex-app" | "codex_app" | "codex_app_server" => "codex",
        "kimi-code" | "kimi_code" | "kimi_acp" => "kimi",
        "claude-code" | "claude_code" | "claude_agent_sdk" => "claude",
        value => value,
    }
}

/// Resolve one current canonical AgentSession and return its server-owned
/// NativeSessionRef. MemberRun/TeamRun values are selectors only; they never
/// become a replacement source of provider identity or filesystem authority.
/// MemberRun.runtime_generation is intentionally not an AgentSession fence:
/// Team Close/Reopen may replace the adapter generation while the machine-owned
/// AgentSession and provider-native transcript remain continuous.
fn exact_agent_session_binding<'a>(
    agent_sessions: &'a [Value],
    execution_space_id: &str,
    agent_member_id: &str,
    native_session_id: &str,
    provider: Option<&str>,
) -> Result<(&'a Value, NativeSessionRef), &'static str> {
    if native_session_id.trim().is_empty() {
        return Err("The selected provider-native Session has no exact native id.");
    }
    let expected_provider = provider.map(normalized_provider);
    let current = agent_sessions
        .iter()
        .filter(|session| session["execution_space_id"] == execution_space_id)
        .filter(|session| session["agent_member_id"] == agent_member_id)
        .filter(|session| session["lifecycle"] != "closed")
        .filter_map(|session| {
            let native = serde_json::from_value::<NativeSessionRef>(
                session.get("native_session_ref")?.clone(),
            )
            .ok()?;
            (native.native_session_id == native_session_id
                && expected_provider.is_none_or(|expected| {
                    normalized_provider(&native.provider) == expected
                        && session["provider_kind"]
                            .as_str()
                            .is_some_and(|value| normalized_provider(value) == expected)
                }))
            .then_some((session, native))
        })
        .collect::<Vec<_>>();
    match current.as_slice() {
        [(session, native)] => Ok((*session, native.clone())),
        [] => Err("No current canonical AgentSession binds this provider-native Session."),
        _ => Err("Multiple current AgentSessions ambiguously bind this provider-native Session."),
    }
}

struct SessionProjectionReadRequest<'a> {
    execution_space_id: &'a str,
    project_id: &'a str,
    team_id: &'a str,
    selected_agent_id: &'a str,
    viewer_identity_id: &'a str,
    run: Option<&'a AgentTeamRun>,
    selected_member_run: Option<&'a Value>,
}

fn read_session_event_projection(
    store: &HarnessStore,
    facts: &Facts,
    request: SessionProjectionReadRequest<'_>,
) -> Value {
    let selector = if let Some(member_run) = request.selected_member_run {
        let Some(native) = member_run
            .get("native_session")
            .filter(|value| !value.is_null())
        else {
            return unavailable_session_event_projection(
                "No provider-native Session is bound to this selected Agent run.",
            );
        };
        let Some(native_id) = native["native_session_id"].as_str() else {
            return unavailable_session_event_projection(
                "The selected provider-native Session has no exact native id.",
            );
        };
        (native_id, native["provider"].as_str())
    } else {
        let Some(run) = request.run else {
            return unavailable_session_event_projection(
                "No current TeamRun binds the selected Host Agent Session.",
            );
        };
        let Some(native_id) = run
            .host_thread_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return unavailable_session_event_projection(
                "No provider-native Session is bound to the selected Host run.",
            );
        };
        (native_id, Some(run.host_surface.as_str()))
    };
    if request
        .run
        .is_none_or(|run| run.project_binding_id != request.project_id)
    {
        return unavailable_session_event_projection(
            "The selected TeamRun belongs to another Project Binding.",
        );
    }
    let (session, native_session) = match exact_agent_session_binding(
        &facts.agent_sessions,
        request.execution_space_id,
        request.selected_agent_id,
        selector.0,
        selector.1,
    ) {
        Ok(binding) => binding,
        Err(reason) => return unavailable_session_event_projection(reason),
    };
    let node_id = session["node_id"].as_str().unwrap_or_default();
    let lease = match store.latest_node_daemon_lease(node_id) {
        Ok(Some(lease))
            if enum_string(&lease.status) == "active"
                && lease.expires_unix_ms > crate::current_unix_ms_u64()
                && session["node_daemon_id"].as_str() == Some(lease.daemon_id.as_str())
                && session["node_daemon_generation"].as_u64() == Some(lease.generation) =>
        {
            lease
        }
        _ => {
            return unavailable_session_event_projection(
                "The canonical AgentSession is not owned by the current NodeDaemon generation.",
            )
        }
    };
    crate::provider_event_api::read_historical_projection(
        crate::provider_event_api::HistoricalProjectionRequest {
            execution_space_id: request.execution_space_id,
            project_id: request.project_id,
            team_id: request.team_id,
            agent_member_id: request.selected_agent_id,
            agent_session_id: session["id"].as_str().unwrap_or_default(),
            agent_session_generation: session["runtime_generation"].as_u64().unwrap_or(0),
            node_daemon_id: &lease.daemon_id,
            node_daemon_generation: lease.generation,
            viewer_identity_id: request.viewer_identity_id,
            native_session: &native_session,
        },
    )
    .unwrap_or_else(|_| {
        unavailable_session_event_projection(
            "The server could not verify and read the bound provider-native Session.",
        )
    })
}

/// How the selected TeamRun's Host provider session is owned. A
/// harness-managed Host has an exact NodeDaemon-owned AgentSession; an
/// external interactive Host only lends its own provider thread for
/// observation and must never be presented as a managed runtime; without a
/// bound thread the Host has no provider session at all.
fn host_session_mode(run: Option<&AgentTeamRun>) -> &'static str {
    match run {
        Some(run)
            if run
                .host_thread_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty()) =>
        {
            match run.host_control_mode {
                HostControlMode::Managed => "harness_managed",
                HostControlMode::External => "external_interactive",
            }
        }
        _ => "unbound",
    }
}

/// Shared Team Inbox (DOC-106): a read-only projection over the durable
/// `team-inbox:` MessageSubscription and its Team-subject canonical
/// deliveries, joined with the immutable Messages. Delivery status, claim
/// binding, correlation, and author/Team provenance are carried for the
/// operator surface; no delivery is mutated by this read.
fn team_inbox_view(
    space_id: &str,
    store: &HarnessStore,
    team_id: &str,
    query: &Query,
    identity: Option<&ReadIdentity>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let team = facts.teams.iter().find(|team| team.id == team_id).ok_or((
        "404 Not Found",
        "TEAM_NOT_FOUND",
        team_id.to_string(),
    ))?;
    let exact_host_identity = identity.is_some_and(|identity| {
        (identity.actor.kind == ActorKind::AgentMember && identity.actor.id == team.host_agent_id)
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == team.host_agent_id)
    });
    let team_member_identity = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember
            && (identity.actor.id == team.host_agent_id
                || team.member_ids.contains(&identity.actor.id))
    }) || exact_host_identity;
    if !team_member_identity {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "TeamInbox requires a Team-scoped AgentMember identity".into(),
        ));
    }
    let inbox = crate::team_inbox_projection(store, space_id, team_id, true).map_err(|e| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            e.to_string(),
        )
    })?;
    let mut items = inbox["items"].as_array().cloned().unwrap_or_default();
    items.truncate(query.limit);
    let data = json!({
        "team": {
            "team_id": team.id,
            "display_name": team.name,
            "team_revision": facts.team_revisions.get(&team.id).copied().unwrap_or(0),
            "mission_id": team.mission_id,
            "host_agent_id": team.host_agent_id,
            "node_id": team.node_id,
            "status": enum_string(&team.status),
        },
        "subscription": inbox["subscription"],
        "items": items,
        "page": {
            "as_of_event_sequence": facts.sequence,
            "item_count": items.len(),
            "next_cursor": null,
        },
    });
    Ok(envelope("team_inbox", &facts, data, vec![], vec![]))
}

fn agent_workspace_view(
    space_id: &str,
    store: &HarnessStore,
    route_ref: &str,
    query: &Query,
    identity: Option<&ReadIdentity>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let route_member_run = facts.member_runs.iter().find(|run| run["id"] == route_ref);
    let route_run = facts
        .runs
        .iter()
        .find(|run| run.id == route_ref)
        .or_else(|| {
            route_member_run
                .and_then(|member_run| member_run["team_run_id"].as_str())
                .and_then(|id| facts.runs.iter().find(|run| run.id == id))
        });
    let resolved_team_id = route_run
        .map(|run| run.agent_team_id.as_str())
        .unwrap_or(route_ref);
    let team = facts
        .teams
        .iter()
        .find(|team| team.id == resolved_team_id)
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", route_ref.to_string()))?;
    let run = route_run.or_else(|| facts.latest_run(resolved_team_id));
    let run_id = run.map(|run| run.id.as_str());
    let selected_agent_id = query
        .values
        .get("agent_id")
        .and_then(|values| values.first())
        .map(String::as_str)
        .or_else(|| route_member_run.and_then(|run| run["agent_member_id"].as_str()))
        .unwrap_or(team.host_agent_id.as_str());
    let selected_is_host = selected_agent_id == team.host_agent_id;
    let selected_is_member = team
        .member_ids
        .iter()
        .any(|member_id| member_id == selected_agent_id);
    if !selected_is_host && !selected_is_member {
        return Err((
            "404 Not Found",
            "AGENT_NOT_IN_TEAM",
            format!(
                "AgentMember {selected_agent_id} is not part of Team {}",
                team.id
            ),
        ));
    }
    let exact_host_identity = identity.is_some_and(|identity| {
        (identity.actor.kind == ActorKind::AgentMember && identity.actor.id == team.host_agent_id)
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == team.host_agent_id)
    });
    let exact_selected_identity = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember && identity.actor.id == selected_agent_id
    });
    if !(exact_host_identity || exact_selected_identity) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "AgentWorkspace requires the exact selected AgentMember or this Team's exact Host authority"
                .into(),
        ));
    }
    if selected_is_host && !exact_host_identity {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "Host Agent Session is visible only to this Team's exact Host authority".into(),
        ));
    }
    let projection_scope = if selected_is_host {
        "host_self_private"
    } else if exact_selected_identity {
        "member_self_private"
    } else {
        "host_member_public"
    };

    // Reuse the bounded TeamWorkspace summaries; no browser-side ledger joins
    // or second Work/Message model is introduced by AgentWorkspace.
    let team_envelope = team_view(
        space_id,
        store,
        route_ref,
        false,
        identity,
        query.company.as_deref(),
    )?;
    let team_data = &team_envelope["data"];
    let all_works = team_data["works"].as_array().cloned().unwrap_or_default();
    let all_messages = team_data["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let selected_recipient_ids = facts
        .member_runs
        .iter()
        .filter(|member_run| member_run["agent_member_id"] == selected_agent_id)
        .filter_map(|member_run| member_run["id"].as_str())
        .chain(std::iter::once(selected_agent_id))
        .collect::<BTreeSet<_>>();
    let mut messages = all_messages
        .into_iter()
        .filter(|message| {
            message["sender"]["id"]
                .as_str()
                .is_some_and(|id| selected_recipient_ids.contains(id))
                || message["recipients"].as_array().is_some_and(|recipients| {
                    recipients.iter().any(|recipient| {
                        recipient["id"]
                            .as_str()
                            .is_some_and(|id| selected_recipient_ids.contains(id))
                    })
                })
        })
        .collect::<Vec<_>>();
    let mut works = all_works
        .into_iter()
        .filter(|work| {
            selected_is_host
                || work["owner_actor_ref"]["id"] == selected_agent_id
                || work["eligible_member_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == selected_agent_id))
        })
        .collect::<Vec<_>>();
    let public_unread_count = messages
        .iter()
        .filter(|message| {
            message["deliveries"].as_array().is_some_and(|deliveries| {
                deliveries.iter().any(|delivery| {
                    matches!(delivery["status"].as_str(), Some("queued" | "delivered"))
                })
            })
        })
        .count();
    if projection_scope == "host_member_public" {
        // Coordination content and responsibility are public to the exact Host,
        // but delivery receipts, runtime bindings, and workspace bindings are
        // execution-private. Redact them before the RoleView leaves the server.
        for message in &mut messages {
            message["deliveries"] = json!([]);
        }
        for work in &mut works {
            work["current_member_run_ref"] = Value::Null;
            work["runtime_summary"] = json!({
                "state":"not_projected",
                "generation":null,
                "freshness":"unknown",
            });
            work["workspace_summary"] = json!({
                "binding_id":null,
                "lifecycle":"not_projected",
                "safety":"unknown",
            });
        }
    }
    let selected_work_ids = works
        .iter()
        .filter_map(|work| work["work_id"].as_str())
        .collect::<BTreeSet<_>>();

    let mut roster = team_data["members"].as_array().cloned().unwrap_or_default();
    roster.retain(|member| member["agent_member_ref"]["id"] != team.host_agent_id);
    let host_member = facts
        .members
        .iter()
        .find(|member| member["id"] == team.host_agent_id);
    roster.insert(
        0,
        json!({
            "agent_member_ref":{"kind":"agent_member","id":team.host_agent_id},
            "display_name":host_member.and_then(|member|member["name"].as_str()).unwrap_or("Host Agent"),
            "role":host_member.and_then(|member|member["role"].as_str()).unwrap_or("Host"),
            "organization_status":host_member.and_then(|member|member["organization_status"].as_str()).unwrap_or("active"),
            "coordination_status":run.map(|run|enum_string(&run.status)),
            "provider":run.map(|run|run.host_surface.clone()),
            "model":null,
            "native_session_health":if run.and_then(|run|run.host_thread_id.as_ref()).is_some(){"available"}else{"unknown"},
            "host_session_mode":host_session_mode(run),
            "current_member_run_ref":null,
            "runtime_state":run.map(|run|enum_string(&run.status)),
            "runtime_generation":null,
            "capacity":"unknown",
            "active_work_count":0,
            "queued_work_count":0,
            "review_work_count":0,
            "blocked_work_count":0,
            "latest_action":null,
            "is_host":true,
        }),
    );
    for member in &mut roster {
        if let Some(object) = member.as_object_mut() {
            object.entry("is_host").or_insert(json!(false));
            let is_selected = object
                .get("agent_member_ref")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(selected_agent_id);
            for key in [
                "provider",
                "model",
                "native_session_health",
                "current_member_run_ref",
                "runtime_generation",
                "latest_action",
            ] {
                object.remove(key);
            }
            if !is_selected || projection_scope == "host_member_public" {
                object.remove("runtime_state");
            }
            if projection_scope == "host_member_public" {
                // The public Host-selected surface is responsibility and
                // coordination only. Provider-derived or Member-private live
                // state is structurally absent, including roster rollups.
                object.remove("coordination_status");
                object.insert("coordination_status".into(), Value::Null);
                object.insert("capacity".into(), json!("not_projected"));
            }
        }
    }

    let mut member_runs = facts
        .member_runs
        .iter()
        .filter(|member_run| member_run["agent_member_id"] == selected_agent_id)
        .filter(|member_run| {
            member_run["team_run_id"].as_str().is_some_and(|candidate| {
                facts.runs.iter().any(|candidate_run| {
                    candidate_run.id == candidate && candidate_run.agent_team_id == team.id
                })
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    member_runs.sort_by(|left, right| {
        right["started_at"]
            .as_str()
            .cmp(&left["started_at"].as_str())
            .then_with(|| {
                right["runtime_generation"]
                    .as_u64()
                    .cmp(&left["runtime_generation"].as_u64())
            })
    });
    let selected_member_run = if selected_is_host {
        None
    } else {
        run_id
            .and_then(|current_run_id| {
                member_runs
                    .iter()
                    .find(|member_run| member_run["team_run_id"] == current_run_id)
            })
            .or_else(|| member_runs.first())
    };
    // Provider-private Session data is owner-bound, not merely Team-authorized.
    // The exact Host can read the Host Session. The exact Member can read that
    // Member's Session. Host authority selecting a Member receives only public
    // coordination/Work facts and never that Member's native Session internals.
    let may_read_private_session =
        (selected_is_host && exact_host_identity) || (!selected_is_host && exact_selected_identity);
    // The owner-only historical projection is decoded on demand. It is
    // independent from the volatile live overlay and never enters a ledger.
    let viewer_identity_id = identity
        .map(|identity| identity.actor.id.as_str())
        .unwrap_or_default();
    let session_event_projection = may_read_private_session.then(|| {
        let project_binding_id = store
            .provider_compatibility_scope()
            .map(|(project_id, _)| project_id)
            .unwrap_or_default();
        read_session_event_projection(
            store,
            &facts,
            SessionProjectionReadRequest {
                execution_space_id: space_id,
                project_id: project_binding_id,
                team_id: &team.id,
                selected_agent_id,
                viewer_identity_id,
                run,
                selected_member_run,
            },
        )
    });
    // Only an exact MemberRun selector plus its current canonical AgentSession
    // can receive the process-local live overlay. MemberRun and AgentSession
    // generations are independent fences; Host runs without a MemberRun stay null.
    let live_provider_activity = if may_read_private_session {
        let project_binding_id = store
            .provider_compatibility_scope()
            .map(|(project_id, _)| project_id)
            .unwrap_or_default();
        selected_member_run
            .and_then(|member_run| {
                let typed_member = serde_json::from_value(member_run.clone()).ok()?;
                crate::provider_event_api::exact_live_scope(
                    store,
                    space_id,
                    project_binding_id,
                    member_run["team_run_id"].as_str()?,
                    &typed_member,
                )
                .ok()
            })
            .as_ref()
            .and_then(crate::provider_event_api::live_snapshot)
    } else {
        None
    };
    let selected_member = facts
        .members
        .iter()
        .find(|member| member["id"] == selected_agent_id);
    let selected_roster = roster
        .iter()
        .find(|member| member["agent_member_ref"]["id"] == selected_agent_id);
    let selected_member_run_id = selected_member_run.and_then(|run| run["id"].as_str());
    let workspace_binding = selected_member_run_id
        .and_then(|member_run_id| current_workspace(&facts, member_run_id))
        .map(|workspace| record_summary("workspace_binding", workspace));
    let configuration = json!({
        "description":selected_member.and_then(|member|member["description"].as_str()),
        "prompt_ref":null,
        "prompt_projection":"not_modeled",
        "skill_refs":selected_member.and_then(|member|member["skill_refs"].as_array()).cloned().unwrap_or_default(),
        "capabilities":selected_member.and_then(|member|member["capabilities"].as_array()).cloned().unwrap_or_default(),
        "tool_refs":[],
        "tools_projection":"not_modeled_by_agent_member",
        "provider_profile_ref":if may_read_private_session {selected_member.and_then(|member|member["provider_profile_ref"].as_str())} else {None},
        "model_preference":if may_read_private_session {selected_member.and_then(|member|member["model_preference"].as_str())} else {None},
        "workspace_policy":if may_read_private_session {selected_member.and_then(|member|member["workspace_policy"].as_str())} else {None},
        "permission_ceiling":if may_read_private_session {selected_member.and_then(|member|member["permission_ceiling"].as_str())} else {None},
        "forbidden_actions":[],
        "forbidden_actions_projection":"not_modeled",
        "workspace_binding":if may_read_private_session {workspace_binding} else {None},
    });

    let authority_envelope = if exact_host_identity {
        team_view(
            space_id,
            store,
            route_ref,
            true,
            identity,
            query.company.as_deref(),
        )?
    } else if let Some(member_run_id) = selected_member_run_id {
        member_view(
            space_id,
            store,
            member_run_id,
            identity,
            query.company.as_deref(),
        )?
    } else {
        json!({"allowed_actions":[]})
    };
    let allowed_actions = authority_envelope["allowed_actions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|action| match action["target_ref"]["kind"].as_str() {
            Some("team_run") => true,
            // Host control authority and provider-private observation are
            // separate planes. A Host-selected public projection may expose
            // exact, server-authorized MemberRun controls without exposing the
            // Member's Session, runtime facts, or workspace binding.
            Some("member_run") => {
                selected_member_run_id.is_some_and(|id| action["target_ref"]["id"] == id)
            }
            Some("work") => action["target_ref"]["id"]
                .as_str()
                .is_some_and(|id| selected_work_ids.contains(id)),
            _ => false,
        })
        .collect::<Vec<_>>();

    let selected_runtime_status = selected_member_run
        .and_then(|member_run| member_run["runtime_status"].as_str().map(str::to_owned))
        .or_else(|| run.map(|run| enum_string(&run.status)));
    let selected = json!({
        "agent_member_ref":{"kind":"agent_member","id":selected_agent_id},
        "display_name":selected_member.and_then(|member|member["name"].as_str()).or_else(||selected_roster.and_then(|member|member["display_name"].as_str())).unwrap_or(if selected_is_host{"Host Agent"}else{"Agent"}),
        "role":selected_member.and_then(|member|member["role"].as_str()).or_else(||selected_roster.and_then(|member|member["role"].as_str())).unwrap_or(if selected_is_host{"Host"}else{"Agent"}),
        "organization_status":selected_member.and_then(|member|member["organization_status"].as_str()).unwrap_or("unknown"),
        "is_host":selected_is_host,
        "current_member_run_ref":if may_read_private_session {selected_member_run_id} else {None},
        "provider":if may_read_private_session {selected_member_run.and_then(|run|run["provider"].as_str())} else {None},
        "execution_mode":if may_read_private_session {selected_member_run.and_then(|run|run["execution_mode"].as_str())} else {None},
        "runtime_status":if may_read_private_session {selected_runtime_status} else {None},
        "runtime_generation":if may_read_private_session {selected_member_run.and_then(|run|run["runtime_generation"].as_u64())} else {None},
        "host_session_mode":if selected_is_host {Some(host_session_mode(run))} else {None},
    });
    let unread_count = if projection_scope == "host_member_public" {
        public_unread_count
    } else {
        messages
            .iter()
            .filter(|message| {
                message["deliveries"].as_array().is_some_and(|deliveries| {
                    deliveries.iter().any(|delivery| {
                        matches!(delivery["status"].as_str(), Some("queued" | "delivered"))
                    })
                })
            })
            .count()
    };
    let safe_team = json!({
        "team_id":team_data["team"]["team_id"],
        "display_name":team_data["team"]["display_name"],
        "team_revision":team_data["team"]["team_revision"],
        "mission_id":team_data["team"]["mission_id"],
        "host_agent_id":team_data["team"]["host_agent_id"],
        "viewer_role":team_data["team"]["viewer_role"],
        "status":team_data["team"]["status"],
        "latest_run_id":team_data["team"]["latest_run"]["id"],
    });
    let current_work_id = works
        .iter()
        .filter(|work| selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id)
        .find(|work| work["phase"] == "active")
        .or_else(|| {
            works
                .iter()
                .filter(|work| {
                    selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id
                })
                .find(|work| work["phase"] == "review")
        })
        .or_else(|| {
            works
                .iter()
                .filter(|work| {
                    selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id
                })
                .find(|work| work["phase"] == "open")
        })
        .and_then(|work| work["work_id"].as_str());
    let mut response = envelope(
        "agent_workspace",
        &facts,
        json!({
            "projection_scope":projection_scope,
            "team":safe_team,
            "selected_agent":selected,
            "roster":roster,
            "session_event_projection":session_event_projection,
            "live_provider_activity":live_provider_activity,
            "messages":messages,
            "works":works,
            "configuration":configuration,
            "context_summary":{
                "current_work_id":current_work_id,
                "message_count":messages.len(),
                "unread_count":unread_count,
                "last_activity_at":selected_member_run.and_then(|member_run|member_run["last_event_at"].as_str()),
                "authorization_count":allowed_actions.iter().filter(|action|action["disabled_reason"].is_null()).count(),
            },
        }),
        vec![],
        allowed_actions,
    );
    response["data"]
        .as_object_mut()
        .expect("AgentWorkspace data object")
        .remove("runtime_fabric");
    if projection_scope == "host_member_public" {
        let data = response["data"]
            .as_object_mut()
            .expect("AgentWorkspace data object");
        data.remove("session_event_projection");
        data.remove("live_provider_activity");
    }
    Ok(response)
}

fn member_view(
    space_id: &str,
    store: &HarnessStore,
    member_run_id: &str,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let run = facts
        .member_runs
        .iter()
        .find(|r| r["id"] == member_run_id)
        .ok_or((
            "404 Not Found",
            "MEMBER_RUN_NOT_FOUND",
            member_run_id.to_string(),
        ))?;
    let member_id = run["agent_member_id"].as_str().unwrap_or_default();
    if !identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember && identity.actor.id == member_id
    }) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "MemberWorkbench is visible only to its authenticated AgentMember".into(),
        ));
    }
    let member = facts
        .members
        .iter()
        .find(|m| m["id"] == member_id)
        .cloned()
        .ok_or((
            "404 Not Found",
            "AGENT_MEMBER_NOT_FOUND",
            member_id.to_string(),
        ))?;
    let team_run_id = run["team_run_id"].as_str().unwrap_or_default();
    let active_generations = facts
        .member_runs
        .iter()
        .filter(|candidate| {
            candidate["agent_member_id"] == member_id
                && candidate["team_run_id"] == team_run_id
                && candidate["coordination_status"] == "active"
        })
        .count();
    if active_generations > 1 {
        return Err((
            "409 Conflict",
            "IDENTITY_CONFLICT",
            format!(
                "AgentMember {member_id} has {active_generations} active MemberRuns in TeamRun {team_run_id}"
            ),
        ));
    }
    let team = facts
        .runs
        .iter()
        .find(|r| r.id == team_run_id)
        .and_then(|r| facts.teams.iter().find(|t| t.id == r.agent_team_id))
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", team_run_id.to_string()))?;
    // DOC-106: Member responsibility follows the assignee TeamMembership, not
    // a MemberRun or runtime. Legacy rows still resolve through the mirrored
    // owner identity until responsibility migration binds their membership.
    let my_membership_ids = facts
        .team_memberships
        .iter()
        .filter(|membership| {
            membership["agent_member_id"].as_str() == Some(member_id)
                && membership["team_id"].as_str() == Some(team.id.as_str())
        })
        .filter_map(|membership| membership["id"].as_str())
        .collect::<BTreeSet<_>>();
    let in_team_scope = |work: &&Work| {
        work.accountable_team_id.as_deref() == Some(team.id.as_str())
            || work.team_run_id == team_run_id
    };
    let assigned_to_member = |work: &&Work| {
        work.assignee_membership_id
            .as_deref()
            .is_some_and(|id| my_membership_ids.contains(id))
            || work.owner_member_id.as_deref() == Some(member_id)
    };
    let team_work_ids = facts
        .works
        .iter()
        .filter(|work| in_team_scope(work))
        .map(|work| work.id.as_str())
        .collect::<BTreeSet<_>>();
    let my = facts
        .works
        .iter()
        .filter(|w| in_team_scope(w) && assigned_to_member(w))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let member_work_ids = facts
        .works
        .iter()
        .filter(|work| in_team_scope(work) && assigned_to_member(work))
        .map(|work| work.id.clone())
        .collect::<BTreeSet<_>>();
    let collaboration = collaboration_projection(company_id, &team.id, Some(&member_work_ids));
    let pool = facts
        .works
        .iter()
        .filter(|w| {
            in_team_scope(w)
                && w.phase == WorkPhase::Open
                && w.condition == WorkCondition::Normal
                && (w.eligible_member_ids.is_empty()
                    || w.eligible_member_ids.iter().any(|id| id == member_id))
        })
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let queued = facts
        .message_deliveries
        .iter()
        .filter(|d| d["recipient_agent_member_id"] == member_id && d["status"] == "queued")
        .cloned()
        .collect::<Vec<_>>();
    let message_ids = queued
        .iter()
        .filter_map(|d| d["message_id"].as_str())
        .collect::<BTreeSet<_>>();
    let unread = facts
        .messages
        .iter()
        .filter(|m| m["id"].as_str().is_some_and(|id| message_ids.contains(id)))
        .map(|message| message_summary(&facts, message))
        .collect::<Vec<_>>();
    let workspace = current_workspace(&facts, member_run_id).cloned();
    let mut actions = Vec::new();
    let addressed_generation_is_current =
        run["coordination_status"] == "active" && active_generations == 1;
    let team_revision = facts.team_revisions.get(&team.id).copied().unwrap_or(0);
    if addressed_generation_is_current {
        let message_disabled = message_fabric_disabled(&facts, store, team);
        actions.push(action(
            "send_message",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "reply_message",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "request_decision",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
    }
    for w in &my {
        if !addressed_generation_is_current {
            break;
        }
        let id = w["work_id"].as_str().unwrap_or_default();
        let Some(version) = w["work_revision"].as_u64() else {
            continue;
        };
        let phase = w["phase"].as_str().unwrap_or("unknown");
        let condition = w["condition"].as_str().unwrap_or("unknown");
        if phase == "open" && condition == "normal" {
            actions.push(action("start_work", "work", id, version, None));
        } else if phase == "active" && condition == "normal" {
            actions.push(action("block_work", "work", id, version, None));
            actions.push(action("submit_work", "work", id, version, None));
            if facts
                .works
                .iter()
                .find(|work| work.id == id)
                .is_some_and(|work| work.blocker_reason.is_some())
            {
                actions.push(action("revise_work", "work", id, version, None));
            }
            actions.push(action("write_report", "work", id, version, None));
            actions.push(action("write_finding", "work", id, version, None));
            actions.push(action("write_failure", "work", id, version, None));
        } else if phase == "active" && condition == "blocked" {
            actions.push(action("unblock_work", "work", id, version, None));
            actions.push(action("write_report", "work", id, version, None));
            actions.push(action("write_finding", "work", id, version, None));
            actions.push(action("write_failure", "work", id, version, None));
        }
    }
    for w in &pool {
        if !addressed_generation_is_current {
            break;
        }
        actions.push(action(
            "claim_work",
            "work",
            w["work_id"].as_str().unwrap_or_default(),
            w["work_revision"]
                .as_u64()
                .expect("Work summary carries a durable revision"),
            None,
        ));
    }
    for requirement in records(&facts, |value| {
        value.get("requirement_set_fingerprint").is_some()
            && value["evaluator_ref"]["kind"] == "agent_member"
            && value["evaluator_ref"]["id"] == member_id
    }) {
        if let (Some(id), Some(version)) =
            (requirement["id"].as_str(), requirement["version"].as_u64())
        {
            actions.push(action(
                "evaluate_gate",
                "gate_requirement",
                id,
                version,
                None,
            ));
        }
    }
    Ok(envelope(
        "member_workbench",
        &facts,
        json!({"agent_member":agent_member_summary(&member),"member_run":member_run_summary(run),"my_works":my,"eligible_ready_pool":pool,"unread_messages":unread,"queued_deliveries":record_summaries("message_delivery",queued),"workspace_binding":workspace.as_ref().map(|value|record_summary("workspace_binding",value)),"native_session_health":run["native_session"].get("availability").cloned().unwrap_or(json!("unknown")),"report_history":record_summaries("work_report",records(&facts,|v|v["authored_by"]["id"]==member_id&&v.get("report_revision").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"finding_history":record_summaries("work_finding",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("detail_markdown").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"failure_history":record_summaries("failure_analysis",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("observed_failure").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"gate_requirements":record_summaries("gate_requirement",records(&facts,|v|v.get("requirement_set_fingerprint").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id))&&facts.works.iter().any(|work|v["work_id"]==work.id&&v["work_revision"].as_u64()==Some(work.version)))),"collaboration":collaboration}),
        vec![],
        actions,
    ))
}

fn operator_view(
    space_id: &str,
    store: &HarnessStore,
    node_id: &str,
    build_sha: &str,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let node = store
        .latest_execution_nodes()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .find(|n| n.id == node_id)
        .ok_or(("404 Not Found", "NODE_NOT_FOUND", node_id.to_string()))?;
    let operator_authorized = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::Service && identity.actor.id == node_id
    });
    if !operator_authorized {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "OperatorView requires an exact machine-scoped Service authority".into(),
        ));
    }
    let lease = store.latest_node_daemon_lease(node_id).map_err(|e| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            e.to_string(),
        )
    })?;
    let node_revision = store
        .execution_nodes()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .filter(|candidate| candidate.id == node_id)
        .count() as u64;
    let node_run_ids = facts
        .runs
        .iter()
        .filter(|run| run.execution_node_id == node_id)
        .map(|run| run.id.as_str())
        .collect::<BTreeSet<_>>();
    let node_member_run_ids = facts
        .member_runs
        .iter()
        .filter(|run| {
            run["team_run_id"]
                .as_str()
                .is_some_and(|id| node_run_ids.contains(id))
        })
        .filter_map(|run| run["id"].as_str())
        .collect::<BTreeSet<_>>();
    let message_backlog = facts
        .message_deliveries
        .iter()
        .filter(|delivery| delivery["target_node_id"] == node_id)
        .filter(|d| {
            matches!(
                d["status"].as_str(),
                Some("queued" | "claimed" | "failed" | "expired")
            )
        })
        .count();
    let work_backlog = facts
        .work_deliveries
        .iter()
        .filter(|delivery| {
            delivery["recipient_member_run_id"]
                .as_str()
                .is_some_and(|id| node_member_run_ids.contains(id))
        })
        .filter(|delivery| {
            matches!(
                delivery["status"].as_str(),
                Some("queued" | "claimed" | "failed" | "expired")
            )
        })
        .count();
    let backlog = message_backlog + work_backlog;
    let runtime_recovery = facts
        .runtime_commands
        .iter()
        .filter(|command| {
            command["target_node_id"] == node_id
                && command["status"] == "recovery_required"
                && command["effect_certainty"] == "unknown"
        })
        .map(|command| {
            let mut projected = command.clone();
            projected["summary"] = json!(format!(
                "command={} effect_certainty={} session={} generation={} failure={}",
                command["command"].as_str().unwrap_or("unknown"),
                command["effect_certainty"].as_str().unwrap_or("unknown"),
                command["target_session_id"].as_str().unwrap_or("none"),
                command["target_session_generation"].as_u64().unwrap_or(0),
                command["failure_code"].as_str().unwrap_or("unclassified"),
            ));
            projected
        })
        .collect::<Vec<_>>();
    let mut operator_actions = facts
        .work_deliveries
        .iter()
        .filter(|delivery| {
            delivery["status"] == "claimed"
                && delivery["recipient_member_run_id"]
                    .as_str()
                    .is_some_and(|id| node_member_run_ids.contains(id))
        })
        .filter_map(|delivery| {
            let delivery_id = delivery["id"].as_str()?;
            Some(action(
                "reconcile_delivery",
                "work_delivery",
                delivery_id,
                *facts
                    .canonical_versions
                    .get(&("work_delivery".into(), delivery_id.into()))?,
                None,
            ))
        })
        .collect::<Vec<_>>();
    for delivery in facts
        .message_deliveries
        .iter()
        .filter(|delivery| delivery["status"] == "claimed" && delivery["target_node_id"] == node_id)
    {
        if let (Some(id), Some(version)) = (delivery["id"].as_str(), delivery["version"].as_u64()) {
            operator_actions.push(action(
                "reconcile_message_delivery",
                "canonical_message_delivery",
                id,
                version,
                None,
            ));
        }
    }
    for command in &runtime_recovery {
        if let (Some(id), Some(version)) = (command["id"].as_str(), command["version"].as_u64()) {
            operator_actions.push(action(
                "resolve_runtime_recovery",
                "runtime_command",
                id,
                version,
                None,
            ));
        }
    }
    operator_actions.push(action(
        "diagnose",
        "execution_node",
        node_id,
        node_revision,
        None,
    ));
    let firm_home = crate::execution_space::firm_home().ok();
    let daemon_live = firm_home.as_ref().is_some_and(|home| {
        crate::supervisor_daemon::daemon_status_via_socket(home, node_id).is_some()
    });
    let local_machine_proven =
        crate::read_local_node_id().ok().as_deref() == Some(node_id) && firm_home.is_some();
    let mut daemon_action = action(
        if daemon_live {
            "stop_daemon"
        } else {
            "start_daemon"
        },
        "execution_node",
        node_id,
        node_revision,
        (!local_machine_proven)
            .then_some("this serve process cannot prove exact local Node lifecycle ownership"),
    );
    daemon_action["authority_generation"] =
        json!(lease.as_ref().map(|lease| lease.generation).unwrap_or(0));
    operator_actions.push(daemon_action);
    for (provider, execution_mode) in crate::role_actions_api::OPERATOR_PROVIDER_ADMISSION_TUPLES {
        let binding = crate::role_actions_api::provider_admission_action_binding(
            store,
            space_id,
            node_id,
            node_revision,
            provider,
            execution_mode,
        );
        let disabled_reason = (!local_machine_proven)
            .then_some("this serve process cannot prove exact local Node admission ownership")
            .map(str::to_string)
            .or_else(|| binding.disabled_reason.clone());
        let mut admission_action = action(
            "admit_provider",
            "execution_node",
            node_id,
            node_revision,
            disabled_reason.as_deref(),
        );
        admission_action["intent_binding"] =
            serde_json::to_value(binding).expect("provider admission action binding serializes");
        operator_actions.push(admission_action);
    }
    let remote_fabric = company_id.map(|company_id| {
        let result = (|| -> Result<Value, String> {
            let home = crate::execution_space::firm_home().map_err(|error| error.to_string())?;
            let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
                .map_err(|error| error.to_string())?;
            let root = layout
                .node_local_root(company_id, node_id)
                .map_err(|error| error.to_string())?;
            if !root.exists() {
                return Ok(json!({
                    "company_id":company_id,
                    "node_id":node_id,
                    "state":"unavailable",
                    "reason":"no Node-local Remote Fabric journal exists",
                }));
            }
            let local = layout
                .open_node_local(company_id, node_id)
                .map_err(|error| error.to_string())?;
            let snapshot = local.snapshot().map_err(|error| error.to_string())?;
            let queued = snapshot
                .outboxes
                .values()
                .filter(|outbox| {
                    !matches!(
                        outbox.local_state,
                        harness_fabric::LocalOutboxState::Terminal
                    )
                })
                .count();
            let recovery_required = snapshot
                .inboxes
                .values()
                .filter(|inbox| inbox.state == harness_fabric::LocalInboxState::RecoveryRequired)
                .map(|inbox| inbox.operation_id.clone())
                .collect::<Vec<_>>();
            let now = crate::current_unix_ms_u64();
            let oldest_outbox_age_ms = snapshot
                .outboxes
                .values()
                .filter(|outbox| {
                    !matches!(
                        outbox.local_state,
                        harness_fabric::LocalOutboxState::Terminal
                    )
                })
                .filter_map(|outbox| outbox.operation.as_ref())
                .map(|operation| now.saturating_sub(operation.created_at_unix_ms))
                .max()
                .unwrap_or_default();
            let control_plane_diagnostics = layout
                .control_plane_root(company_id)
                .ok()
                .filter(|root| root.exists())
                .and_then(|_| layout.open_control_plane(company_id).ok())
                .and_then(|control_store| {
                    harness_fabric::diagnostics::inspect_fabric(&control_store, company_id, now)
                        .ok()
                });
            let control_plane_online = control_plane_diagnostics
                .as_ref()
                .map(|diagnostics| diagnostics.control_plane_online);
            let control_plane_metrics =
                control_plane_diagnostics.as_ref().and_then(|diagnostics| {
                    diagnostics
                        .nodes
                        .iter()
                        .find(|diagnostic| diagnostic.node_id == node_id)
                        .cloned()
                });
            let collaboration = layout
                .collaboration_root(company_id)
                .ok()
                .filter(|root| root.exists())
                .and_then(|root| {
                    HarnessStore::new(root)
                        .list_collaboration_delegations(
                            company_id,
                            &harness_store::CollaborationDelegationFilter {
                                source_team_id: None,
                                target_team_id: None,
                                node_id: Some(node_id.into()),
                                state: None,
                            },
                            None,
                            200,
                        )
                        .ok()
                        .map(|page| {
                            let attention = page
                                .items
                                .iter()
                                .filter(|delegation| {
                                    matches!(
                                        delegation.state,
                                        harness_core::collaboration::DelegationState::AwaitingTargetDecision
                                            | harness_core::collaboration::DelegationState::ProvisioningTargetWork
                                            | harness_core::collaboration::DelegationState::CancellationRequested
                                    )
                                })
                                .count();
                            json!({
                                "state":"observed",
                                "delegation_count":page.items.len(),
                                "attention_count":attention,
                                "as_of_store_sequence":page.as_of_store_sequence,
                            })
                        })
                })
                .unwrap_or_else(|| {
                    json!({
                        "state":"unavailable",
                        "reason":"Company collaboration projection is not present on this server",
                    })
                });
            let (state, reason) = match (control_plane_online, control_plane_metrics.as_ref()) {
                (Some(true), Some(_)) => ("observed", None),
                (Some(false), _) => (
                    "offline",
                    Some("Company Control Plane lease is offline or expired"),
                ),
                (Some(true), None) => (
                    "unknown",
                    Some("Control Plane has no projection for this Node"),
                ),
                (None, _) => (
                    "unknown",
                    Some(
                        "Control Plane metrics are unavailable; local journal is not health truth",
                    ),
                ),
            };
            Ok(json!({
                "company_id":company_id,
                "node_id":node_id,
                "state":state,
                "reason":reason,
                "gateway_session":snapshot.active_session,
                "outbox_depth":queued,
                "oldest_outbox_age_ms":oldest_outbox_age_ms,
                "inbox_depth":snapshot.inboxes.len(),
                "recovery_required":recovery_required,
                "control_plane_online":control_plane_online,
                "control_plane_metrics":control_plane_metrics,
                "collaboration":collaboration,
                "store_revision":snapshot.revision,
            }))
        })();
        result.unwrap_or_else(|error| {
            json!({
                "company_id":company_id,
                "node_id":node_id,
                "state":"unavailable",
                "reason":error,
            })
        })
    });
    Ok(envelope(
        "operator",
        &facts,
        json!({
            "node":{"node_id":node.id,"node_revision":node_revision,"daemon_generation":lease.as_ref().map(|l|l.generation),"status":enum_string(&node.status)},
            "build":{"build_sha":build_sha,"protocol_version":"agentfirm-member-trust/1","schema_version":SCHEMA_VERSION},
            "projects":record_summaries("node_project_registration",store.latest_node_project_registrations().unwrap_or_default().into_iter().filter(|p|p.node_id==node_id).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "team_supervisors":record_summaries("team_supervisor_lease",store.team_runs().unwrap_or_default().into_iter().filter(|r|r.execution_node_id==node_id).filter_map(|r|store.latest_team_supervisor_lease(&r.id).ok().flatten()).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "delivery_backlog":{"depth":backlog,"oldest_age_ms":null,"recovery_required":backlog>0},
            "runtime_recovery":record_summaries("runtime_command",runtime_recovery),
            "provider_admission":record_summaries("provider_compatibility_admission",store.latest_provider_compatibility_admissions().unwrap_or_default().into_iter().filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "workspace_safety":record_summaries("workspace_binding",node_member_run_ids.iter().filter_map(|id|current_workspace(&facts,id)).cloned().collect()),
            "diagnostics":[{"kind":"daemon_lease","state":lease.as_ref().map(|l|enum_string(&l.status)).unwrap_or_else(||"unavailable".into())}],
            "remote_fabric":remote_fabric,
        }),
        vec![],
        operator_actions,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    #[test]
    fn query_is_closed_and_bounded() {
        assert!(Query::parse("/v1/views/global-work?limit=201").is_err());
        assert!(Query::parse("/v1/views/global-work?mystery=x").is_err());
        assert_eq!(
            Query::parse("/v1/views/global-work?team_id=a&team_id=b")
                .unwrap()
                .values["team_id"],
            ["a", "b"]
        );
        assert_eq!(
            Query::parse("/v1/views/global-work?assignee_kind=unassigned")
                .unwrap()
                .values["assignee_kind"],
            ["unassigned"]
        );
    }

    #[test]
    fn empty_global_view_is_zero_match_and_read_only() {
        let root = PathBuf::from(format!(
            "/tmp/agentfirm-role-view-purity-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let stores = vec![("space-empty".to_string(), HarnessStore::new(&root))];
        let view =
            global_work_view(&stores, &Query::parse("/v1/views/global-work").unwrap()).unwrap();
        assert_eq!(view["view_kind"], json!("global_work"));
        assert_eq!(view["data"]["items"], json!([]));
        assert_eq!(view["data"]["pending_migration_work_ids"], json!([]));
        assert_eq!(view["data"]["page"]["next_cursor"], Value::Null);
        assert!(
            !root.exists(),
            "read-only RoleView must not initialize a Store"
        );
    }

    #[test]
    fn historical_duplicate_active_membership_fails_role_view_closed() {
        let duplicate = vec![
            json!({"id":"membership-1","team_id":"team-1","agent_member_id":"agent-1","state":"active","membership_generation":1}),
            json!({"id":"membership-2","team_id":"team-1","agent_member_id":"agent-1","state":"active","membership_generation":2}),
        ];
        let error = ensure_active_membership_cardinality(&duplicate)
            .expect_err("ambiguous historical authority must fail closed");
        assert!(error.contains("IDENTITY_CONFLICT"));
    }

    #[test]
    fn host_delivery_reconcile_projection_is_team_scoped() {
        let team_work_ids = BTreeSet::from(["work-team-a"]);
        let team_delivery = json!({"id":"delivery-a","work_id":"work-team-a","status":"failed"});
        let sibling_delivery = json!({"id":"delivery-b","work_id":"work-team-b","status":"failed"});
        assert!(delivery_requires_team_reconcile(
            &team_delivery,
            &team_work_ids
        ));
        assert!(!delivery_requires_team_reconcile(
            &sibling_delivery,
            &team_work_ids
        ));
    }

    #[test]
    fn collaboration_projection_filters_by_team_before_any_page_limit() {
        let root = PathBuf::from(format!(
            "/tmp/agentfirm-role-view-collaboration-page-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        let fixture =
            serde_json::from_str::<harness_core::collaboration::WorkDelegationV1>(include_str!(
                "../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
            ))
            .unwrap();
        let ledger = root.join("agentfirm_collaboration_operations.jsonl");
        let mut writer = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&ledger)
            .unwrap();
        for index in 0..=205_u64 {
            let mut delegation = fixture.clone();
            delegation.id = if index == 205 {
                "zzz-visible-after-company-first-200".into()
            } else {
                format!("noise-{index:03}")
            };
            delegation.source_work_attestation_id = format!("attestation-{index}");
            delegation.source_work_ref.work_id = format!("source-work-{index}");
            delegation.source_team_id = "noise-source-team".into();
            delegation.source_work_ref.team_id = delegation.source_team_id.clone();
            delegation.target_placement.team_id = if index == 205 {
                "team-visible".into()
            } else {
                "noise-target-team".into()
            };
            let operation = harness_store::CollaborationOperation {
                store_version: harness_core::collaboration::COLLABORATION_STORE_VERSION.into(),
                company_id: "company-1".into(),
                command_name: "fixture.insert".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "fixture".into(),
                },
                idempotency_key: format!("fixture-{index}"),
                request_fingerprint: format!("sha256:{index:064x}"),
                aggregate_kind: "work_delegation_v1".into(),
                aggregate_id: delegation.id.clone(),
                store_sequence: index + 1,
                resulting_revision: delegation.revision,
                resulting_projection: serde_json::to_value(&delegation).unwrap(),
                immutable_side_records: Vec::new(),
                created_at: format!("unix-ms:{index}"),
            };
            writeln!(writer, "{}", serde_json::to_string(&operation).unwrap()).unwrap();
        }
        writer.flush().unwrap();
        let (as_of, visible) = list_team_collaboration_delegations(
            &HarnessStore::new(&root),
            "company-1",
            "team-visible",
        )
        .unwrap();
        assert_eq!(as_of, 206);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "zzz-visible-after-company-first-200");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn message_delivery_state_distinguishes_every_canonical_outcome() {
        let row = |status: &str| json!({"status": status});
        assert_eq!(message_delivery_state(&[]), "unsettled");
        assert_eq!(message_delivery_state(&[&row("queued")]), "queued");
        assert_eq!(message_delivery_state(&[&row("routed")]), "queued");
        assert_eq!(message_delivery_state(&[&row("claimed")]), "delivered");
        assert_eq!(
            message_delivery_state(&[&row("provider_received")]),
            "delivered"
        );
        assert_eq!(
            message_delivery_state(&[&row("acknowledged")]),
            "acknowledged"
        );
        assert_eq!(message_delivery_state(&[&row("failed")]), "failed");
        assert_eq!(message_delivery_state(&[&row("expired")]), "failed");
        assert_eq!(message_delivery_state(&[&row("invalidated")]), "failed");
        assert_eq!(
            message_delivery_state(&[&row("acknowledged"), &row("queued")]),
            "queued",
            "one pending recipient keeps the Message queued"
        );
        assert_eq!(
            message_delivery_state(&[&row("acknowledged"), &row("provider_received")]),
            "delivered"
        );
        assert_eq!(
            message_delivery_state(&[&row("acknowledged"), &row("failed")]),
            "failed"
        );
    }

    fn host_run_fixture(host_thread_id: Option<&str>, mode: HostControlMode) -> AgentTeamRun {
        AgentTeamRun {
            id: "run-1".into(),
            agent_team_id: "team-1".into(),
            execution_node_id: "node-1".into(),
            project_binding_id: "project-1".into(),
            previous_run_id: None,
            host_surface: "codex".into(),
            host_thread_id: host_thread_id.map(str::to_owned),
            host_actor: None,
            host_control_mode: mode,
            objective: "test".into(),
            execution_root: None,
            status: harness_core::TeamRunStatus::Running,
            member_run_ids: Vec::new(),
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        }
    }

    #[test]
    fn host_session_mode_distinguishes_managed_external_and_unbound() {
        assert_eq!(
            host_session_mode(Some(&host_run_fixture(
                Some("thread-1"),
                HostControlMode::External
            ))),
            "external_interactive"
        );
        assert_eq!(
            host_session_mode(Some(&host_run_fixture(
                Some("thread-1"),
                HostControlMode::Managed
            ))),
            "harness_managed"
        );
        assert_eq!(
            host_session_mode(Some(&host_run_fixture(None, HostControlMode::External))),
            "unbound"
        );
        assert_eq!(
            host_session_mode(Some(&host_run_fixture(
                Some("  "),
                HostControlMode::Managed
            ))),
            "unbound",
            "a blank thread id is not a binding"
        );
        assert_eq!(host_session_mode(None), "unbound");
    }

    #[test]
    fn exact_session_history_survives_a_member_adapter_generation_change() {
        let sessions = vec![json!({
            "id":"agent-session-1",
            "execution_space_id":"space-1",
            "agent_member_id":"member-1",
            "lifecycle":"idle",
            "provider_kind":"codex",
            "runtime_generation":1,
            "native_session_ref":{
                "provider":"codex",
                "execution_mode":"codex_app_server",
                "native_session_id":"native-thread-1",
                "native_locator_kind":"codex_rollout",
                "adapter_contract_version":"codex-app-server-v1",
                "availability":"available",
                "supports_resume":true
            }
        })];

        // A MemberRun may now be adapter generation 2 after Reopen while this
        // machine-owned AgentSession remains generation 1. Exact identity,
        // provider and native-session binding still authorize owner history.
        let (session, native) = exact_agent_session_binding(
            &sessions,
            "space-1",
            "member-1",
            "native-thread-1",
            Some("codex_app_server"),
        )
        .expect("same native AgentSession remains the exact history authority");
        assert_eq!(session["runtime_generation"], 1);
        assert_eq!(native.native_session_id, "native-thread-1");
    }

    #[test]
    fn interrupt_action_requires_the_exact_active_verified_runtime_binding() {
        let member_run = json!({"id":"member-run-1","runtime_generation":2});
        let pending = json!({
            "id":"member-run-1",
            "runtime_generation":2,
            "provider_profile":{"capability_bindings":[{
                "capability":"interrupt_current_cycle",
                "status":"review_required",
                "admission":"pending_dependency"
            }]}
        });
        assert!(!member_run_has_active_provider_capability(
            &[pending],
            &member_run,
            "interrupt_current_cycle"
        ));

        let active = json!({
            "id":"member-run-1",
            "runtime_generation":2,
            "provider_profile":{"capability_bindings":[{
                "capability":"interrupt_current_cycle",
                "status":"verified",
                "admission":"active"
            }]}
        });
        assert!(member_run_has_active_provider_capability(
            &[active],
            &member_run,
            "interrupt_current_cycle"
        ));

        let stale_generation = json!({
            "id":"member-run-1",
            "runtime_generation":1,
            "provider_profile":{"capability_bindings":[{
                "capability":"interrupt_current_cycle",
                "status":"verified",
                "admission":"active"
            }]}
        });
        assert!(!member_run_has_active_provider_capability(
            &[stale_generation],
            &member_run,
            "interrupt_current_cycle"
        ));
    }

    #[test]
    fn ready_capability_admission_requires_active_core_bindings() {
        let active_binding = |capability: &str| json!({"capability":capability,"status":"verified","admission":"active"});
        let active = json!({"capability_bindings":[
            active_binding("open_or_resume"),
            active_binding("start_cycle"),
            active_binding("observe")
        ]});
        assert_eq!(
            provider_core_capability_admission(Some(&active)).0,
            "active"
        );

        let pending = json!({"capability_bindings":[
            active_binding("open_or_resume"),
            {"capability":"start_cycle","status":"review_required","admission":"pending_dependency"},
            active_binding("observe")
        ]});
        assert_eq!(
            provider_core_capability_admission(Some(&pending)).0,
            "review_required"
        );

        let missing = json!({"capability_bindings":[active_binding("open_or_resume")]});
        assert_eq!(
            provider_core_capability_admission(Some(&missing)).0,
            "unavailable"
        );
        assert_eq!(provider_core_capability_admission(None).0, "unknown");
    }
}
