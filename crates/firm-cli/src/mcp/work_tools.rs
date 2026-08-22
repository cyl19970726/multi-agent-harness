use super::*;

pub(super) fn tool_team_run_work_list(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let brief = arguments
        .get("brief")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let since = match arguments.get("since") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or_else(|| {
            "argument `since` must be a non-negative integer WorkOperation cursor".to_string()
        })?),
    };
    let cursors = match since {
        Some(_) => {
            Some(work_operation_cursors(store, team_run_id).map_err(|error| error.to_string())?)
        }
        None => None,
    };
    let mut works = store
        .latest_works()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|work| work.team_run_id == team_run_id)
        .filter(|work| {
            since.is_none_or(|cursor| {
                cursors
                    .as_ref()
                    .and_then(|cursors| cursors.get(&work.id))
                    .is_some_and(|sequence| *sequence > cursor)
            })
        })
        .collect::<Vec<_>>();
    works.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    if brief {
        let works_brief: Vec<String> = works.iter().map(format_work_brief_line).collect();
        return Ok(json!({"works_brief": works_brief}));
    }
    if let Some(since) = since {
        let next_since = cursors
            .as_ref()
            .and_then(|cursors| cursors.values().copied().max())
            .unwrap_or(0)
            .max(since);
        return Ok(json!({"since": since, "next_since": next_since, "works": works}));
    }
    Ok(json!({"works": works}))
}

/// `team_run_board_summary` -- mirrors `harness team-run board-summary`: a
/// single bounded plain-text digest (issue #305), returned under `summary`
/// rather than as the raw MCP text payload so the shape matches every other
/// tool result here.
pub(super) fn tool_team_run_board_summary(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    require_current_team_run(store, team_run_id)?;
    let summary =
        team_run_board_summary_text(store, team_run_id).map_err(|error| error.to_string())?;
    Ok(json!({"summary": summary}))
}

pub(super) fn tool_team_run_work_show(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    let team_run_id = required_str(arguments, "team_run_id")?;
    let work_id = required_str(arguments, "work_id")?;
    let work = store
        .latest_works()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|work| work.team_run_id == team_run_id && work.id == work_id)
        .ok_or_else(|| format!("Work not found: {work_id}"))?;
    let events = store
        .work_events()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|event| event.team_run_id == team_run_id && event.work_id == work_id)
        .collect::<Vec<_>>();
    let deliveries = store
        .latest_work_deliveries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|delivery| delivery.team_run_id == team_run_id && delivery.work_id == work_id)
        .collect::<Vec<_>>();
    Ok(json!({"work": work, "events": events, "deliveries": deliveries}))
}

pub(super) fn tool_team_run_work_create(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    const ALLOWED: &[&str] = &[
        "team_run_id",
        "id",
        "title",
        "context_markdown",
        "completion_criteria_markdown",
        "owner_member_run_id",
        "claim_mode",
        "eligible_member_ids",
        "prerequisite_work_ids",
        "priority",
        "caused_by_message_id",
        "idempotency_key",
    ];
    reject_unknown_arguments(arguments, "team_run_work_create", ALLOWED)?;
    let team_run_id = required_non_empty_str(arguments, "team_run_id")?;
    let run = require_current_team_run(store, team_run_id)?;
    let owner_member_run_id = optional_non_empty_str(arguments, "owner_member_run_id")?;
    let claim_mode = match optional_non_empty_str(arguments, "claim_mode")?.as_deref() {
        None if owner_member_run_id.is_some() => WorkClaimMode::HostAssign,
        None => WorkClaimMode::TeamClaim,
        Some("host_assign") => WorkClaimMode::HostAssign,
        Some("team_claim") => WorkClaimMode::TeamClaim,
        Some(value) => return Err(format!("invalid claim_mode `{value}`")),
    };
    let priority = match optional_non_empty_str(arguments, "priority")?.as_deref() {
        None | Some("normal") => WorkPriority::Normal,
        Some("low") => WorkPriority::Low,
        Some("high") => WorkPriority::High,
        Some("urgent") => WorkPriority::Urgent,
        Some(value) => return Err(format!("invalid priority `{value}`")),
    };
    optional_non_empty_str(arguments, "caused_by_message_id")?;
    optional_non_empty_str(arguments, "idempotency_key")?;
    let team = store
        .latest_teams()
        .map_err(|error| error.to_string())?
        .remove(&run.agent_team_id)
        .ok_or_else(|| format!("AgentTeam not found: {}", run.agent_team_id))?;
    let context = local_mcp_host_work_context(arguments, &team.host_agent_id);
    WorkApplication::new(store)
        .create(CreateWorkCommand {
            work_id: optional_non_empty_str(arguments, "id")?
                .unwrap_or_else(|| generated_id("work")),
            team_run_id: team_run_id.to_string(),
            accountable_team_id: run.agent_team_id,
            title: required_non_empty_str(arguments, "title")?.to_string(),
            context_markdown: optional_str(arguments, "context_markdown")?.unwrap_or_default(),
            completion_criteria_markdown: required_non_empty_str(
                arguments,
                "completion_criteria_markdown",
            )?
            .to_string(),
            claim_mode,
            eligible_member_ids: optional_string_array(arguments, "eligible_member_ids")?,
            prerequisite_work_ids: optional_string_array(arguments, "prerequisite_work_ids")?,
            priority,
            initial_member_run_id: owner_member_run_id,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context,
        })
        .map(|work| json!(work))
        .map_err(|error| error.to_string())
}

