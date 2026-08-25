use super::*;

impl HttpExchange<'_> {
    #[allow(unused_variables)]
    pub(super) fn handle_dashboard_post(&mut self) -> CliResult<bool> {
        let projects = self.projects;
        let stream = &mut *self.stream;
        let sse_manager = self.sse_manager.clone();
        let method = self.method.as_str();
        let path = &self.path;
        let path_only = &self.path_only;
        let project_param = &self.project_param;
        let project_id = &self.project_id;
        let store_owned = &self.store;
        let store = store_owned;
        let company_os_path = self.company_os_path;
        let body = &self.body;
        let trust_transport_token = &self.trust_transport_token;
        let trust_idempotency_key = &self.trust_idempotency_key;
        let trust_expected_version = self.trust_expected_version;
        let trust_confirmed_action = &self.trust_confirmed_action;
        let trust_identity_override_header = self.trust_identity_override_header;
        let live_provider_activity_token = &self.live_provider_activity_token;
        let body_json = if body.is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_slice::<serde_json::Value>(body) {
                Ok(value) => value,
                Err(error) => {
                    write_http_json(
                        stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok": false, "error": format!("invalid JSON body: {error}")}),
                    )?;
                    return Ok(true);
                }
            }
        };
        // DOC-108: every POST /v1/company-os/* writer is retired with its module.
        // One explicit tombstone answers them all; historical Company data is
        // export/verify-only through `harness legacy-company-os export|verify`.
        if company_os_path {
            write_http_json(
                stream,
                "410 Gone",
                &serde_json::json!({
                    "ok": false,
                    "error": "retired_write_authority",
                    "detail": "The legacy Company OS writers were retired with the DOC-108 cutover. Team-scoped Work is authoritative (harness team-run work / /v1/team-runs/*), the Global Work aggregate is read-only, and historical Company data is export/verify-only through `harness legacy-company-os export|verify`.",
                }),
            )?;
            return Ok(true);
        }
        // POST /v1/projects/switch — flip the active project in the registry +
        // `ACTIVE_PROJECT` marker so CLI-spawned workers and a live serve converge on
        // the same central store (multi-project P6 #89 invariant). This is a serve-level
        // routing action (not a store mutation), so it is handled before the generic
        // store-action dispatch. The Dashboard follows the bounded response with a
        // GET snapshot for the newly selected Project Binding.
        if path_only == "/v1/projects/switch" {
            match handle_project_switch(projects, &body_json) {
                Ok((id, _compatibility_store)) => write_http_json(
                    stream,
                    "200 OK",
                    &serde_json::json!({
                        "ok": true,
                        "result": {"current": id},
                    }),
                )?,
                Err(error) => write_http_json(
                    stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": error.to_string()}),
                )?,
            }
            return Ok(true);
        }

        if path_only == "/v1/spaces/switch" {
            match handle_space_switch(projects, &body_json) {
                Ok((id, _switch_store)) => write_http_json(
                    stream,
                    "200 OK",
                    &serde_json::json!({
                        "ok": true,
                        "result": {"current": id},
                    }),
                )?,
                Err(error) => write_http_json(
                    stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": error.to_string()}),
                )?,
            }
            return Ok(true);
        }

        if path_only == "/v1/live/member-activity" {
            write_http_json(
                stream,
                "410 Gone",
                &serde_json::json!({
                    "ok": false,
                    "error": "retired_live_member_activity",
                    "detail": "Use the typed, exact-AgentSession /v1/live/provider-activity bridge."
                }),
            )?;
            return Ok(true);
        }

        // POST /v1/live/provider-activity — private loopback ingress from the one
        // local NodeDaemon. The body cannot select an AgentSession: serve resolves
        // the exact current session from canonical runtime state or fails closed.
        // Registry and SSE output are process-memory-only and never replayed.
        if path_only == "/v1/live/provider-activity" {
            let expected_token = LIVE_PROVIDER_ACTIVITY_TOKEN.get();
            if expected_token.is_none()
                || live_provider_activity_token.as_deref() != expected_token.map(String::as_str)
            {
                write_http_json(
                    stream,
                    "401 Unauthorized",
                    &serde_json::json!({"ok": false, "error": "invalid_live_provider_activity_token"}),
                )?;
                return Ok(true);
            }
            let update = match serde_json::from_value::<LiveProviderActivityUpdate>(body_json) {
                Ok(update) => update,
                Err(error) => {
                    write_http_json(
                        stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok": false, "error": format!("invalid live provider activity: {error}")}),
                    )?;
                    return Ok(true);
                }
            };
            let result = (|| -> CliResult<serde_json::Value> {
                let (team_run_id, member_run_id, source_member_run_generation) = match &update {
                    LiveProviderActivityUpdate::Updated {
                        team_run_id,
                        member_run_id,
                        member_run_generation,
                        ..
                    }
                    | LiveProviderActivityUpdate::Terminal {
                        team_run_id,
                        member_run_id,
                        member_run_generation,
                        ..
                    } => (
                        team_run_id.clone(),
                        member_run_id.clone(),
                        *member_run_generation,
                    ),
                };
                let member = latest_member_runs_in_append_order(store_owned)?
                    .into_iter()
                    .find(|member| member.id == member_run_id)
                    .ok_or_else(|| {
                        CliError::Usage(format!("member run not found: {member_run_id}"))
                    })?;
                require_live_member_run_generation(
                    &member.id,
                    member.runtime_generation,
                    source_member_run_generation,
                )?;
                let run = latest_team_run(store_owned, &team_run_id)?;
                let current_execution_space_id = team_run_execution_space_id(store_owned, &run)?;
                if current_execution_space_id != *project_id {
                    return Err(CliError::Usage(format!(
                        "EXECUTION_SPACE_SCOPE_MISMATCH: TeamRun {team_run_id} belongs to Execution Space {current_execution_space_id}, not {project_id}"
                    )));
                }
                let project_binding_id = run.project_binding_id.clone();
                let scope = provider_event_api::exact_live_scope(
                    store_owned,
                    project_id,
                    &project_binding_id,
                    &team_run_id,
                    &member,
                )
                .map_err(|reason| CliError::Usage(reason.to_string()))?;
                let event = match update {
                    LiveProviderActivityUpdate::Updated {
                        provider,
                        kind,
                        display_summary,
                        ..
                    } => {
                        if run.status != TeamRunStatus::Running {
                            return Err(CliError::Usage(format!(
                                "team run {team_run_id} is {}, not running",
                                serde_snake_label(&run.status)
                            )));
                        }
                        ensure_member_coordination_open(&member)?;
                        if matches!(
                            member.status,
                            MemberRunStatus::Completed
                                | MemberRunStatus::Failed
                                | MemberRunStatus::Stopped
                                | MemberRunStatus::Blocked
                        ) {
                            return Err(CliError::Usage(format!(
                                "member run {member_run_id} is terminal and cannot publish live activity"
                            )));
                        }
                        if provider != member.provider {
                            return Err(CliError::Usage(
                                "provider activity does not match the canonical MemberRun provider"
                                    .to_string(),
                            ));
                        }
                        let display_summary = sanitize_live_member_preview(&display_summary)
                            .ok_or_else(|| {
                                CliError::Usage(
                                    "live provider activity summary must not be empty".to_string(),
                                )
                            })?;
                        let activity = provider_event_api::record_live(
                            scope.clone(),
                            &member.provider,
                            kind,
                            display_summary,
                        );
                        provider_event_api::updated_live_event(&scope, activity)
                    }
                    LiveProviderActivityUpdate::Terminal { .. } => {
                        provider_event_api::clear_live_terminal(&scope)
                    }
                };
                Ok(broadcast_live_provider_activity(
                    &sse_manager,
                    project_id,
                    &project_binding_id,
                    &member.agent_member_id,
                    event,
                ))
            })();
            match result {
                Ok(activity) => write_http_json(
                    stream,
                    "202 Accepted",
                    &serde_json::json!({"ok": true, "result": activity}),
                )?,
                Err(error) => write_http_json(
                    stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": error.to_string()}),
                )?,
            }
            return Ok(true);
        }

        // POST /v1/team-runs/{id}/members/{member-id}/reopen — reactivate the same
        // ProviderRuntimeProjection and, when no Supervisor currently owns the run, start one so a
        // managed adapter process resumes the recorded provider-native session.
        if let Some(rest) = path_only.strip_prefix("/v1/team-runs/") {
            let parts = rest.split('/').collect::<Vec<_>>();
            if let [team_run_id, "members", member_run_id, "reopen"] = parts.as_slice() {
                let result = (|| -> CliResult<serde_json::Value> {
                    let reopened =
                        reopen_team_member_value(store, team_run_id, member_run_id, &body_json)?;
                    let runtime_start = if reopened_member_requires_supervisor_start(
                        store,
                        team_run_id,
                        member_run_id,
                    )? {
                        Some(delegate_team_run_to_node_daemon_in_space(
                            store,
                            project_id,
                            team_run_id,
                            TEAM_RUN_START_DEFAULT_CONCURRENCY,
                        )?)
                    } else {
                        None
                    };
                    Ok(serde_json::json!({"reopen": reopened, "runtime_start": runtime_start}))
                })();
                match result {
                    Ok(reopened) => {
                        write_http_json(
                            stream,
                            "202 Accepted",
                            &serde_json::json!({"ok": true, "result": reopened}),
                        )?;
                    }
                    Err(error) => write_http_json(
                        stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok": false, "error": error.to_string()}),
                    )?,
                }
                return Ok(true);
            }
        }

        // POST /v1/team-runs/{id}/members/{member-id}/resume — capability-gated
        // alias over the reopen machinery for resuming the recorded native
        // session; refuses active members (message/steer is their continuation).
        if let Some(rest) = path_only.strip_prefix("/v1/team-runs/") {
            let parts = rest.split('/').collect::<Vec<_>>();
            if let [team_run_id, "members", member_run_id, "resume"] = parts.as_slice() {
                let result = (|| -> CliResult<serde_json::Value> {
                    let resumed =
                        resume_team_member_value(store, team_run_id, member_run_id, &body_json)?;
                    let runtime_start = if reopened_member_requires_supervisor_start(
                        store,
                        team_run_id,
                        member_run_id,
                    )? {
                        Some(delegate_team_run_to_node_daemon_in_space(
                            store,
                            project_id,
                            team_run_id,
                            TEAM_RUN_START_DEFAULT_CONCURRENCY,
                        )?)
                    } else {
                        None
                    };
                    Ok(serde_json::json!({"resume": resumed, "runtime_start": runtime_start}))
                })();
                match result {
                    Ok(resumed) => {
                        write_http_json(
                            stream,
                            "202 Accepted",
                            &serde_json::json!({"ok": true, "result": resumed}),
                        )?;
                    }
                    Err(error) => write_http_json(
                        stream,
                        "400 Bad Request",
                        &serde_json::json!({"ok": false, "error": error.to_string()}),
                    )?,
                }
                return Ok(true);
            }
        }

        // POST /v1/team-runs/{id}/start — ask the one machine NodeDaemon to adopt
        // the attempt. The HTTP server never starts an in-process or per-run
        // supervisor, so every public control surface shares the same parent fence.
        if let Some(team_run_id) = path_only
            .strip_prefix("/v1/team-runs/")
            .and_then(|rest| rest.strip_suffix("/start"))
        {
            let parse_positive = |key: &str, default: u64| -> CliResult<u64> {
                match body_json.get(key) {
                    None | Some(serde_json::Value::Null) => Ok(default),
                    Some(value) => value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
                        CliError::Usage(format!("JSON field {key} must be a positive integer"))
                    }),
                }
            };
            let result = (|| -> CliResult<(serde_json::Value, AgentTeamRun)> {
                let max_concurrency_u64 =
                    parse_positive("max_concurrency", TEAM_RUN_START_DEFAULT_CONCURRENCY as u64)?;
                let max_concurrency = usize::try_from(max_concurrency_u64)
                    .ok()
                    .filter(|value| *value <= 64)
                    .ok_or_else(|| {
                        CliError::Usage("max_concurrency must be between 1 and 64".to_string())
                    })?;
                let delegated = delegate_team_run_to_node_daemon_in_space(
                    store,
                    project_id,
                    team_run_id,
                    max_concurrency,
                )?;
                Ok((delegated, latest_team_run(store, team_run_id)?))
            })();
            match result {
                Ok((node_daemon, running)) => {
                    write_http_json(
                        stream,
                        "202 Accepted",
                        &serde_json::json!({
                            "ok": true,
                            "result": {
                                "id": running.id,
                                "status": running.status,
                                "node_daemon": node_daemon,
                            },
                        }),
                    )?;
                }
                Err(error) => write_http_json(
                    stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": error.to_string()}),
                )?,
            }
            return Ok(true);
        }

        // Raw --store compatibility mode has no registered project_root. Do not
        // mislabel its centralized store_root as an execution workspace.
        let project_context = projects
            .firm_home
            .as_ref()
            .map(|_| projects.context_for(project_param.as_deref(), Some(project_id), store));
        match handle_http_action(
            store,
            project_context.as_ref(),
            project_id,
            path_only,
            &body_json,
        ) {
            Ok(response) => write_http_json(
                stream,
                "200 OK",
                &serde_json::json!({"ok": true, "result": response}),
            )?,
            Err(error) => {
                let (status, body) = http_action_error_response(error);
                write_http_json(stream, status, &body)?;
            }
        }
        Ok(true)
    }
}
