//! Server-built, read-only RoleViews for the local AgentFirm product loop.
//!
//! The browser consumes these bounded projections and never folds ledgers or
//! invents lifecycle state. All writes remain on the Wave 4A canonical
//! mutation service.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
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
                    "project" | "space" | "company" => {}
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
    teams: Vec<AgentTeam>,
    runs: Vec<AgentTeamRun>,
    works: Vec<Work>,
    members: Vec<Value>,
    member_runs: Vec<Value>,
    messages: Vec<Value>,
    message_deliveries: Vec<Value>,
    work_deliveries: Vec<Value>,
    side: Vec<Value>,
}

impl Facts {
    fn read(space_id: &str, store: &HarnessStore) -> Result<Self, String> {
        let operations = store
            .canonical_operations()
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
        let mut side = Vec::new();
        for operation in &operations {
            if operation.event.aggregate_kind == "work" {
                if let Ok(work) =
                    serde_json::from_value::<Work>(operation.resulting_projection.clone())
                {
                    works.insert(work.id.clone(), work);
                }
            }
            if operation.event.aggregate_kind != "work" {
                side.push(operation.resulting_projection.clone());
            }
            side.extend(operation.immutable_side_records.clone());
        }
        let store_identity = std::fs::canonicalize(store.root())
            .unwrap_or_else(|_| store.root().to_path_buf())
            .display()
            .to_string();
        Ok(Self {
            space_id: space_id.to_string(),
            store_identity,
            sequence,
            teams: store
                .latest_teams()
                .map_err(|error| error.to_string())?
                .into_values()
                .collect(),
            runs: store.team_runs().map_err(|error| error.to_string())?,
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
                .trust_team_messages(space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|value| serde_json::to_value(value).unwrap_or(Value::Null))
                .collect(),
            message_deliveries: store
                .trust_message_deliveries(space_id)
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

    fn latest_run(&self, team_id: &str) -> Option<&AgentTeamRun> {
        self.runs
            .iter()
            .filter(|run| run.agent_team_id == team_id)
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)))
    }
}

fn now() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{ms}")
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
        "sender":value["sender"],
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
    .rev()
    .find_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
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
    let deliveries = facts
        .work_deliveries
        .iter()
        .filter(|d| d["work_id"] == work.id)
        .collect::<Vec<_>>();
    let count_status = |status: &str| deliveries.iter().filter(|d| d["status"] == status).count();
    let workspace = work.active_member_run_id.as_deref().and_then(|id| {
        facts
            .side
            .iter()
            .rev()
            .find(|v| v["member_run_id"] == id && v.get("canonical_root").is_some())
    });
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
        "gate_summary": {"required": requirements.len(), "passed": evaluations.iter().filter(|v| v["verdict"] == "passed").count(), "failed": evaluations.iter().filter(|v| v["verdict"] == "failed").count(), "pending": requirements.len().saturating_sub(evaluations.len()+waivers.len()), "waived": waivers.iter().filter(|v| v["state"] == "active").count(), "stale": 0},
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
    data: Value,
    attention: Vec<Value>,
    actions: Vec<Value>,
) -> Value {
    json!({"view_kind":kind,"schema_version":SCHEMA_VERSION,"source_execution_space_id":facts.space_id,
        "source_store_identity":facts.store_identity,"as_of_event_sequence":facts.sequence,"generated_at":now(),
        "freshness":"current","data":data,"attention":attention,"allowed_actions":actions})
}

