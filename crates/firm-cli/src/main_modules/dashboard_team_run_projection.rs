use super::*;

/// A bounded canonical projection for a Team deep link. The shared Dashboard
/// builder applies the selected-run filter before it materializes JSON, so
/// unrelated history never becomes part of this response in memory.
pub(super) fn dashboard_team_run_snapshot(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<serde_json::Value> {
    dashboard_snapshot_with_team_run(store, Some(team_run_id))
}

#[cfg(test)]
pub(super) fn dashboard_team_run_snapshot_via_global(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<serde_json::Value> {
    let mut snapshot = dashboard_snapshot(store)?;
    scope_dashboard_snapshot(&mut snapshot, team_run_id)?;
    Ok(snapshot)
}

#[cfg(test)]
fn scope_dashboard_snapshot(snapshot: &mut serde_json::Value, team_run_id: &str) -> CliResult<()> {
    let selected = snapshot["team_runs"]
        .as_array()
        .and_then(|runs| {
            runs.iter()
                .find(|run| json_field_eq(run, "id", team_run_id))
        })
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("TeamRun not found: {team_run_id}")))?;
    let mission_id = selected
        .get("mission_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let team_id = selected
        .get("agent_team_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let execution_node_id = selected
        .get("execution_node_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let project_binding_id = selected
        .get("project_binding_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);

    retain_json_rows(snapshot, "team_runs", |row| {
        json_field_eq(row, "id", team_run_id)
    });
    for key in [
        "member_runs",
        "team_messages",
        "works",
        "work_deliveries",
        "team_supervisor_leases",
        "member_actions",
        "delegation_runs",
        "team_run_events",
    ] {
        retain_json_rows(snapshot, key, |row| {
            json_field_eq(row, "team_run_id", team_run_id)
        });
    }
    let agent_member_ids = json_string_set(snapshot, "member_runs", "agent_member_id");
    let member_run_ids = json_string_set(snapshot, "member_runs", "id");
    let work_ids = json_string_set(snapshot, "works", "id");
    retain_json_rows(snapshot, "work_events", |row| {
        json_field_in(row, "work_id", &work_ids)
    });
    retain_json_rows(snapshot, "members", |row| {
        json_field_in(row, "id", &agent_member_ids)
    });
    retain_json_rows(snapshot, "agent_identities", |row| {
        json_field_in(row, "id", &agent_member_ids)
    });
    retain_json_rows(snapshot, "agent_sessions", |row| {
        json_field_in(row, "agent_member_id", &agent_member_ids)
    });
    retain_json_rows(snapshot, "teams", |row| {
        team_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "id", id))
    });
    retain_json_rows(snapshot, "team_memberships", |row| {
        team_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "team_id", id))
    });
    retain_json_rows(snapshot, "work_execution_bindings", |row| {
        json_field_in(row, "work_id", &work_ids)
    });
    retain_json_rows(snapshot, "canonical_messages", |row| {
        json_field_eq(row, "team_run_id", team_run_id)
    });
    let message_ids = json_string_set(snapshot, "canonical_messages", "id");
    retain_json_rows(snapshot, "canonical_message_deliveries", |row| {
        json_field_in(row, "message_id", &message_ids)
    });
    retain_json_rows(snapshot, "team_member_close_requests", |row| {
        json_field_in(row, "member_run_id", &member_run_ids)
    });
    retain_json_rows(snapshot, "missions", |row| {
        mission_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "id", id))
    });
    for key in ["legacy_waves", "mission_log"] {
        retain_json_rows(snapshot, key, |row| {
            mission_id
                .as_deref()
                .is_some_and(|id| json_field_eq(row, "mission_id", id))
        });
    }
    retain_json_rows(snapshot, "work_delegations", |row| {
        json_nested_field_eq(row, "source_work_ref", "team_run_id", team_run_id)
            || json_nested_field_eq(row, "target_work_ref", "team_run_id", team_run_id)
    });
    let delegation_ids = json_string_set(snapshot, "work_delegations", "id");
    retain_json_rows(snapshot, "work_delegation_events", |row| {
        json_field_in(row, "delegation_id", &delegation_ids)
    });
    retain_json_rows(snapshot, "execution_nodes", |row| {
        execution_node_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "id", id))
    });
    retain_json_rows(snapshot, "node_project_registrations", |row| {
        execution_node_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "node_id", id))
            && project_binding_id
                .as_deref()
                .is_some_and(|id| json_field_eq(row, "project_binding_id", id))
    });
    retain_json_rows(snapshot, "node_daemon_leases", |row| {
        execution_node_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "node_id", id))
    });
    retain_json_rows(snapshot, "integrity_annotations", |row| {
        json_field_eq(row, "team_run_id", team_run_id)
    });
    for key in ["messages", "events", "evidence", "provider_child_threads"] {
        retain_json_rows(snapshot, key, |_| false);
    }
    if let Some(object) = snapshot.as_object_mut() {
        object.insert("company_os".to_string(), serde_json::json!({}));
    }
    Ok(())
}

#[cfg(test)]
fn json_string_set(snapshot: &serde_json::Value, key: &str, field: &str) -> HashSet<String> {
    snapshot[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.get(field).and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
fn json_field_eq(row: &serde_json::Value, field: &str, expected: &str) -> bool {
    row.get(field).and_then(serde_json::Value::as_str) == Some(expected)
}

#[cfg(test)]
fn json_nested_field_eq(
    row: &serde_json::Value,
    parent: &str,
    field: &str,
    expected: &str,
) -> bool {
    row.get(parent)
        .and_then(|value| value.get(field))
        .and_then(serde_json::Value::as_str)
        == Some(expected)
}

#[cfg(test)]
fn json_field_in(row: &serde_json::Value, field: &str, expected: &HashSet<String>) -> bool {
    row.get(field)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| expected.contains(value))
}

#[cfg(test)]
fn retain_json_rows(
    snapshot: &mut serde_json::Value,
    key: &str,
    mut keep: impl FnMut(&serde_json::Value) -> bool,
) {
    if let Some(rows) = snapshot
        .get_mut(key)
        .and_then(serde_json::Value::as_array_mut)
    {
        rows.retain(|row| keep(row));
    }
}
