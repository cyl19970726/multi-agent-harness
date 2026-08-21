use super::*;

pub(super) fn http_action_error_response(error: CliError) -> (&'static str, serde_json::Value) {
    match error {
        CliError::Store(StoreError::LockTimeout(detail)) => (
            "503 Service Unavailable",
            serde_json::json!({
                "ok": false,
                "error": "store_busy",
                "retryable": true,
                "detail": format!("timed out waiting for store write lock {detail}"),
            }),
        ),
        other => (
            "400 Bad Request",
            serde_json::json!({"ok": false, "error": other.to_string()}),
        ),
    }
}

pub(super) fn retired_http_path(path: &str) -> bool {
    path == "/v1/goals"
        || path.starts_with("/v1/goals/")
        || path == "/v1/tasks"
        || path.starts_with("/v1/tasks/")
        || path == "/v1/phases"
        || path.starts_with("/v1/phases/")
}

/// Apply a `POST /v1/projects/switch {project: <id>}` request: switch the active
/// project atomically and return the new `(id, store)`. In raw-override mode (no
/// `firm_home`) there is no registry to switch, so it is rejected.
pub(super) fn handle_project_switch(
    projects: &ServeProjects,
    body: &serde_json::Value,
) -> CliResult<(String, HarnessStore)> {
    let id = json_string(body, "project")
        .or_else(|| json_string(body, "id"))
        .or_else(|| json_string(body, "project_id"))
        .ok_or_else(|| CliError::Usage("missing `project` id to switch to".to_string()))?;
    let home = projects.firm_home.as_ref().ok_or_else(|| {
        CliError::Usage(
            "serve is running with a raw --store/FIRM_ROOT override; project switch is unavailable"
                .to_string(),
        )
    })?;
    let ctx = project::switch_current_project(home, &id, &now_string()).map_err(project_err)?;
    Ok((ctx.id.clone(), HarnessStore::new(ctx.store_root)))
}

pub(super) fn handle_space_switch(
    projects: &ServeProjects,
    body: &serde_json::Value,
) -> CliResult<(String, HarnessStore)> {
    let id = json_string(body, "space")
        .or_else(|| json_string(body, "id"))
        .or_else(|| json_string(body, "space_id"))
        .ok_or_else(|| CliError::Usage("missing `space` id to switch to".to_string()))?;
    let home = projects.firm_home.as_ref().ok_or_else(|| {
        CliError::Usage(
            "serve is running with a raw --store/FIRM_ROOT override; Execution Space switch is unavailable"
                .to_string(),
        )
    })?;
    let space = execution_space::switch_current_space(home, &id, &now_string())
        .map_err(execution_space_err)?;
    Ok((space.id.clone(), HarnessStore::new(space.store_root)))
}

/// Render the compatibility context as a native Project Binding. The old
/// project-derived store remains visible only as an explicitly labelled
/// compatibility locator; it is not the binding's owned state.
pub(super) fn project_context_json(ctx: &ProjectContext, current: &str) -> serde_json::Value {
    let binding = project::firm_home()
        .ok()
        .and_then(|home| project::binding_for_root(&ctx.project_root, &home).ok());
    serde_json::json!({
        "id": ctx.id,
        "project_root": ctx.project_root.display().to_string(),
        "compatibility_store_root": ctx.store_root.display().to_string(),
        "kind": ctx.kind,
        "is_git_repo": ctx.is_git_repo,
        "repository_url": binding.as_ref().and_then(|value| value.repository_url.clone()),
        "default_branch": binding.as_ref().and_then(|value| value.default_branch.clone()),
        "git_common_dir": binding.as_ref().and_then(|value| value.git_common_dir.as_ref()).map(|path| path.display().to_string()),
        "instruction_boundary": binding.as_ref().map(|value| value.instruction_boundary.display().to_string()).unwrap_or_else(|| ctx.project_root.display().to_string()),
        "skill_discovery_boundary": binding.as_ref().map(|value| value.skill_discovery_boundary.display().to_string()).unwrap_or_else(|| ctx.project_root.display().to_string()),
        "worktree_policy": binding.as_ref().map(|value| value.worktree_policy.clone()),
        "permission_policy": binding.as_ref().map(|value| value.permission_policy.clone()),
        "identity_boundary": "project_binding",
        "owns_execution_store": false,
        "is_current": ctx.id == current,
    })
}