fn action(
    kind: &str,
    target_kind: &str,
    target_id: &str,
    version: Option<u64>,
    disabled: Option<&str>,
) -> Value {
    json!({"kind":kind,"target_ref":{"kind":target_kind,"id":target_id},"required_version":version,"disabled_reason":disabled})
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
        operator_view(current_space_id, current, node_id, build_sha, identity)
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
    let mut facet_nodes = BTreeSet::new();
    let mut facet_hosts = BTreeSet::new();
    let mut facet_members = BTreeSet::new();
    for (space_id, store) in spaces {
        let facts = Facts::read(space_id, store)
            .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
        max_sequence = max_sequence.max(facts.sequence);
        identities.push(facts.store_identity.clone());
        for work in &facts.works {
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
    let mut hasher = DefaultHasher::new();
    query.values.hash(&mut hasher);
    query.delegated.hash(&mut hasher);
    "updated_at:desc,work_id:asc".hash(&mut hasher);
    let query_fingerprint = hasher.finish();
    let offset = if let Some(cursor) = &query.cursor {
        let parts = cursor.split(':').collect::<Vec<_>>();
        if parts.len() != 4
            || parts[0] != "rv1"
            || u64::from_str_radix(parts[1], 16).ok() != Some(query_fingerprint)
            || parts[2].parse::<u64>().ok() != Some(max_sequence)
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
            "rv1:{query_fingerprint:016x}:{max_sequence}:{}",
            offset + page_items.len()
        )
    });
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
        teams: vec![],
        runs: vec![],
        works: vec![],
        members: vec![],
        member_runs: vec![],
        messages: vec![],
        message_deliveries: vec![],
        work_deliveries: vec![],
        side: vec![],
    };
    Ok(envelope(
        "company_work",
        &facts,
        json!({"query":query.values,"sort":[{"field":"updated_at","direction":"desc"},{"field":"work_id","direction":"asc"}],"items":page_items,"page":{"as_of_event_sequence":max_sequence,"item_count":all.len(),"next_cursor":next},"facets":{"teams":facets("team_id"),"missions":facets("mission_id"),"nodes":facet_nodes,"hosts":facet_hosts,"members":facet_members,"phases":facets("phase"),"conditions":facets("condition"),"resolutions":facets("resolution"),"modules":all.iter().flat_map(|v|v["module_refs"].as_array().into_iter().flatten()).filter_map(Value::as_str).collect::<BTreeSet<_>>(),"gate_states":["passed","failed","pending","waived","stale"]}}),
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
    let run = facts.latest_run(team_id);
    let run_id = run.map(|r| r.id.as_str());
    let works = facts
        .works
        .iter()
        .filter(|w| w.team_id.as_deref() == Some(team_id) || run_id == Some(w.team_run_id.as_str()))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let team_member_ids = team
        .member_ids
        .iter()
        .chain(std::iter::once(&team.host_agent_id))
        .collect::<BTreeSet<_>>();
    let members=facts.members.iter().filter(|m|m["id"].as_str().is_some_and(|id|team_member_ids.iter().any(|member|member.as_str()==id))).map(|member|{
        let member_id=member["id"].as_str().unwrap_or_default(); let current=facts.member_runs.iter().filter(|r|r["agent_member_id"]==member_id&&run_id.is_some_and(|id|r["team_run_id"]==id)&&r["coordination_status"]=="active").max_by_key(|r|r["runtime_generation"].as_u64().unwrap_or(0));
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
    let raw_reports = records(&facts, |v| v.get("report_revision").is_some());
    let raw_findings = records(&facts, |v| v.get("detail_markdown").is_some());
    let raw_failures = records(&facts, |v| v.get("observed_failure").is_some());
    let raw_requirements = records(&facts, |v| v.get("requirement_set_fingerprint").is_some());
    let raw_evaluations = records(&facts, |v| {
        v.get("verdict").is_some() && v.get("requirement_id").is_some()
    });
    let raw_waivers = records(&facts, |v| {
        v.get("authority_actor").is_some() && v.get("requirement_id").is_some()
    });
    let raw_workspace_attention = records(&facts, |v| {
        v.get("canonical_root").is_some() && v["lifecycle"] != "ready"
    });
    let raw_delegations = facts
        .side
        .iter()
        .filter(|v| v.get("source_work_ref").is_some() || v.get("target_work_ref").is_some())
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
    if !host {
        let data = json!({"team":{"team_id":team.id,"team_revision":1,"mission_id":team.mission_id,"node_id":team.node_id,"placement_generation":run.map(|_|1),"status":enum_string(&team.status)},"works":works,"members":members,"messages":messages,"reports":reports,"findings":findings,"failures":failures,"gate_requirements":requirements,"gate_evaluations":evaluations,"gate_waivers":waivers,"workspace_attention":workspace_attention,"delegation_provenance":delegations,"page":{"as_of_event_sequence":facts.sequence,"item_count":works.len(),"next_cursor":null}});
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
    let host_authorized = identity.is_some_and(|identity| {
        identity.actor.id == team.host_agent_id
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.id == team.host_agent_id)
    });
    let disabled = (!host_authorized).then_some("authenticated actor is not this Team's Host");
    let mut actions = Vec::new();
    if let Some(run_id) = run_id {
        actions.push(action("create_work", "team_run", run_id, None, disabled));
        actions.push(action("send_message", "team_run", run_id, None, disabled));
    }
    for w in &works {
        let id = w["work_id"].as_str().unwrap_or_default();
        let version = w["work_revision"].as_u64();
        for kind in [
            "assign_work",
            "rebind_work",
            "release_work",
            "request_changes",
            "accept_work",
            "cancel_work",
        ] {
            actions.push(action(kind, "work", id, version, disabled));
        }
        actions.push(action(
            "request_gate_evaluation",
            "work",
            id,
            version,
            disabled,
        ));
    }
    for requirement in &raw_requirements {
        let Some(id) = requirement["id"].as_str() else {
            continue;
        };
        let evaluator_enabled = identity.is_some_and(|identity| {
            serde_json::to_value(&identity.actor).ok().as_ref() == requirement.get("evaluator_ref")
        });
        actions.push(action(
            "evaluate_gate",
            "gate_requirement",
            id,
            requirement["version"].as_u64(),
            (!evaluator_enabled).then_some("authenticated actor is not the frozen evaluator"),
        ));
        let waiver_enabled = identity.is_some_and(|identity| !identity.authority_actors.is_empty());
        actions.push(action(
            "waive_gate",
            "gate_requirement",
            id,
            requirement["version"].as_u64(),
            (!waiver_enabled).then_some("credential has no waiver authority"),
        ));
    }
    for waiver in &raw_waivers {
        if waiver["state"] != "active" {
            continue;
        }
        let Some(id) = waiver["id"].as_str() else {
            continue;
        };
        let revoke_enabled = identity.is_some_and(|identity| {
            serde_json::to_value(&identity.actor).ok().as_ref() == waiver.get("performed_by_actor")
                && identity.authority_actors.iter().any(|actor| {
                    serde_json::to_value(actor).ok().as_ref() == waiver.get("authority_actor")
                })
        });
        actions.push(action(
            "revoke_waiver",
            "gate_waiver",
            id,
            waiver["version"].as_u64(),
            (!revoke_enabled).then_some("credential does not match the waiver authority"),
        ));
    }
    for member in &members {
        if let Some(member_run_id) = member["current_member_run_ref"].as_str() {
            for kind in ["close_member_run", "reopen_member_run", "retire_member_run"] {
                actions.push(action(kind, "member_run", member_run_id, None, disabled));
            }
            for kind in [
                "provision_workspace",
                "attach_workspace",
                "archive_workspace",
                "cleanup_workspace",
            ] {
                actions.push(action(kind, "member_run", member_run_id, None, disabled));
            }
        }
    }
    for delivery in facts
        .work_deliveries
        .iter()
        .filter(|delivery| matches!(delivery["status"].as_str(), Some("failed" | "expired")))
    {
        if let Some(id) = delivery["id"].as_str() {
            actions.push(action(
                "reconcile_delivery",
                "work_delivery",
                id,
                None,
                disabled,
            ));
        }
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
    let team = facts
        .runs
        .iter()
        .find(|r| r.id == team_run_id)
        .and_then(|r| facts.teams.iter().find(|t| t.id == r.agent_team_id))
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", team_run_id.to_string()))?;
    let my = facts
        .works
        .iter()
        .filter(|w| w.owner_member_id.as_deref() == Some(member_id))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let pool = facts
        .works
        .iter()
        .filter(|w| {
            w.phase == WorkPhase::Open
                && w.condition == WorkCondition::Normal
                && (w.eligible_member_ids.is_empty()
                    || w.eligible_member_ids.iter().any(|id| id == member_id))
        })
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let queued = facts
        .message_deliveries
        .iter()
        .filter(|d| d["recipient_member_run_id"] == member_run_id && d["status"] == "queued")
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
    let workspace = facts
        .side
        .iter()
        .rev()
        .find(|v| v["member_run_id"] == member_run_id && v.get("canonical_root").is_some())
        .cloned();
    let mut actions = Vec::new();
    for w in &my {
        let id = w["work_id"].as_str().unwrap_or_default();
        let version = w["work_revision"].as_u64();
        for kind in [
            "start_work",
            "block_work",
            "unblock_work",
            "submit_work",
            "revise_work",
            "write_report",
            "write_finding",
            "write_failure",
        ] {
            actions.push(action(kind, "work", id, version, None));
        }
    }
    for w in &pool {
        actions.push(action(
            "claim_work",
            "work",
            w["work_id"].as_str().unwrap_or_default(),
            w["work_revision"].as_u64(),
            None,
        ));
    }
    actions.push(action("send_message", "team_run", team_run_id, None, None));
    actions.push(action(
        "request_decision",
        "team_run",
        team_run_id,
        None,
        None,
    ));
    for requirement in records(&facts, |value| {
        value.get("requirement_set_fingerprint").is_some()
            && value.get("evaluator_ref")
                == identity
                    .and_then(|identity| serde_json::to_value(&identity.actor).ok())
                    .as_ref()
    }) {
        if let Some(id) = requirement["id"].as_str() {
            actions.push(action(
                "evaluate_gate",
                "gate_requirement",
                id,
                requirement["version"].as_u64(),
                None,
            ));
        }
    }
    Ok(envelope(
        "member_workbench",
        &facts,
        json!({"agent_member":agent_member_summary(&member),"member_run":member_run_summary(run),"my_works":my,"eligible_ready_pool":pool,"unread_messages":unread,"queued_deliveries":record_summaries("message_delivery",queued),"workspace_binding":workspace.as_ref().map(|value|record_summary("workspace_binding",value)),"native_session_health":run["native_session"].get("availability").cloned().unwrap_or(json!("unknown")),"pending_provider_interactions":[],"report_history":record_summaries("work_report",records(&facts,|v|v["authored_by"]["id"]==member_id&&v.get("report_revision").is_some())),"finding_history":record_summaries("work_finding",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("detail_markdown").is_some())),"failure_history":record_summaries("failure_analysis",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("observed_failure").is_some())),"gate_requirements":record_summaries("gate_requirement",records(&facts,|v|v.get("requirement_set_fingerprint").is_some()))}),
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
    let lease = store.latest_node_daemon_lease(node_id).map_err(|e| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            e.to_string(),
        )
    })?;
    let backlog = facts
        .message_deliveries
        .iter()
        .chain(facts.work_deliveries.iter())
        .filter(|d| {
            matches!(
                d["status"].as_str(),
                Some("queued" | "claimed" | "failed" | "expired")
            )
        })
        .count();
    let operator_authorized = identity.is_some_and(|identity| {
        matches!(identity.actor.kind, ActorKind::Human | ActorKind::Service)
    });
    Ok(envelope(
        "operator",
        &facts,
        json!({
            "node":{"node_id":node.id,"node_revision":1,"daemon_generation":lease.as_ref().map(|l|l.generation),"status":enum_string(&node.status)},
            "build":{"build_sha":build_sha,"protocol_version":"agentfirm-member-trust/1","schema_version":SCHEMA_VERSION},
            "projects":record_summaries("node_project_registration",store.latest_node_project_registrations().unwrap_or_default().into_iter().filter(|p|p.node_id==node_id).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "team_supervisors":record_summaries("team_supervisor_lease",store.team_runs().unwrap_or_default().into_iter().filter(|r|r.execution_node_id==node_id).filter_map(|r|store.latest_team_supervisor_lease(&r.id).ok().flatten()).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "delivery_backlog":{"depth":backlog,"oldest_age_ms":null,"recovery_required":backlog>0},
            "runtime_recovery":record_summaries("member_run",facts.member_runs.iter().filter(|r|matches!(r["runtime_status"].as_str(),Some("disconnected"|"failed"|"stopped"))).cloned().collect()),
            "provider_admission":record_summaries("provider_compatibility_admission",store.latest_provider_compatibility_admissions().unwrap_or_default().into_iter().filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "workspace_safety":record_summaries("workspace_binding",facts.side.iter().filter(|v|v.get("canonical_root").is_some()).cloned().collect()),
            "diagnostics":[{"kind":"daemon_lease","state":lease.as_ref().map(|l|enum_string(&l.status)).unwrap_or_else(||"unavailable".into())}]
        }),
        vec![],
        std::iter::once(action(
            "daemon_diagnostics",
            "execution_node",
            node_id,
            None,
            (!operator_authorized).then_some("operator credential required"),
        ))
        .chain(
            facts
                .work_deliveries
                .iter()
                .filter(|delivery| {
                    matches!(delivery["status"].as_str(), Some("failed" | "expired"))
                })
                .filter_map(|delivery| delivery["id"].as_str())
                .map(|id| {
                    action(
                        "reconcile_delivery",
                        "work_delivery",
                        id,
                        None,
                        (!operator_authorized).then_some("operator credential required"),
                    )
                }),
        )
        .chain(
            facts
                .message_deliveries
                .iter()
                .filter(|delivery| {
                    matches!(delivery["status"].as_str(), Some("failed" | "expired"))
                })
                .filter_map(|delivery| delivery["id"].as_str())
                .map(|id| {
                    action(
                        "reconcile_delivery",
                        "message_delivery",
                        id,
                        None,
                        (!operator_authorized).then_some("operator credential required"),
                    )
                }),
        )
        .collect(),
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
}
