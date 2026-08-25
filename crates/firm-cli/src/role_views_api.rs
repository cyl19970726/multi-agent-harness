//! Server-built, read-only RoleViews for the local AgentFirm product loop.
//!
//! The browser consumes these bounded projections and never folds ledgers or
//! invents lifecycle state. All writes remain on the canonical Mission Log
//! mutation service shipped by the historical Wave 4A development batch.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::agentfirm_api::{ActorKind, ActorRef};
use harness_core::{
    derive_work_successor_ids, work_readiness, AgentTeam, AgentTeamRun, HostControlMode,
    NativeSessionRef, Work, WorkClaimMode, WorkReadinessReason,
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
    /// The Dashboard connection terminates on this process's loopback socket.
    /// This is local same-user read authority, not an AgentMember credential
    /// and never authorizes a mutation.
    pub local_operator: bool,
}

#[derive(Default)]
pub(crate) struct Query {
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

pub(crate) struct Facts {
    space_id: String,
    store_identity: String,
    sequence: u64,
    work_sequence: u64,
    team_sequence: u64,
    run_sequence: u64,
    team_revisions: BTreeMap<String, u64>,
    run_revisions: BTreeMap<String, u64>,
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
        for (id, run) in all_latest_runs {
            let resolved_space = store
                .current_team_run_execution_space(&run)
                .map_err(|error| error.to_string())?;
            if resolved_space == space_id {
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
                .current_work_deliveries(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            work_events: work_operations
                .iter()
                .map(|operation| serde_json::to_value(&operation.event).unwrap_or(Value::Null))
                .collect(),
            side,
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
        "id":first_string(&["id","delivery_id","message_id","work_id"]).unwrap_or("unknown"),
        "work_id":first_string(&["work_id"]),
        "member_run_id":first_string(&["member_run_id","recipient_member_run_id"]),
        "requirement_id":first_string(&["requirement_id"]),
        "status":first_string(&["state","status","lifecycle","verdict","runtime_status"]),
        "version":value.get("version").and_then(Value::as_u64),
        "actor_ref":actor_ref,
        "summary":first_string(&["summary","summary_markdown","detail_markdown","observed_failure","failure_code","reason"]),
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
        rows.push(json!({"source":"work_delivery","id":delivery["delivery_id"],"work_id":delivery["work_id"],"actor_ref":null,"status":delivery["status"],"summary":display_text(delivery["failure_code"].as_str()),"created_at":delivery["updated_at"]}));
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
    let readiness = work_readiness(work, &facts.works);
    let failed_or_cancelled_prerequisite_work_ids = readiness
        .reasons
        .iter()
        .filter_map(|reason| match reason {
            WorkReadinessReason::PrerequisiteFailed { work_id }
            | WorkReadinessReason::PrerequisiteCancelled { work_id } => Some(work_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let unsatisfied_prerequisite_work_ids = readiness
        .reasons
        .iter()
        .filter_map(|reason| match reason {
            WorkReadinessReason::PrerequisiteMissing { work_id }
            | WorkReadinessReason::PrerequisitePending { work_id, .. } => Some(work_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let readiness_state = if readiness.ready {
        "ready"
    } else if !failed_or_cancelled_prerequisite_work_ids.is_empty()
        || readiness
            .reasons
            .iter()
            .any(|reason| matches!(reason, WorkReadinessReason::PrerequisiteMissing { .. }))
    {
        "requires_host_attention"
    } else if readiness.reasons.iter().any(|reason| {
        matches!(
            reason,
            WorkReadinessReason::WorkNotOpen { .. }
                | WorkReadinessReason::WorkConditionNotNormal { .. }
        )
    }) {
        "not_claimable"
    } else {
        "waiting_prerequisites"
    };
    let reason_codes = readiness
        .reasons
        .iter()
        .map(|reason| match reason {
            WorkReadinessReason::WorkNotOpen { .. } => "work_not_open",
            WorkReadinessReason::WorkConditionNotNormal { .. } => "work_condition_not_normal",
            WorkReadinessReason::PrerequisiteMissing { .. } => "prerequisite_missing",
            WorkReadinessReason::PrerequisitePending { .. } => "prerequisite_pending",
            WorkReadinessReason::PrerequisiteFailed { .. } => "prerequisite_failed",
            WorkReadinessReason::PrerequisiteCancelled { .. } => "prerequisite_cancelled",
        })
        .collect::<Vec<_>>();
    let successor_work_ids = derive_work_successor_ids(&work.id, &facts.works);
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
        "successor_work_ids":successor_work_ids,
        "readiness": {
            "state": readiness_state,
            "reason_codes": reason_codes,
            "unsatisfied_prerequisite_work_ids": unsatisfied_prerequisite_work_ids,
            "failed_or_cancelled_prerequisite_work_ids": failed_or_cancelled_prerequisite_work_ids,
        },
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

fn work_graph(works: &[Value]) -> Value {
    let mut edges = works
        .iter()
        .flat_map(|work| {
            let dependent_work_id = work["work_id"].as_str().unwrap_or_default().to_string();
            work["prerequisite_work_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(move |prerequisite_work_id| {
                    json!({
                        "prerequisite_work_id": prerequisite_work_id,
                        "dependent_work_id": dependent_work_id,
                        "kind": "hard",
                    })
                })
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left["prerequisite_work_id"]
            .as_str()
            .cmp(&right["prerequisite_work_id"].as_str())
            .then(
                left["dependent_work_id"]
                    .as_str()
                    .cmp(&right["dependent_work_id"].as_str()),
            )
    });
    let ids_for_state = |state: &str| {
        works
            .iter()
            .filter(|work| work["readiness"]["state"] == state)
            .filter_map(|work| work["work_id"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    json!({
        "nodes": works,
        "edges": edges,
        "ready_work_ids": ids_for_state("ready"),
        "attention_work_ids": ids_for_state("requires_host_attention"),
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

mod member_surface;
mod router;
mod team_surface;
mod viewer_surface;
mod workspace_surface;

pub(crate) use member_surface::*;
pub(crate) use router::*;
pub(crate) use team_surface::*;
pub(crate) use viewer_surface::*;
pub(crate) use workspace_surface::*;

#[cfg(test)]
mod tests;
