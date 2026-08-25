use super::*;

impl HttpExchange<'_> {
    #[allow(unused_variables)]
    pub(super) fn handle_get_routes(&mut self) -> CliResult<bool> {
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
        if method == "GET" {
            let local_operator_read = stream
                .local_addr()
                .ok()
                .is_some_and(|address| address.ip().is_loopback())
                && stream
                    .peer_addr()
                    .ok()
                    .is_some_and(|address| address.ip().is_loopback());
            let role_view_identity = if path_only.starts_with("/v1/views/") {
                match trust_transport_token.as_deref() {
                    Some(_) => {
                        match resolve_agentfirm_http_credential(trust_transport_token.as_deref()) {
                            Ok(credential) => Some(role_views_api::ReadIdentity {
                                actor: credential.actor,
                                authority_actors: credential.authority_actors,
                                local_operator: local_operator_read,
                            }),
                            Err(message) => {
                                write_http_json(
                                    stream,
                                    "401 Unauthorized",
                                    &serde_json::json!({"ok":false,"error":{"code":"NOT_AUTHORIZED","message":message}}),
                                )?;
                                return Ok(true);
                            }
                        }
                    }
                    None if local_operator_read => Some(role_views_api::ReadIdentity {
                        actor: harness_core::agentfirm_api::ActorRef {
                            kind: harness_core::agentfirm_api::ActorKind::Service,
                            id: "local-dashboard-operator".into(),
                        },
                        authority_actors: Vec::new(),
                        local_operator: true,
                    }),
                    None => {
                        write_http_json(
                            stream,
                            "401 Unauthorized",
                            &serde_json::json!({"ok":false,"error":{"code":"NOT_AUTHORIZED","message":"non-loopback RoleView reads require an AgentFirm runtime context"}}),
                        )?;
                        return Ok(true);
                    }
                }
            } else {
                None
            };
            let execution_space_stores = projects
                .list_spaces()
                .into_iter()
                .map(|space| (space.id, HarnessStore::new(space.store_root)))
                .collect::<Vec<_>>();
            let role_view_store = if path_only.starts_with("/v1/views/") {
                match projects.scoped_store_for_project(
                    store_owned,
                    project_id,
                    project_param.as_deref(),
                ) {
                    Ok(store) => store,
                    Err(error) => {
                        let detail = error.to_string();
                        write_http_json(
                            stream,
                            "404 Not Found",
                            &serde_json::json!({"ok":false,"error":{"code":"PROJECT_BINDING_NOT_FOUND","message":detail}}),
                        )?;
                        return Ok(true);
                    }
                }
            } else {
                store_owned.clone()
            };
            if let Some(response) = role_views_api::handle_get(
                &role_view_store,
                &execution_space_stores,
                project_id,
                path_only,
                path,
                build_git_rev(),
                role_view_identity.as_ref(),
            ) {
                write_http_json(stream, response.status, &response.body)?;
                return Ok(true);
            }
            // DOC-108: the legacy Company OS read surface is retired with its
            // module. One explicit tombstone answers every /v1/company-os/* GET;
            // historical data is export/verify-only through
            // `harness legacy-company-os export|verify`.
            if company_os_path {
                write_http_json(
                    stream,
                    "410 Gone",
                    &serde_json::json!({
                        "ok": false,
                        "error": "retired_surface",
                        "detail": "The legacy Company OS API was retired with the DOC-108 cutover; historical Company data is export/verify-only through `harness legacy-company-os export|verify`. Current surfaces: /v1/views/global-work (read-only Global Work aggregate), /v1/teams, /v1/team-runs.",
                    }),
                )?;
                return Ok(true);
            }
            if path_only == "/v1/host-attentions" {
                let team_run_id = query_param(path, "team_run_id").unwrap_or_default();
                match host_attentions_value(store, &team_run_id) {
                    Ok(value) => write_http_json(stream, "200 OK", &value)?,
                    Err(CliError::Usage(detail)) => write_http_json(
                        stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "not_found", "detail": detail}),
                    )?,
                    Err(error) => return Err(error),
                }
                return Ok(true);
            }
            if path_only == "/v1/team-runs/host-inbox" {
                let surface = query_param(path, "surface").unwrap_or_default();
                let thread_id = query_param(path, "thread_id").unwrap_or_default();
                let include_all = query_param(path, "all")
                    .as_deref()
                    .is_some_and(|value| matches!(value, "1" | "true" | "yes"));
                match host_inbox_for_native_thread(store, &surface, &thread_id, include_all) {
                    Ok(runs) => {
                        write_http_json(stream, "200 OK", &serde_json::json!({"runs": runs}))?
                    }
                    Err(CliError::Usage(detail)) => write_http_json(
                        stream,
                        "400 Bad Request",
                        &serde_json::json!({"error": "invalid_host_binding", "detail": detail}),
                    )?,
                    Err(error) => return Err(error),
                }
                return Ok(true);
            }
            if path_only == "/v1/work-delegations" {
                let source_work_id = query_param(path, "source_work_id");
                let target_team_id = query_param(path, "target_agent_team_id");
                let state = query_param(path, "state");
                let delegations = store
                    .latest_work_delegations()?
                    .into_iter()
                    .filter(|delegation| {
                        source_work_id
                            .as_deref()
                            .is_none_or(|id| delegation.source_work_ref.work_id == id)
                    })
                    .filter(|delegation| {
                        target_team_id
                            .as_deref()
                            .is_none_or(|id| delegation.target_agent_team_id == id)
                    })
                    .filter(|delegation| {
                        state
                            .as_deref()
                            .is_none_or(|expected| serde_snake_label(&delegation.state) == expected)
                    })
                    .collect::<Vec<_>>();
                write_http_json(
                    stream,
                    "200 OK",
                    &serde_json::json!({"delegations": delegations}),
                )?;
                return Ok(true);
            }
            if path_only == "/v1/execution-nodes" {
                write_http_json(
                    stream,
                    "200 OK",
                    &serde_json::json!({
                        "nodes": store.latest_execution_nodes()?,
                        "registrations": store.latest_node_project_registrations()?,
                        "daemon_leases": store.latest_node_daemon_leases()?,
                    }),
                )?;
                return Ok(true);
            }
            if let Some(node_id) = path_only.strip_prefix("/v1/execution-nodes/") {
                if !node_id.contains('/') {
                    let node = store
                        .latest_execution_nodes()?
                        .into_iter()
                        .find(|node| node.id == node_id);
                    if let Some(node) = node {
                        let registrations = store
                            .latest_node_project_registrations()?
                            .into_iter()
                            .filter(|registration| registration.node_id == node_id)
                            .collect::<Vec<_>>();
                        let daemon_lease = store.latest_node_daemon_lease(node_id)?;
                        write_http_json(
                            stream,
                            "200 OK",
                            &serde_json::json!({
                                "node": node,
                                "registrations": registrations,
                                "daemon_lease": daemon_lease,
                            }),
                        )?;
                    } else {
                        write_http_json(
                            stream,
                            "404 Not Found",
                            &serde_json::json!({"error": "execution_node_not_found"}),
                        )?;
                    }
                    return Ok(true);
                }
            }
            if let Some(delegation_id) = path_only.strip_prefix("/v1/work-delegations/") {
                if !delegation_id.contains('/') {
                    let delegation = store
                        .latest_work_delegations()?
                        .into_iter()
                        .find(|delegation| delegation.id == delegation_id);
                    if let Some(delegation) = delegation {
                        let events = store
                            .work_delegation_events()?
                            .into_iter()
                            .filter(|event| event.delegation_id == delegation_id)
                            .collect::<Vec<_>>();
                        write_http_json(
                            stream,
                            "200 OK",
                            &serde_json::json!({"delegation": delegation, "events": events}),
                        )?;
                    } else {
                        write_http_json(
                            stream,
                            "404 Not Found",
                            &serde_json::json!({"error": "work_delegation_not_found"}),
                        )?;
                    }
                    return Ok(true);
                }
            }
            if let Some(rest) = path_only.strip_prefix("/v1/team-runs/") {
                let parts = rest.split('/').collect::<Vec<_>>();
                if let [team_run_id, "snapshot"] = parts.as_slice() {
                    match dashboard_team_run_snapshot(store, team_run_id) {
                        Ok(snapshot) => write_http_json(stream, "200 OK", &snapshot)?,
                        Err(CliError::Usage(detail)) => write_http_json(
                            stream,
                            "404 Not Found",
                            &serde_json::json!({"error": "team_run_not_found", "detail": detail}),
                        )?,
                        Err(error) => return Err(error),
                    }
                    return Ok(true);
                }
                if let [team_run_id, "members", member_run_id, "inbox"] = parts.as_slice() {
                    write_http_json(
                        stream,
                        "410 Gone",
                        &serde_json::json!({
                            "ok": false,
                            "error": {
                                "code": "RETIRED_RUNTIME_READER",
                                "message": "managed Member Inbox is exact-self only; use the Supervisor-bound `member inbox` command or an authenticated RoleView",
                                "resource_kind": "member_run",
                                "resource_id": member_run_id,
                                "team_run_id": team_run_id
                            }
                        }),
                    )?;
                    return Ok(true);
                }
                if let [team_run_id, "works"] = parts.as_slice() {
                    let mut works = store
                        .latest_works()?
                        .into_iter()
                        .filter(|work| work.team_run_id == *team_run_id)
                        .collect::<Vec<_>>();
                    works.sort_by(|left, right| {
                        work_priority_rank(right.priority)
                            .cmp(&work_priority_rank(left.priority))
                            .then_with(|| left.created_at.cmp(&right.created_at))
                            .then_with(|| left.id.cmp(&right.id))
                    });
                    write_http_json(stream, "200 OK", &serde_json::json!({"works": works}))?;
                    return Ok(true);
                }
                if let [team_run_id, "works", work_id] = parts.as_slice() {
                    let work = store
                        .latest_works()?
                        .into_iter()
                        .find(|work| work.team_run_id == *team_run_id && work.id == *work_id);
                    if let Some(work) = work {
                        let events = store
                            .work_events()?
                            .into_iter()
                            .filter(|event| event.work_id == *work_id)
                            .collect::<Vec<_>>();
                        let deliveries = store
                            .current_work_deliveries_for_team_run(team_run_id)?
                            .into_iter()
                            .filter(|delivery| delivery.work_id == *work_id)
                            .collect::<Vec<_>>();
                        write_http_json(
                            stream,
                            "200 OK",
                            &serde_json::json!({"work": work, "events": events, "deliveries": deliveries}),
                        )?;
                    } else {
                        write_http_json(
                            stream,
                            "404 Not Found",
                            &serde_json::json!({"error": "work_not_found"}),
                        )?;
                    }
                    return Ok(true);
                }
            }
            match path_only.as_str() {
                "/health" | "/v1/health" => write_http_json(
                    stream,
                    "200 OK",
                    &serde_json::json!({"status": "ok", "generated_at": now_string()}),
                )?,
                "/v1/snapshot" | "/v1/dashboard/snapshot" => {
                    // DOC-108: the snapshot no longer merges a Company Store
                    // projection; the execution store is the only source.
                    let snapshot = projects
                        .dashboard_snapshot_builds
                        .build(|| dashboard_snapshot(store_owned))?;
                    write_http_json(stream, "200 OK", &snapshot)?
                }
                "/v1/test/dashboard-snapshot-builds"
                    if dashboard_snapshot_build_test_pause().is_some() =>
                {
                    write_http_json(
                        stream,
                        "200 OK",
                        &projects.dashboard_snapshot_builds.test_metrics(),
                    )?
                }
                // GET /v1/meta — server build/data provenance (issue #307). Always
                // the coordination store (`store_owned`), never the Company OS
                // store: it answers "which build served this, which store did it
                // read, how far has that store's op log advanced".
                "/v1/meta" => write_http_json(stream, "200 OK", &dashboard_meta(store_owned)?)?,
                // GET /v1/projects — enumerate known projects (registry + on-disk stores
                // + reserved `_global`) for the dashboard picker. `current` marks the
                // active project (multi-project P6 / project-api task).
                "/v1/projects" => {
                    let current = projects.current_project_binding_id();
                    let list: Vec<serde_json::Value> = projects
                        .list_project_bindings()
                        .into_iter()
                        .map(|ctx| project_context_json(&ctx, &current))
                        .collect();
                    write_http_json(
                        stream,
                        "200 OK",
                        &serde_json::json!({"projects": list, "current": current}),
                    )?
                }
                // GET /v1/projects/current — the active project id + its context. Read
                // live so a `switch` (API or CLI) is reflected without a serve restart.
                "/v1/projects/current" => {
                    let current = projects.current_project_binding_id();
                    let ctx = projects
                        .list_project_bindings()
                        .into_iter()
                        .find(|context| context.id == current);
                    let context_json = ctx.map(|c| project_context_json(&c, &current));
                    write_http_json(
                        stream,
                        "200 OK",
                        &serde_json::json!({
                            "current": current,
                            "project": context_json,
                        }),
                    )?
                }
                "/v1/spaces" => {
                    let current = projects.current_space_id();
                    let list = projects
                        .list_spaces()
                        .iter()
                        .map(|space| execution_space_json(space, &current))
                        .collect::<Vec<_>>();
                    write_http_json(
                        stream,
                        "200 OK",
                        &serde_json::json!({"spaces": list, "current": current}),
                    )?
                }
                "/v1/spaces/current" => {
                    let current = projects.current_space_id();
                    let space = projects
                        .list_spaces()
                        .into_iter()
                        .find(|space| space.id == current);
                    write_http_json(
                        stream,
                        "200 OK",
                        &serde_json::json!({
                            "current": current,
                            "space": space.map(|space| execution_space_json(&space, &current)),
                        }),
                    )?
                }
                "/v1/events" => {
                    let requested_agent_member_id = query_param(path, "agent_id");
                    let requested_team_id = query_param(path, "team_id");
                    let selected_agent_member_id = match (
                        requested_agent_member_id.as_deref(),
                        requested_team_id.as_deref(),
                    ) {
                        (None, None) => None,
                        (Some(agent_member_id), Some(team_id)) => {
                            let team = store.latest_teams()?.remove(team_id).filter(|team| {
                                team.host_agent_id == agent_member_id
                                    || team.member_ids.iter().any(|id| id == agent_member_id)
                            });
                            let Some(team) = team else {
                                write_http_json(
                                    stream,
                                    "404 Not Found",
                                    &serde_json::json!({"ok":false,"error":{"code":"AGENT_NOT_IN_TEAM","message":"selected AgentMember is not in the selected AgentTeam"}}),
                                )?;
                                return Ok(true);
                            };
                            if !local_operator_read {
                                write_http_json(
                                    stream,
                                    "403 Forbidden",
                                    &serde_json::json!({"ok":false,"error":{"code":"LOCAL_OPERATOR_REQUIRED","message":"provider-native live Session reads are available only from the same-machine Dashboard"}}),
                                )?;
                                return Ok(true);
                            }
                            Some(agent_member_id.to_string())
                        }
                        _ => {
                            write_http_json(
                                stream,
                                "400 Bad Request",
                                &serde_json::json!({"ok":false,"error":{"code":"INVALID_SESSION_SCOPE","message":"team_id and agent_id must be supplied together"}}),
                            )?;
                            return Ok(true);
                        }
                    };
                    let selected_project_binding_id = if selected_agent_member_id.is_some() {
                        match projects
                            .exact_project_context_for(project_param.as_deref(), project_id)
                        {
                            Ok(project) => Some(project.id),
                            Err(error) => {
                                write_http_json(
                                    stream,
                                    "404 Not Found",
                                    &serde_json::json!({"ok":false,"error":{"code":"PROJECT_BINDING_NOT_FOUND","message":error.to_string()}}),
                                )?;
                                return Ok(true);
                            }
                        }
                    } else {
                        None
                    };
                    #[cfg(unix)]
                    if let (Some(agent_member_id), Some(callback), Some(firm_home)) = (
                        selected_agent_member_id.as_deref(),
                        projects.live_provider_activity_callback.as_ref(),
                        projects.firm_home.as_deref(),
                    ) {
                        if let Ok(node_id) = read_local_node_id() {
                            let daemon_instance_id =
                                supervisor_daemon::daemon_status_via_socket(firm_home, &node_id)
                                    .and_then(|raw| {
                                        serde_json::from_str::<serde_json::Value>(&raw).ok()
                                    })
                                    .and_then(|status| {
                                        status["instance_id"].as_str().map(ToString::to_string)
                                    });
                            if let Some(daemon_instance_id) = daemon_instance_id {
                                match supervisor_daemon::register_live_provider_activity_via_socket(
                                    firm_home,
                                    &node_id,
                                    supervisor_daemon::LiveProviderActivityRegistration {
                                        authority: &callback.authority,
                                        token: &callback.token,
                                        agent_member_id,
                                        expected_daemon_instance_id: &daemon_instance_id,
                                        serve_instance_id: &callback.serve_instance_id,
                                    },
                                ) {
                                    Some(Ok(response)) if response.contains("\"ok\":true") => {}
                                    Some(Ok(response)) => eprintln!(
                                        "serve: NodeDaemon rejected Team Session live sink: {response}"
                                    ),
                                    Some(Err(error)) => eprintln!(
                                        "serve: cannot register Team Session live sink: {error}"
                                    ),
                                    None => {}
                                }
                            }
                        }
                    }
                    // Scope coordination to the selected Execution Space and Company
                    // invalidations to the independently selected Company Store.
                    // Team Session live provider activity uses the same exact
                    // selected AgentMember and Team/local read boundary.
                    handle_sse_stream(
                        store_owned,
                        project_id,
                        selected_project_binding_id.as_deref(),
                        None,
                        selected_agent_member_id.as_deref(),
                        stream.try_clone()?,
                        sse_manager,
                    )?
                }
                "/v1/docs" => match read_allowed_doc(path) {
                    Ok((doc_path, content)) => write_http_json(
                        stream,
                        "200 OK",
                        &serde_json::json!({"path": doc_path, "content": content}),
                    )?,
                    Err(detail) => write_http_json(
                        stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "doc_not_found", "detail": detail}),
                    )?,
                },
                member_path
                    if member_path.starts_with("/v1/member-runs/")
                        && member_path.ends_with("/native-activity") =>
                {
                    write_http_json(
                        stream,
                        "410 Gone",
                        &serde_json::json!({
                            "error": "legacy_native_activity_route_retired",
                            "detail": "This unscoped route cannot prove the canonical Team and AgentSession scope. Use AgentWorkspace session_event_projection; provider-native open/resume remains a separate authorized action."
                        }),
                    )?
                }
                _ => write_http_json(
                    stream,
                    "404 Not Found",
                    &serde_json::json!({"error": "not_found", "path": path_only}),
                )?,
            }
            return Ok(true);
        }
        Ok(false)
    }
}