/// Extract a query-string parameter value from a request target like
/// `/v1/snapshot?project=foo&x=1`. Returns the raw (un-decoded) value; project ids
/// are restricted to `[A-Za-z0-9._-]` so no percent-decoding is needed.
pub(super) fn query_param(target: &str, key: &str) -> Option<String> {
    let query = target.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub(super) fn handle_http_action(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    execution_space_id: &str,
    path: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    if role_actions_api::is_retired_legacy_write_path(path) {
        return Err(CliError::Usage(
            "RETIRED_WRITE_AUTHORITY: legacy local Work/WorkDelegation writers are closed".into(),
        ));
    }
    // Mission/Mission Log write endpoints retired with the DOC-108 legacy
    // CompanyOS cutover (same retirement as the `mission
    // create|update-context|close|log append` CLI commands and the
    // `mission_*` MCP writer tools — see `retired_mission_write_error`).
    // Mission rows remain readable history through the CLI legacy reads and
    // the Stage A export/verify path; no current surface may write them.
    if path == "/v1/missions" {
        return Err(retired_mission_write_error("create"));
    }
    if path
        .strip_prefix("/v1/missions/")
        .and_then(|rest| rest.strip_suffix("/close"))
        .is_some()
    {
        return Err(retired_mission_write_error("close"));
    }
    if path
        .strip_prefix("/v1/missions/")
        .and_then(|rest| rest.strip_suffix("/teams"))
        .is_some()
    {
        return Err(CliError::Usage(
            "POST /v1/missions/{id}/teams was retired with the legacy CompanyOS cutover (DOC-108): Mission no longer owns Team creation. Create durable Teams directly through POST /v1/teams (or `harness team create`) without Mission provenance.".to_string(),
        ));
    }
    if path
        .strip_prefix("/v1/missions/")
        .and_then(|rest| rest.strip_suffix("/context"))
        .is_some()
    {
        return Err(retired_mission_write_error("update-context"));
    }
    if path
        .strip_prefix("/v1/missions/")
        .and_then(|rest| rest.strip_suffix("/log"))
        .is_some()
    {
        return Err(retired_mission_write_error("log-append"));
    }
    if let Some(attention_id) = path
        .strip_prefix("/v1/host-attentions/")
        .and_then(|rest| rest.strip_suffix("/ack"))
    {
        return ack_host_attention_value(store, attention_id, body);
    }
    // Wave write endpoints retired with the ADR 0051 Mission Log cutover
    // (same retirement as the `wave create|update|advance|gate` CLI
    // commands — see `retired_wave_write_error`). HTTP and MCP share the
    // same retirement so no surface keeps a live Wave-write dual path.
    // GET-style Wave/Mission reads are unaffected; only the four POST
    // mutation routes below are gone.
    if path == "/v1/waves" {
        return Err(retired_wave_write_error("create"));
    }
    if path
        .strip_prefix("/v1/waves/")
        .and_then(|rest| rest.strip_suffix("/gate"))
        .is_some()
    {
        return Err(retired_wave_write_error("gate"));
    }
    if path
        .strip_prefix("/v1/waves/")
        .and_then(|rest| rest.strip_suffix("/context"))
        .is_some()
    {
        return Err(retired_wave_write_error("update"));
    }
    if path
        .strip_prefix("/v1/waves/")
        .and_then(|rest| rest.strip_suffix("/advance"))
        .is_some()
    {
        return Err(retired_wave_write_error("advance"));
    }
    if path == "/v1/team-runs" {
        return create_team_run_value(store, project_context, execution_space_id, body);
    }
    if path == "/v1/work-delegations" {
        return create_work_delegation_value(store, body);
    }
    if let Some(delegation_id) = path
        .strip_prefix("/v1/work-delegations/")
        .and_then(|rest| rest.strip_suffix("/cancel"))
    {
        return cancel_work_delegation_value(store, delegation_id, body);
    }
    if let Some(team_run_id) = path
        .strip_prefix("/v1/team-runs/")
        .and_then(|rest| rest.strip_suffix("/works"))
    {
        return create_team_work_value(store, team_run_id, body);
    }
    if let Some(rest) = path.strip_prefix("/v1/team-runs/") {
        let parts = rest.split('/').collect::<Vec<_>>();
        if let [team_run_id, "works", work_id, operation] = parts.as_slice() {
            return mutate_team_work_value(store, team_run_id, work_id, operation, body);
        }
    }
    if let Some(team_run_id) = path
        .strip_prefix("/v1/team-runs/")
        .and_then(|rest| rest.strip_suffix("/members"))
    {
        let member = TeamMemberSpec {
            agent_member_id: required_json_string(body, "agent_member_id")?,
            name: required_json_string(body, "name")?,
            role: required_json_string(body, "role")?,
            provider: required_json_string(body, "provider")?,
            execution_mode: optional_json_string(body, "execution_mode")?,
            model: optional_json_string(body, "model")?,
            effort: optional_json_string(body, "effort")?,
            service_tier: optional_json_string(body, "service_tier")?,
            provider_cwd_hint: optional_json_string(body, "provider_cwd_hint")?,
            owned_paths: optional_json_string_array(body, "owned_paths")?,
            resume_native_session_id: optional_json_string(body, "resume_native_session_id")?,
            initial_work: None,
        };
        let initial_work = optional_json_string(body, "initial_work")?;
        let (run, member, work) = add_team_run_member(
            store,
            project_context,
            team_run_id,
            &member,
            initial_work.as_deref(),
        )?;
        return Ok(serde_json::json!({
            "team_run": run,
            "member_run": member,
            "work": work,
        }));
    }
    if let Some(team_run_id) = path
        .strip_prefix("/v1/team-runs/")
        .and_then(|rest| rest.strip_suffix("/messages"))
    {
        return Err(CliError::Usage(format!(
            "RETIRED_WRITE_AUTHORITY: run-addressed message writer is closed for {team_run_id}"
        )));
    }
    if let Some(rest) = path.strip_prefix("/v1/team-runs/") {
        let parts = rest.split('/').collect::<Vec<_>>();
        if let [team_run_id, "messages", message_id, "ack"] = parts.as_slice() {
            return Err(CliError::Usage(format!(
                "RETIRED_WRITE_AUTHORITY: run-addressed message acknowledgement is closed for {team_run_id}/{message_id}"
            )));
        }
        if let [team_run_id, "messages", message_id, "reconcile-delivery"] = parts.as_slice() {
            return Err(CliError::Usage(format!(
                "RETIRED_WRITE_AUTHORITY: run-addressed message reconciliation is closed for {team_run_id}/{message_id}"
            )));
        }
        if let [team_run_id, "messages", message_id, "answer"] = parts.as_slice() {
            return Err(CliError::Usage(format!(
                "UNAUTHORIZED_ACTOR: provider answer route {team_run_id}/{message_id} requires authenticated mutation dispatch"
            )));
        }
        if let [team_run_id, "members", member_run_id, "steer"] = parts.as_slice() {
            return steer_team_member_value(store, team_run_id, member_run_id, body);
        }
        if let [team_run_id, "members", member_run_id, "interrupt"] = parts.as_slice() {
            return interrupt_team_member_value(store, team_run_id, member_run_id, body);
        }
        if let [team_run_id, "members", member_run_id, "close"] = parts.as_slice() {
            return close_team_member_value(store, team_run_id, member_run_id, body);
        }
        if let [team_run_id, "members", member_run_id, "reopen"] = parts.as_slice() {
            return reopen_team_member_value(store, team_run_id, member_run_id, body);
        }
        if let [team_run_id, "members", member_run_id, "resume"] = parts.as_slice() {
            return resume_team_member_value(store, team_run_id, member_run_id, body);
        }
        if let [team_run_id, "members", member_run_id, "rename"] = parts.as_slice() {
            return Ok(serde_json::to_value(rename_team_run_member(
                store,
                team_run_id,
                member_run_id,
                &required_json_string(body, "name")?,
            )?)?);
        }
        if let [team_run_id, "members", member_run_id, "deactivate"] = parts.as_slice() {
            return Ok(serde_json::to_value(deactivate_team_run_member(
                store,
                team_run_id,
                member_run_id,
                &required_json_string(body, "reason")?,
            )?)?);
        }
    }
    if let Some(team_run_id) = path
        .strip_prefix("/v1/team-runs/")
        .and_then(|rest| rest.strip_suffix("/transition"))
    {
        return transition_team_run_value(store, team_run_id, body);
    }
    if path == "/v1/messages" {
        return create_message_value(store, body);
    }
    if path == "/v1/teams" {
        return create_team_value(store, execution_space_id, body);
    }
    if path == "/v1/gateway/tick" {
        return provider_gateway_tick_value(
            store,
            project_context,
            GatewayOptions {
                dry_run: json_bool(body, "dry_run").unwrap_or(false),
                start_runtime: json_bool(body, "start_runtime").unwrap_or(false),
                timeout_ms: json_u64(body, "timeout_ms").unwrap_or(3_000),
                claim_ttl_ms: json_u64(body, "claim_ttl_ms").unwrap_or(300_000),
            },
        );
    }
    Err(CliError::Usage(format!("unknown action path: {path}")))
}

#[cfg(test)]
mod tests_http_action_error_response {
    use super::*;

    #[test]
    fn exhausted_store_contention_is_retryable_http_503() {
        let (status, body) = http_action_error_response(CliError::Store(StoreError::LockTimeout(
            "/tmp/store/.store.lock".into(),
        )));
        assert_eq!(status, "503 Service Unavailable");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "store_busy");
        assert_eq!(body["retryable"], true);
        assert!(body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains(".store.lock")));
    }

    #[test]
    fn non_contention_action_errors_remain_non_retryable_client_errors() {
        let (status, body) = http_action_error_response(CliError::Usage("invalid request".into()));
        assert_eq!(status, "400 Bad Request");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"], "invalid request");
        assert!(body.get("retryable").is_none());
    }
}