pub(super) fn tool_team_work_replace_dependencies(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    const ALLOWED: &[&str] = &[
        "team_id",
        "work_id",
        "expected_version",
        "prerequisite_work_ids",
        "reason",
        "idempotency_key",
    ];
    reject_unknown_arguments(arguments, "team_work_replace_dependencies", ALLOWED)?;
    let team_id = required_non_empty_str(arguments, "team_id")?;
    let work_id = required_non_empty_str(arguments, "work_id")?;
    let expected_version = arguments
        .get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "argument `expected_version` must be a non-negative integer".to_string())?;
    let reason = required_non_empty_str(arguments, "reason")?;
    let team = store
        .latest_teams()
        .map_err(|error| error.to_string())?
        .remove(team_id)
        .ok_or_else(|| format!("AgentTeam not found: {team_id}"))?;
    let mut context = local_mcp_host_work_context(arguments, &team.host_agent_id);
    context.causation_ref = Some(WorkCausationRef {
        kind: "work_dependency_reason".into(),
        id: reason.to_string(),
    });
    WorkApplication::new(store)
        .replace_dependencies(ReplaceWorkDependenciesCommand {
            accountable_team_id: team_id.to_string(),
            work_id: work_id.to_string(),
            expected_version,
            prerequisite_work_ids: optional_string_array(arguments, "prerequisite_work_ids")?,
            context,
        })
        .map(|work| json!(work))
        .map_err(|error| error.to_string())
}

/// Record a Work-bound Review through the local MCP Host boundary. Unlike the
/// retired HTTP route, this is a typed tool: caller-controlled identity and
/// authority fields are not part of its schema and unknown arguments fail.
pub(super) fn reject_unknown_arguments(
    arguments: &Value,
    tool: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| format!("{tool} arguments must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!(
            "unknown argument `{key}` for {tool}; actor identity is fixed by the local MCP boundary"
        ));
    }
    Ok(())
}

pub(super) fn optional_non_empty_str(
    arguments: &Value,
    key: &str,
) -> Result<Option<String>, String> {
    let value = optional_str(arguments, key)?;
    if value
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        Err(format!("argument `{key}` must not be empty"))
    } else {
        Ok(value)
    }
}

pub(super) fn local_mcp_host_work_context(
    arguments: &Value,
    host_agent_id: &str,
) -> WorkCommandContext {
    let host_actor = TeamActorRef {
        kind: TeamActorKind::Host,
        id: host_agent_id.to_string(),
        display_name: None,
        authn_source: Some("local_mcp_host_authority".to_string()),
    };
    WorkCommandContext {
        event_id: generated_id("work-event"),
        performed_by_actor: host_actor.clone(),
        authority_actor: Some(host_actor),
        causation_ref: arguments
            .get("caused_by_message_id")
            .and_then(Value::as_str)
            .map(|id| WorkCausationRef {
                kind: "team_message".to_string(),
                id: id.to_string(),
            }),
        idempotency_key: arguments
            .get("idempotency_key")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: false,
    }
}

pub(super) fn required_non_empty_str<'a>(
    arguments: &'a Value,
    key: &str,
) -> Result<&'a str, String> {
    let value = required_str(arguments, key)?;
    if value.trim().is_empty() {
        Err(format!("argument `{key}` must not be empty"))
    } else {
        Ok(value)
    }
}

pub(super) fn optional_string_array(arguments: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = arguments.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("argument `{key}` must be an array of non-empty strings"))?;
    values
        .iter()
        .map(|value| {
            let text = value
                .as_str()
                .ok_or_else(|| format!("argument `{key}` must contain only strings"))?;
            if text.trim().is_empty() {
                Err(format!("argument `{key}` must not contain empty strings"))
            } else {
                Ok(text.to_string())
            }
        })
        .collect()
}

pub(super) fn tool_team_run_work_mutate(
    store: &HarnessStore,
    arguments: &Value,
    operation: &str,
) -> Result<Value, String> {
    let team_run_id = required_non_empty_str(arguments, "team_run_id")?;
    let run = require_current_team_run(store, team_run_id)?;
    let team = store
        .latest_teams()
        .map_err(|error| error.to_string())?
        .remove(&run.agent_team_id)
        .ok_or_else(|| format!("AgentTeam not found: {}", run.agent_team_id))?;
    let work_id = required_non_empty_str(arguments, "work_id")?;
    let expected_version = arguments
        .get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "argument `expected_version` must be a non-negative integer".to_string())?;
    let context = local_mcp_host_work_context(arguments, &team.host_agent_id);
    let application = WorkApplication::new(store);
    let work = match operation {
        "assign" => application.assign_runtime(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "member_run_id")?,
            context,
        ),
        "rebind" => application.rebind(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "member_run_id")?,
            context,
        ),
        "block" => application.block_as_host(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "reason")?,
            context,
        ),
        "resume" => application.resume_as_host(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "resolution")?,
            context,
        ),
        "release" => application.release_as_host(work_id, expected_version, context),
        "request-changes" => application.request_changes(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "reason")?,
            context,
        ),
        "cancel" => application.cancel(
            work_id,
            expected_version,
            required_non_empty_str(arguments, "reason")?,
            context,
        ),
        other => return Err(format!("unsupported Work mutation: {other}")),
    }
    .map_err(|error| error.to_string())?;
    Ok(json!(work))
}

pub(super) fn tool_team_run_work_reconcile_delivery(
    store: &HarnessStore,
    arguments: &Value,
) -> Result<Value, String> {
    reconcile_team_work_delivery_value(store, required_str(arguments, "team_run_id")?, arguments)
        .map_err(|error| error.to_string())
}
