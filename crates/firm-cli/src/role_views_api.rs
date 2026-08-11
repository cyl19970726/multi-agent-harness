//! Server-built, read-only RoleViews for the local AgentFirm product loop.
//!
//! The browser consumes these bounded projections and never folds ledgers or
//! invents lifecycle state. All writes remain on the Wave 4A canonical
//! mutation service.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::agentfirm_api::{ActorKind, ActorRef};
use harness_core::{AgentTeam, AgentTeamRun, Work, WorkCondition, WorkPhase};
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
        let run_revisions = run_rows.iter().fold(BTreeMap::new(), |mut revisions, run| {
            *revisions.entry(run.id.clone()).or_insert(0) += 1;
            revisions
        });
        let mut latest_runs = BTreeMap::new();
        for run in run_rows {
            latest_runs.insert(run.id.clone(), run);
        }
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
            work_sequence: store
                .work_operations()
                .map_err(|error| error.to_string())?
                .len() as u64,
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
            membership["agent_identity_id"]
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
    .filter(|actor| actor.get("kind").is_some() && actor.get("id").is_some())
    .cloned();
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

fn message_summary(value: &Value, deliveries: &[Value]) -> Value {
    json!({
        "message_id":value["id"],
        "work_id":value["work_id"],
        "sender":value["sender_actor_ref"],
        "recipients":value["recipients"],
        "response_intent":value["response_intent"],
        "created_at":value["created_at"],
        "delivery_summary":deliveries.iter().filter(|delivery|delivery["message_id"]==value["id"]).map(|delivery|delivery["status"].clone()).collect::<Vec<_>>(),
    })
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

fn work_summary(facts: &Facts, team: &AgentTeam, work: &Work) -> Value {
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
    json!({
        "work_id": work.id, "work_revision": work.version, "team_id": team.id, "mission_id": team.mission_id,
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
                    && membership["agent_identity_id"] == identity_id.as_str()
                    && membership["node_id"] == team.node_id
                    && membership["state"] == "active"
            })
            .collect::<Vec<_>>();
        memberships.len() == 1
            && facts.message_subscriptions.iter().any(|subscription| {
                subscription["subscriber_agent_id"] == identity_id.as_str()
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
    let result = if path == "/v1/views/company-work" {
        company_view(spaces, &query)
    } else if let Some(team_id) = path.strip_prefix("/v1/views/team-workspace/") {
        team_view(current_space_id, current, team_id, false, identity)
    } else if let Some(team_id) = path.strip_prefix("/v1/views/host-console/") {
        team_view(current_space_id, current, team_id, true, identity)
    } else if let Some(member_run_id) = path.strip_prefix("/v1/views/member-workbench/") {
        member_view(current_space_id, current, member_run_id, identity)
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

fn company_view(spaces: &[(String, HarnessStore)], query: &Query) -> ViewResult {
    let mut all = Vec::new();
    let mut max_sequence = 0;
    let mut identities = Vec::new();
    let mut snapshot_vector = Vec::new();
    let mut work_sources = BTreeMap::<String, String>::new();
    let mut facet_nodes = BTreeSet::new();
    let mut facet_hosts = BTreeSet::new();
    let mut facet_members = BTreeSet::new();
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
                .team_id
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
                continue;
            };
            if !query.matches("team_id", Some(&team.id))
                || !query.matches("mission_id", Some(&team.mission_id))
                || !query.matches("node_id", Some(&team.node_id))
                || !query.matches("host_id", Some(&team.host_agent_id))
                || !query.matches("member_id", work.owner_member_id.as_deref())
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
            let summary = work_summary(&facts, team, work);
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
            "Company Work sources changed during projection; retry the read".into(),
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
        space_id: "company".into(),
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
        side: vec![],
    };
    Ok(envelope(
        "company_work",
        &facts,
        json!({"query":query.values,"sort":[{"field":"updated_at","direction":"desc"},{"field":"work_id","direction":"asc"}],"items":page_items,"page":{"as_of_event_sequence":max_sequence,"item_count":all.len(),"next_cursor":next,"snapshot_vector":snapshot_vector},"facets":{"teams":facets("team_id"),"missions":facets("mission_id"),"nodes":facet_nodes,"hosts":facet_hosts,"members":facet_members,"phases":facets("phase"),"conditions":facets("condition"),"resolutions":facets("resolution"),"modules":all.iter().flat_map(|v|v["module_refs"].as_array().into_iter().flatten()).filter_map(Value::as_str).collect::<BTreeSet<_>>(),"gate_states":["passed","failed","pending","waived","stale"]}}),
        vec![],
        vec![],
    ))
}

fn team_view(
    space_id: &str,
    store: &HarnessStore,
    team_id: &str,
    host: bool,
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
    let run = facts.latest_run(team_id);
    let run_id = run.map(|r| r.id.as_str());
    let works = facts
        .works
        .iter()
        .filter(|w| w.team_id.as_deref() == Some(team_id) || run_id == Some(w.team_run_id.as_str()))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let team_work_ids = works
        .iter()
        .filter_map(|work| work["work_id"].as_str())
        .collect::<BTreeSet<_>>();
    let team_member_ids = team
        .member_ids
        .iter()
        .chain(std::iter::once(&team.host_agent_id))
        .collect::<BTreeSet<_>>();
    let members=facts.members.iter().filter(|m|m["id"].as_str().is_some_and(|id|team_member_ids.iter().any(|member|member.as_str()==id))).map(|member|{
        let member_id=member["id"].as_str().unwrap_or_default(); let active=facts.member_runs.iter().filter(|r|r["agent_member_id"]==member_id&&run_id.is_some_and(|id|r["team_run_id"]==id)&&r["coordination_status"]=="active").collect::<Vec<_>>(); let current=if active.len()==1 { Some(active[0]) } else { None };
        json!({"agent_member_ref":{"kind":"agent_member","id":member_id},"role":member["role"],"organization_status":member["organization_status"],"current_member_run_ref":current.and_then(|r|r["id"].as_str()),"runtime_state":current.and_then(|r|r["runtime_status"].as_str()),"runtime_generation":current.and_then(|r|r["runtime_generation"].as_u64()),"capacity":match current.and_then(|r|r["runtime_status"].as_str()){Some("running")|Some("queued")=>"busy",Some("idle")|Some("waiting")=>"available",_=>"unknown"}})
    }).collect::<Vec<_>>();
    let messages = facts
        .messages
        .iter()
        .filter(|m| run_id.is_some_and(|id| m["team_run_id"] == id))
        .map(|m| message_summary(m, &facts.message_deliveries))
        .collect::<Vec<_>>();
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
    if !host {
        let data = json!({"team":{"team_id":team.id,"team_revision":team_revision,"mission_id":team.mission_id,"node_id":team.node_id,"placement_generation":run.and_then(|run|facts.run_revisions.get(&run.id).copied()),"status":enum_string(&team.status)},"works":works,"members":members,"messages":messages,"reports":reports,"findings":findings,"failures":failures,"gate_requirements":requirements,"gate_evaluations":evaluations,"gate_waivers":waivers,"workspace_attention":workspace_attention,"delegation_provenance":delegations,"page":{"as_of_event_sequence":facts.sequence,"item_count":works.len(),"next_cursor":null}});
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
                Some("active") => actions.push(action(
                    "close_member_run",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                )),
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
            if matches!(
                member_run["runtime_status"].as_str(),
                Some("disconnected" | "failed" | "stopped")
            ) {
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
    Ok(envelope(
        "host_console",
        &facts,
        json!({"team_ref":team.id,"mission_ref":team.mission_id,"work_queues":{"ready":works.iter().filter(|w|w["phase"]=="open"&&w["condition"]=="normal").cloned().collect::<Vec<_>>(),"unassigned":works.iter().filter(|w|w["owner_actor_ref"].is_null()).cloned().collect::<Vec<_>>(),"blocked":works.iter().filter(|w|w["condition"]=="blocked").cloned().collect::<Vec<_>>(),"review":by_phase("review"),"integration":works.iter().filter(|w|w["module_refs"].as_array().is_some_and(|a|a.iter().any(|m|m=="integration-plan"))).cloned().collect::<Vec<_>>()},"member_capacity":members,"convergence_plans":[],"reusable_findings":findings,"workspace_conflicts":record_summaries("workspace_binding",raw_workspace_attention),"provider_capacity_attention":[{"state":"not_modeled","reason":"generic tool leases are outside Wave 4B"}],"deliveries_requiring_reconcile":record_summaries("work_delivery",facts.work_deliveries.iter().filter(|d|matches!(d["status"].as_str(),Some("failed"|"expired"))).cloned().collect()),"gate_attention":requirements,"daemon_summary":{"node_id":team.node_id,"lease_status":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|enum_string(&lease.status)),"generation":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|lease.generation)}}),
        identity_attention,
        actions,
    ))
}

fn member_view(
    space_id: &str,
    store: &HarnessStore,
    member_run_id: &str,
    identity: Option<&ReadIdentity>,
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
    let team_work_ids = facts
        .works
        .iter()
        .filter(|work| work.team_run_id == team_run_id)
        .map(|work| work.id.as_str())
        .collect::<BTreeSet<_>>();
    let my = facts
        .works
        .iter()
        .filter(|w| w.team_run_id == team_run_id && w.owner_member_id.as_deref() == Some(member_id))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let pool = facts
        .works
        .iter()
        .filter(|w| {
            w.team_run_id == team_run_id
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
        .filter(|d| d["recipient_identity_id"] == member_id && d["status"] == "queued")
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
        .map(|message| message_summary(message, &facts.message_deliveries))
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
        json!({"agent_member":agent_member_summary(&member),"member_run":member_run_summary(run),"my_works":my,"eligible_ready_pool":pool,"unread_messages":unread,"queued_deliveries":record_summaries("message_delivery",queued),"workspace_binding":workspace.as_ref().map(|value|record_summary("workspace_binding",value)),"native_session_health":run["native_session"].get("availability").cloned().unwrap_or(json!("unknown")),"pending_provider_interactions":[],"report_history":record_summaries("work_report",records(&facts,|v|v["authored_by"]["id"]==member_id&&v.get("report_revision").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"finding_history":record_summaries("work_finding",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("detail_markdown").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"failure_history":record_summaries("failure_analysis",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("observed_failure").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"gate_requirements":record_summaries("gate_requirement",records(&facts,|v|v.get("requirement_set_fingerprint").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id))&&facts.works.iter().any(|work|v["work_id"]==work.id&&v["work_revision"].as_u64()==Some(work.version))))}),
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
            Ok(json!({
                "company_id":company_id,
                "node_id":node_id,
                "state":"observed",
                "gateway_session":snapshot.active_session,
                "outbox_depth":queued,
                "inbox_depth":snapshot.inboxes.len(),
                "recovery_required":recovery_required,
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
    use std::path::PathBuf;
    #[test]
    fn query_is_closed_and_bounded() {
        assert!(Query::parse("/v1/views/company-work?limit=201").is_err());
        assert!(Query::parse("/v1/views/company-work?mystery=x").is_err());
        assert_eq!(
            Query::parse("/v1/views/company-work?team_id=a&team_id=b")
                .unwrap()
                .values["team_id"],
            ["a", "b"]
        );
    }

    #[test]
    fn empty_company_view_is_zero_match_and_read_only() {
        let root = PathBuf::from(format!(
            "/tmp/agentfirm-role-view-purity-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let stores = vec![("space-empty".to_string(), HarnessStore::new(&root))];
        let view = company_view(&stores, &Query::parse("/v1/views/company-work").unwrap()).unwrap();
        assert_eq!(view["data"]["items"], json!([]));
        assert_eq!(view["data"]["page"]["next_cursor"], Value::Null);
        assert!(
            !root.exists(),
            "read-only RoleView must not initialize a Store"
        );
    }

    #[test]
    fn historical_duplicate_active_membership_fails_role_view_closed() {
        let duplicate = vec![
            json!({"id":"membership-1","team_id":"team-1","agent_identity_id":"agent-1","state":"active","membership_generation":1}),
            json!({"id":"membership-2","team_id":"team-1","agent_identity_id":"agent-1","state":"active","membership_generation":2}),
        ];
        let error = ensure_active_membership_cardinality(&duplicate)
            .expect_err("ambiguous historical authority must fail closed");
        assert!(error.contains("IDENTITY_CONFLICT"));
    }
}
