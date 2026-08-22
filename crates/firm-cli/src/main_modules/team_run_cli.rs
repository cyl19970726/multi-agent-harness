use super::*;

pub(super) fn team_run_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(
        args,
        "team-run create|list|status|board-summary|work|recover|host-inbox|dispatch-host|bind-host|host-lease-status|renew-host-lease|release-host-lease|inbox|add-member|rename-member|close-member|reopen-member|deactivate-member|start|send|answer-message|events|wait|complete|cancel",
    )?;
    let json = has_flag(args, "--json");
    match args[0].as_str() {
        "work" => team_run_work_command(store, resolved, &args[1..])?,
        "dispatch-host" => {
            let result = dispatch_headless_host_once(store, resolved, &args[1..])?;
            print_json(&result)?;
        }
        "create" => {
            let agent_team_id = value(args, "--agent-team-id");
            let mut members: Vec<TeamMemberSpec> = many(args, "--member")
                .iter()
                .map(|raw| parse_team_member_spec(raw))
                .collect::<CliResult<_>>()?;
            if members.is_empty() {
                if let Some(team_id) = agent_team_id.as_deref() {
                    let execution_space_id = resolved
                        .execution_space_context
                        .as_ref()
                        .map(|space| space.id.as_str())
                        .ok_or_else(|| {
                            CliError::Usage(
                                "team-run create requires an explicitly selected --space".into(),
                            )
                        })?;
                    members =
                        team_member_specs_from_definition(store, execution_space_id, team_id)?;
                }
            }
            for resume in many(args, "--resume-member") {
                let (name, session_id) = resume.split_once(':').ok_or_else(|| {
                    CliError::Usage("--resume-member expects name:native-session-id".to_string())
                })?;
                let member = members
                    .iter_mut()
                    .find(|member| member.name == name)
                    .ok_or_else(|| {
                        CliError::Usage(format!("--resume-member names unknown member {name}"))
                    })?;
                member.resume_native_session_id = Some(session_id.to_string());
            }
            for override_spec in many(args, "--member-worktree") {
                let (name, provider_cwd_hint) = override_spec.split_once(':').ok_or_else(|| {
                    CliError::Usage("--member-worktree expects name:path".to_string())
                })?;
                let member = members
                    .iter_mut()
                    .find(|member| member.name == name)
                    .ok_or_else(|| {
                        CliError::Usage(format!("--member-worktree names unknown member {name}"))
                    })?;
                member.provider_cwd_hint = Some(provider_cwd_hint.to_string());
            }
            for override_spec in many(args, "--member-owned-path") {
                let (name, owned_path) = override_spec.split_once(':').ok_or_else(|| {
                    CliError::Usage("--member-owned-path expects name:path".to_string())
                })?;
                if owned_path.trim().is_empty() {
                    return Err(CliError::Usage(
                        "--member-owned-path path must not be empty".to_string(),
                    ));
                }
                let member = members
                    .iter_mut()
                    .find(|member| member.name == name)
                    .ok_or_else(|| {
                        CliError::Usage(format!("--member-owned-path names unknown member {name}"))
                    })?;
                member.owned_paths.push(owned_path.to_string());
            }
            for override_spec in many(args, "--member-effort") {
                let (name, effort) = override_spec.split_once(':').ok_or_else(|| {
                    CliError::Usage("--member-effort expects name:effort".to_string())
                })?;
                if effort.trim().is_empty() {
                    return Err(CliError::Usage(
                        "--member-effort effort must not be empty".to_string(),
                    ));
                }
                let member = members
                    .iter_mut()
                    .find(|member| member.name == name)
                    .ok_or_else(|| {
                        CliError::Usage(format!("--member-effort names unknown member {name}"))
                    })?;
                member.effort = Some(effort.to_string());
            }
            for override_spec in many(args, "--member-service-tier") {
                let (name, service_tier) = override_spec.split_once(':').ok_or_else(|| {
                    CliError::Usage("--member-service-tier expects name:service-tier".to_string())
                })?;
                if service_tier.trim().is_empty() {
                    return Err(CliError::Usage(
                        "--member-service-tier service tier must not be empty".to_string(),
                    ));
                }
                let member = members
                    .iter_mut()
                    .find(|member| member.name == name)
                    .ok_or_else(|| {
                        CliError::Usage(format!(
                            "--member-service-tier names unknown member {name}"
                        ))
                    })?;
                member.service_tier = Some(service_tier.to_string());
            }
            let budget_limit_usd = value(args, "--budget-usd")
                .map(|raw| {
                    raw.parse::<f64>()
                        .map_err(|_| CliError::Usage("--budget-usd must be a number".to_string()))
                })
                .transpose()?;
            let env_host_surface = std::env::var("STAR_HARNESS_HOST_SURFACE")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let env_host_thread_id = std::env::var("STAR_HARNESS_HOST_THREAD_ID")
                .ok()
                .filter(|s| !s.trim().is_empty());
            // Refuse ambiguous partial auto-bind: both must be present or
            // neither; a single env var is a misconfiguration, not intent.
            match (&env_host_surface, &env_host_thread_id) {
                (Some(_), None) => {
                    eprintln!(
                        "[WARNING] STAR_HARNESS_HOST_SURFACE is set but STAR_HARNESS_HOST_THREAD_ID is missing — refusing to auto-bind"
                    );
                }
                (None, Some(_)) => {
                    eprintln!(
                        "[WARNING] STAR_HARNESS_HOST_THREAD_ID is set but STAR_HARNESS_HOST_SURFACE is missing — refusing to auto-bind"
                    );
                }
                (Some(_), Some(_)) | (None, None) => {}
            }
            let host_surface = canonical_surface(
                &value(args, "--host-surface")
                    .or_else(|| env_host_surface.clone())
                    .unwrap_or_else(|| "cli".into()),
            )
            .to_string();
            let host_thread_id =
                value(args, "--host-thread-id").or_else(|| env_host_thread_id.clone());
            let requested_host_mode = value(args, "--host-runtime-mode");
            let host_control_mode = parse_host_runtime_mode(requested_host_mode.as_deref())?;
            let execution_space_id = resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.as_str())
                .ok_or_else(|| {
                    CliError::Usage(
                        "team-run create requires an explicitly selected --space".into(),
                    )
                })?;
            let team_id = agent_team_id.as_deref().ok_or_else(|| {
                CliError::Usage("--agent-team-id is required for team-run create".into())
            })?;
            apply_host_runtime_mode(
                store,
                execution_space_id,
                team_id,
                &mut members,
                host_control_mode,
            )?;
            let created = create_team_run(
                store,
                resolved.context.as_ref(),
                resolved
                    .execution_space_context
                    .as_ref()
                    .map(|space| space.id.as_str()),
                value(args, "--execution-root"),
                &required(args, "--objective")?,
                budget_limit_usd,
                &host_surface,
                host_thread_id.clone(),
                host_control_mode,
                value(args, "--previous"),
                agent_team_id,
                value(args, "--mission-id"),
                value(args, "--wave-id"),
                &members,
            )?;
            let mut host_lease = None;
            if created.team_run.host_thread_id.is_some() {
                let (lease, warning) = acquire_validated_interactive_host_lease(
                    store,
                    &created.team_run,
                    checked_host_binding_lease_ttl_ms(args)?,
                    &RuntimeHostSessionValidator::default(),
                    current_unix_ms_u64(),
                )?;
                host_lease = lease;
                if let Some(warning) = warning {
                    eprintln!("[WARNING] {warning}");
                }
            }
            if host_thread_id.is_none() {
                eprintln!(
                    "[WARNING] Team run {} created without host binding — member messages will queue silently.\n\
                     Bind with: harness team-run bind-host --id {} --surface <surface> --thread-id <thread-id>",
                    created.team_run.id, created.team_run.id
                );
            }
            if json {
                let mut output = created_team_run_json(&created);
                if let Some(object) = output.as_object_mut() {
                    object.insert(
                        "host_binding_lease".to_string(),
                        serde_json::json!(host_lease),
                    );
                }
                print_json(&output)?;
            } else {
                println!("{}", created.team_run.id);
            }
        }
        // complete / cancel share the HTTP attempt-transition logic, so CLI
        // and dashboard cannot disagree about attempt eligibility.
        "complete" => {
            let id = required(args, "--id")?;
            let run = transition_team_run(store, &id, TeamRunStatus::Completed)?;
            if json {
                print_json(&serde_json::json!(run))?;
            } else {
                println!("{}\t{}", run.id, serde_snake_label(&run.status));
            }
        }
        "cancel" => {
            let id = required(args, "--id")?;
            let run = if has_flag(args, "--confirm-provider-stopped") {
                recover_interrupted_team_run(
                    store,
                    &id,
                    &required(args, "--reason")?,
                    &value(args, "--cancelled-by").unwrap_or_else(|| "host".to_string()),
                )?
            } else {
                transition_team_run(store, &id, TeamRunStatus::Cancelled)?
            };
            if json {
                print_json(&serde_json::json!(run))?;
            } else {
                println!("{}\t{}", run.id, serde_snake_label(&run.status));
            }
        }
        "answer-message" => {
            let team_run_id = required(args, "--id")?;
            let message_id = required(args, "--message-id")?;
            let mut body = serde_json::json!({});
            if let Some(option_id) = value(args, "--option-id") {
                body["option_id"] = serde_json::Value::String(option_id);
            }
            if let Some(response_text) = value(args, "--response-text") {
                body["response_text"] = serde_json::Value::String(response_text);
            }
            let actor = team_run_host_actor(store, &team_run_id)?;
            let response = answer_provider_message_value(
                store,
                &team_run_id,
                &message_id,
                &body,
                &actor,
                "trusted_local_cli",
            )?;
            if json {
                print_json(&response)?;
            } else {
                println!("{}", response["id"].as_str().unwrap_or(&message_id));
            }
        }
        "list" => {
            // One Execution Space store holds every tenant bound to it (ADR
            // 0042), so an unscoped list makes every caller read every other
            // project's history. Filters, not a store split, are the fix.
            let project_filter = value(args, "--project-binding");
            let status_filter = value(args, "--status");
            let runs: Vec<_> = latest_team_runs_in_append_order(store)?
                .into_iter()
                .filter(|run| match project_filter.as_deref() {
                    Some(wanted) => run.project_binding_id == wanted,
                    None => true,
                })
                .filter(|run| match status_filter.as_deref() {
                    Some(wanted) => serde_snake_label(&run.status) == wanted,
                    None => true,
                })
                .collect();
            if json {
                let display = runs
                    .iter()
                    .map(|run| team_run_display_json(store, run))
                    .collect::<CliResult<Vec<_>>>()?;
                print_json(&display)?;
            } else {
                for run in &runs {
                    println!(
                        "{}\t{}\tmembers={}\t{}\t{}",
                        run.id,
                        serde_snake_label(&run.status),
                        run.member_run_ids.len(),
                        run.created_at,
                        run.objective
                    );
                }
            }
        }
        "status" => {
            let id = required(args, "--id")?;
            let run = latest_team_run(store, &id)?;
            let member_runs: Vec<ProviderRuntimeProjection> =
                latest_member_runs_in_append_order(store)?
                    .into_iter()
                    .filter(|member| member.team_run_id == id)
                    .collect();
            let actions = visible_member_actions_in_append_order(store)?;
            let works: Vec<Work> = store
                .latest_works()?
                .into_iter()
                .filter(|work| work.team_run_id == id)
                .collect();
            let latest_action_of = |member_run_id: &str| {
                actions
                    .iter()
                    .filter(|action| {
                        action.team_run_id == id && action.member_run_id == member_run_id
                    })
                    .max_by_key(|action| action.seq)
            };
            let unacked_messages = team_run_unacknowledged_message_count(store, &id)?;
            let supervisor = store.latest_team_supervisor_lease(&id)?;
            let supervisor_current = supervisor.as_ref().is_some_and(is_supervisor_current);
            #[cfg(unix)]
            let node_daemon = execution_space::firm_home()
                .ok()
                .and_then(|home| {
                    supervisor_daemon::daemon_status_via_socket(&home, &run.execution_node_id)
                })
                .and_then(|response| serde_json::from_str::<serde_json::Value>(&response).ok());
            #[cfg(not(unix))]
            let node_daemon: Option<serde_json::Value> = None;
            if json {
                let members: Vec<serde_json::Value> = member_runs
                    .iter()
                    .map(|member| {
                        serde_json::json!({
                            "member_run": member,
                            "latest_action": latest_action_of(&member.id),
                        })
                    })
                    .collect();
                print_json(&serde_json::json!({
                    "team_run": run,
                    "members": members,
                    "unacked_messages": unacked_messages,
                    "supervisor": {
                        "lease": supervisor,
                        "current": supervisor_current,
                        "owner_pid_alive": supervisor
                            .as_ref()
                            .map(|lease| pid_exists_libc(lease.owner_process_id))
                            .unwrap_or(false),
                        "heartbeat_age_s": supervisor
                            .as_ref()
                            .map(|lease| {
                                current_unix_ms_u64()
                                    .saturating_sub(lease.heartbeat_unix_ms)
                                    / 1000
                            })
                            .unwrap_or(0),
                    },
                    "node_daemon": node_daemon,
                }))?;
                if unacked_messages > 0 && run.host_thread_id.is_none() {
                    eprintln!(
                        "[WARNING] {unacked_messages} unacked message(s) queued without host binding — member messages may be waiting silently.\n\
                         Bind with: harness team-run bind-host --id {} --surface <surface> --thread-id <thread-id>",
                        run.id
                    );
                }
            } else {
                println!(
                    "{}\t{}\t{}",
                    run.id,
                    serde_snake_label(&run.status),
                    run.objective
                );
                for member in &member_runs {
                    let last = match latest_action_of(&member.id) {
                        Some(action) => format!("[{}] {}", action.action_type, action.title),
                        None => "-".to_string(),
                    };
                    println!(
                        "  {} ({}/{})\t{}\tlast: {}",
                        member.name,
                        member.role,
                        member.provider,
                        serde_snake_label(&member.status),
                        last
                    );
                }
                println!("unacked_messages (canonical Host deliveries): {unacked_messages}");
                if unacked_messages > 0 && run.host_thread_id.is_none() {
                    eprintln!(
                        "[WARNING] {unacked_messages} unacked message(s) queued without host binding — member messages may be waiting silently.\n\
                         Bind with: harness team-run bind-host --id {} --surface <surface> --thread-id <thread-id>",
                        run.id
                    );
                }
                match supervisor {
                    Some(lease) => {
                        let pid_alive = pid_exists_libc(lease.owner_process_id);
                        let heartbeat_age_s =
                            current_unix_ms_u64().saturating_sub(lease.heartbeat_unix_ms) / 1000;
                        println!(
                            "supervisor: {}\tgen={}\tstatus={}\tcurrent={}\towner_pid={}\tpid_alive={}\thb_age={}s\texpires_unix_ms={}",
                            lease.supervisor_id,
                            lease.generation,
                            serde_snake_label(&lease.status),
                            supervisor_current,
                            lease.owner_process_id,
                            pid_alive,
                            heartbeat_age_s,
                            lease.expires_unix_ms
                        );
                        if !supervisor_current || !pid_alive {
                            let ready = works
                                .iter()
                                .filter(|work| work.is_claim_ready(&works))
                                .count();
                            eprintln!(
                                "[WARNING] no live supervisor: {} ready work(s) undelivered. Run: harness team-run start --id {}",
                                ready, run.id
                            );
                        }
                    }
                    None => {
                        println!("supervisor: none");
                        let ready = works
                            .iter()
                            .filter(|work| work.is_claim_ready(&works))
                            .count();
                        eprintln!(
                            "[WARNING] no supervisor lease. {} ready work(s) undelivered. Run: harness team-run start --id {}",
                            ready, run.id
                        );
                    }
                }
                if let Some(status) = node_daemon {
                    let managed = status["runs"].as_array().map(Vec::len).unwrap_or_default();
                    println!("node_daemon: running\tmanaged_runs={managed}");
                } else {
                    println!("node_daemon: absent");
                }
            }
        }
        "host-inbox" => {
            let surface = required(args, "--surface")?;
            let thread_id = required(args, "--thread-id")?;
            let inbox =
                host_inbox_for_native_thread(store, &surface, &thread_id, has_flag(args, "--all"))?;
            if json {
                print_json(&inbox)?;
            } else {
                for entry in &inbox {
                    let run_id = entry["team_run_id"].as_str().unwrap_or("?");
                    for message in entry["messages"].as_array().into_iter().flatten() {
                        println!(
                            "{}\t{}\tfrom={}\t{}",
                            run_id,
                            message["id"].as_str().unwrap_or("?"),
                            message["sender_runtime_id"].as_str().unwrap_or("?"),
                            message["body"]
                                .as_str()
                                .unwrap_or_default()
                                .lines()
                                .next()
                                .unwrap_or_default()
                        );
                    }
                    for attention in entry["attentions"].as_array().into_iter().flatten() {
                        println!(
                            "{}\t{}\tkind={}\twork={}\tmember={}\tstatus={}",
                            run_id,
                            attention["id"].as_str().unwrap_or("?"),
                            attention["kind"].as_str().unwrap_or("?"),
                            attention["work_id"].as_str().unwrap_or("?"),
                            attention["member_run_id"].as_str().unwrap_or("?"),
                            attention["status"].as_str().unwrap_or("?"),
                        );
                    }
                }
            }
        }
        "bind-host" => {
            let id = required(args, "--id")?;
            let surface = required(args, "--surface")?;
            let thread_id = required(args, "--thread-id")?;
            let result = bind_host_with_validator(
                store,
                &id,
                &surface,
                &thread_id,
                checked_host_binding_lease_ttl_ms(args)?,
                &RuntimeHostSessionValidator::default(),
                current_unix_ms_u64(),
            )?;
            if let Some(warning) = result.validation_warning.as_deref() {
                eprintln!("[WARNING] {warning}");
            }
            if json {
                print_json(&serde_json::json!({
                    "team_run": result.run,
                    "host_binding_lease": result.lease,
                    "validation_warning": result.validation_warning,
                }))?;
            } else {
                println!(
                    "{}\t{}:{}\tlease={}",
                    result.run.id,
                    result.run.host_surface,
                    result.run.host_thread_id.as_deref().unwrap_or("?"),
                    result
                        .lease
                        .as_ref()
                        .map(|lease| lease.lease_id.as_str())
                        .unwrap_or("unleased")
                );
            }
        }
        "host-lease-status" => {
            let id = required(args, "--id")?;
            latest_team_run(store, &id)?;
            let now = current_unix_ms_u64();
            let latest = store.latest_host_binding_lease(&id)?;
            let effective = store.effective_host_binding_lease_at(&id, now)?;
            if json {
                print_json(&serde_json::json!({
                    "team_run_id": id,
                    "observed_unix_ms": now,
                    "latest": latest,
                    "effective": effective,
                }))?;
            } else if let Some(lease) = latest {
                println!(
                    "{}\t{:?}\towner={}\tlease={}\tgeneration={}\teffective={}",
                    lease.team_run_id,
                    lease.owner_kind,
                    lease.owner_id,
                    lease.lease_id,
                    lease.generation,
                    effective.is_some()
                );
            } else {
                println!("{id}\tunleased");
            }
        }
        "renew-host-lease" => {
            let expected = exact_host_binding_lease_from_args(store, args)?;
            let renewed = store_conflict_as_usage(store.renew_host_binding_lease(
                &expected,
                current_unix_ms_u64(),
                checked_host_binding_lease_ttl_ms(args)?,
            ))?;
            if json {
                print_json(&renewed)?;
            } else {
                println!(
                    "{}\tlease={}\tgeneration={}\texpires_unix_ms={}",
                    renewed.team_run_id,
                    renewed.lease_id,
                    renewed.generation,
                    renewed.expires_unix_ms
                );
            }
        }
        "release-host-lease" => {
            let expected = exact_host_binding_lease_from_args(store, args)?;
            let released = store_conflict_as_usage(
                store.release_host_binding_lease(&expected, current_unix_ms_u64()),
            )?;
            if json {
                print_json(&released)?;
            } else {
                println!(
                    "{}\tlease={}\tgeneration={}\treleased",
                    released.team_run_id, released.lease_id, released.generation
                );
            }
        }
        "inbox" => {
            let member_run_id = required(args, "--member-run-id")?;
            let team_run_id = required(args, "--id")?;
            require_external_interactive_inbox_scope(store, &team_run_id, &member_run_id)?;
            let messages =
                team_run_inbox(store, &team_run_id, &member_run_id, has_flag(args, "--all"))?;
            if json {
                print_json(&messages)?;
            } else {
                for message in &messages {
                    let delivery = message
                        .deliveries
                        .iter()
                        .find(|delivery| delivery.member_id == member_run_id)
                        .map(|delivery| serde_snake_label(&delivery.status))
                        .unwrap_or_else(|| "unknown".to_string());
                    println!(
                        "{}\t{}\tfrom={}\t{}\t{}",
                        message.id,
                        team_message_kind_label(&message.kind),
                        message.sender_runtime_id,
                        delivery,
                        message.body.lines().next().unwrap_or_default()
                    );
                }
            }
        }
        "ack" => {
            return Err(CliError::Usage(
                "RETIRED_WRITE_AUTHORITY: team-run ack cannot authenticate the recipient session; acknowledge canonical MessageDelivery through the target NodeDaemon"
                    .to_string(),
            ));
        }
        "reconcile-delivery" => {
            return Err(CliError::Usage(
                "RETIRED_WRITE_AUTHORITY: team-run reconcile-delivery cannot supply NodeDaemon delivery authority; use canonical target-NodeDaemon reconciliation"
                    .to_string(),
            ));
        }
        "send" => {
            return Err(CliError::Usage(
                "RETIRED_WRITE_AUTHORITY: team-run send cannot select a sender identity; use an authenticated AgentFirm Role Action or source NodeDaemon RuntimeCommand"
                    .to_string(),
            ));
        }
        "add-member" => {
            let mut member = parse_team_member_spec(&required(args, "--member")?)?;
            member.effort = value(args, "--effort");
            member.service_tier = value(args, "--service-tier");
            let initial_work = value(args, "--initial-work");
            let (run, member, work) = add_team_run_member(
                store,
                resolved.context.as_ref(),
                &required(args, "--id")?,
                &member,
                initial_work.as_deref(),
            )?;
            print_json(&serde_json::json!({
                "team_run": run,
                "member_run": member,
                "work": work,
            }))?;
        }
        "rename-member" => print_json(&rename_team_run_member(
            store,
            &required(args, "--id")?,
            &required(args, "--member-run-id")?,
            &required(args, "--name")?,
        )?)?,
        "deactivate-member" => print_json(&deactivate_team_run_member(
            store,
            &required(args, "--id")?,
            &required(args, "--member-run-id")?,
            &required(args, "--reason")?,
        )?)?,
        "close-member" => print_json(&close_team_member_value(
            store,
            &required(args, "--id")?,
            &required(args, "--member-run-id")?,
            &serde_json::json!({
                "reason": required(args, "--reason")?,
                "requested_by": value(args, "--requested-by").unwrap_or_else(|| "host".to_string()),
            }),
        )?)?,
        "reopen-member" => print_json(&reopen_team_member_value(
            store,
            &required(args, "--id")?,
            &required(args, "--member-run-id")?,
            &serde_json::json!({
                "reason": value(args, "--reason").unwrap_or_else(|| "Host reopened member".to_string()),
                "reopened_by": value(args, "--reopened-by").unwrap_or_else(|| "host".to_string()),
                "host_runtime_mode": value(args, "--host-runtime-mode"),
                "execution_mode": value(args, "--execution-mode"),
                "host_thread_id": value(args, "--host-thread-id"),
            }),
        )?)?,
        "start" => {
            // Foreground orchestration: this process is the WRITER driving
            // member sessions; `harness serve` stays the read/broadcast side.
            let id = required(args, "--id")?;
            let run = latest_team_run(store, &id)?;
            // L1: auto-bind from star-harness hook env when unambiguous.
            if run.host_thread_id.is_none() {
                let env_surface = std::env::var("STAR_HARNESS_HOST_SURFACE")
                    .ok()
                    .filter(|s| !s.trim().is_empty());
                let env_thread_id = std::env::var("STAR_HARNESS_HOST_THREAD_ID")
                    .ok()
                    .filter(|s| !s.trim().is_empty());
                match (&env_surface, &env_thread_id) {
                    (Some(_), None) => {
                        eprintln!("[WARNING] STAR_HARNESS_HOST_SURFACE is set but STAR_HARNESS_HOST_THREAD_ID is missing — refusing to auto-bind");
                    }
                    (None, Some(_)) => {
                        eprintln!("[WARNING] STAR_HARNESS_HOST_THREAD_ID is set but STAR_HARNESS_HOST_SURFACE is missing — refusing to auto-bind");
                    }
                    (Some(surface), Some(thread_id)) => {
                        let mut next = run.clone();
                        next.host_surface = canonical_surface(surface).to_string();
                        next.host_thread_id = Some(thread_id.clone());
                        next.updated_at = now_string();
                        if store.compare_and_append_team_run(&run, &next).is_ok() {
                            eprintln!("[star-harness] Auto-bound host to {surface}:{thread_id}");
                        }
                    }
                    (None, None) => {}
                }
            }
            // Re-read after potential auto-bind so the warning sees fresh state.
            let current = latest_team_run(store, &id)?;
            if current.host_thread_id.is_none() {
                eprintln!(
                    "[WARNING] Team run {id} has no host binding — member messages will queue silently.\n\
                     Bind with: harness team-run bind-host --id {id} --surface <surface> --thread-id <thread-id>"
                );
            } else {
                let (_lease, warning) = acquire_validated_interactive_host_lease(
                    store,
                    &current,
                    checked_host_binding_lease_ttl_ms(args)?,
                    &RuntimeHostSessionValidator::default(),
                    current_unix_ms_u64(),
                )?;
                if let Some(warning) = warning {
                    eprintln!("[WARNING] {warning}");
                }
            }
            let max_concurrency = value(args, "--max-concurrency")
                .and_then(|raw| raw.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(TEAM_RUN_START_DEFAULT_CONCURRENCY);
            let idle_timeout_s = value(args, "--idle-timeout-s")
                .and_then(|raw| raw.parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(kimi_acp::DEFAULT_PROMPT_IDLE_TIMEOUT_SECS);
            team_run_start(
                store,
                resolved,
                &id,
                max_concurrency,
                Duration::from_secs(idle_timeout_s),
            )?;
        }
        "events" => {
            let id = required(args, "--id")?;
            let after_seq = value(args, "--after-seq")
                .and_then(|raw| raw.parse::<u64>().ok())
                .unwrap_or(0);
            let mut events: Vec<TeamRunEvent> = store
                .current_team_run_events(&id)?
                .into_iter()
                .filter(|event| event.seq > after_seq)
                .collect();
            events.sort_by_key(|event| event.seq);
            if json {
                print_json(&events)?;
            } else {
                for event in &events {
                    println!(
                        "seq={}\t{}\t{}:{}\t{}\t{}",
                        event.seq,
                        serde_snake_label(&event.source_kind),
                        event.entity_type,
                        event.entity_id,
                        event.operation,
                        event.summary
                    );
                }
            }
        }
        // Blocking form of `events --after-seq`. Without it the Host has no way
        // to await member progress: the subcommand surface had no wait/follow
        // and hook delivery only fires at turn boundaries, so a long Host turn
        // could only poll. Measured on run 019fa80d: 35 `status` polls, median
        // gap 58 s, and a 25-minute window with 27 polls and zero patches —
        // each poll costing a full model round-trip.
        "wait" => {
            let id = required(args, "--id")?;
            let mut after_seq = value(args, "--after-seq")
                .and_then(|raw| raw.parse::<u64>().ok())
                .unwrap_or(0);
            let timeout_secs = value(args, "--timeout-secs")
                .and_then(|raw| raw.parse::<u64>().ok())
                .unwrap_or(600);
            let poll_ms = value(args, "--poll-ms")
                .and_then(|raw| raw.parse::<u64>().ok())
                .unwrap_or(500)
                .clamp(50, 10_000);
            // A caller that passes no cursor means "wait for what happens
            // next", not "replay this run's whole history".
            if value(args, "--after-seq").is_none() {
                after_seq = store
                    .current_team_run_events(&id)?
                    .into_iter()
                    .map(|event| event.seq)
                    .max()
                    .unwrap_or(0);
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
            loop {
                let mut events: Vec<TeamRunEvent> = store
                    .current_team_run_events(&id)?
                    .into_iter()
                    .filter(|event| event.seq > after_seq)
                    .collect();
                events.sort_by_key(|event| event.seq);
                if !events.is_empty() {
                    let next_after_seq = events.last().map(|event| event.seq).unwrap_or(after_seq);
                    if json {
                        print_json(&serde_json::json!({
                            "timed_out": false,
                            "after_seq": after_seq,
                            "next_after_seq": next_after_seq,
                            "events": events,
                        }))?;
                    } else {
                        for event in &events {
                            println!(
                                "seq={}\t{}\t{}:{}\t{}\t{}",
                                event.seq,
                                serde_snake_label(&event.source_kind),
                                event.entity_type,
                                event.entity_id,
                                event.operation,
                                event.summary
                            );
                        }
                        println!("next_after_seq={next_after_seq}");
                    }
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    if json {
                        print_json(&serde_json::json!({
                            "timed_out": true,
                            "after_seq": after_seq,
                            "next_after_seq": after_seq,
                            "events": [],
                        }))?;
                    } else {
                        println!("timed_out\tnext_after_seq={after_seq}");
                    }
                    break;
                }
                // Never sleep past the deadline. Sleeping a full poll interval
                // after the check made the real bound `timeout_secs + poll_ms`:
                // measured 10.08 s for `--timeout-secs 1 --poll-ms 10000`.
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                std::thread::sleep(remaining.min(Duration::from_millis(poll_ms)));
            }
        }
        "board-summary" => {
            let id = required(args, "--id")?;
            println!("{}", team_run_board_summary_text(store, &id)?);
        }
        "recover" => {
            let id = required(args, "--id")?;
            let report = team_run_recover(store, &id, json)?;
            if json {
                print_json(&report)?;
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown team-run command: {other}"
            )))
        }
    }
    Ok(())
}

/// Read one run-scoped Agent Team member without copying provider-native
/// transcript/tool history into Harness. The projection joins the durable
/// coordination facts needed by a Host or operator; `native_session` remains
/// a locator to the provider-owned execution truth.
pub(super) fn member_run_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "member-run show|open-native")?;
    match args[0].as_str() {
        "show" => {
            let detail = member_run_detail_json(store, &required(args, "--id")?)?;
            if has_flag(args, "--json") {
                print_json(&detail)?;
            } else {
                let member = detail
                    .get("member_run")
                    .and_then(|value| {
                        serde_json::from_value::<ProviderRuntimeProjection>(value.clone()).ok()
                    })
                    .ok_or_else(|| CliError::Usage("invalid member detail projection".into()))?;
                let inbox_count = detail
                    .pointer("/mailbox/inbox")
                    .and_then(|value| value.as_array())
                    .map(Vec::len)
                    .unwrap_or(0);
                let outbox_count = detail
                    .pointer("/mailbox/outbox")
                    .and_then(|value| value.as_array())
                    .map(Vec::len)
                    .unwrap_or(0);
                println!(
                    "{}\t{}\t{}/{}\t{}",
                    member.id,
                    serde_snake_label(&member.status),
                    member.provider,
                    member
                        .provider_profile
                        .as_ref()
                        .map(|profile| profile.execution_mode.as_str())
                        .unwrap_or("unknown"),
                    member.name,
                );
                println!(
                    "team_run={}\tinbox={inbox_count}\toutbox={outbox_count}",
                    member.team_run_id
                );
                println!(
                    "agent_member_id={}\tidentity_link=explicit",
                    member.agent_member_id
                );
                if let Some(session) = member.native_session.as_ref() {
                    println!(
                        "native_session={}\tlocator_kind={}",
                        session.native_session_id, session.native_locator_kind
                    );
                }
            }
        }
        "open-native" => {
            let member_run_id = required(args, "--id")?;
            let member = latest_member_runs_in_append_order(store)?
                .into_iter()
                .find(|member| member.id == member_run_id)
                .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
            let mut target = native_session_open_target(&member)?;
            let print_only = has_flag(args, "--print-only");
            if !print_only {
                open_native_session_target(
                    target["uri"]
                        .as_str()
                        .expect("native-session target always has a URI"),
                )?;
            }
            target["opened"] = serde_json::Value::Bool(!print_only);
            if has_flag(args, "--json") {
                print_json(&target)?;
            } else if print_only {
                println!("{}", target["uri"].as_str().unwrap_or_default());
            } else {
                println!(
                    "opened {}\tdesktop_session={}",
                    target["uri"].as_str().unwrap_or_default(),
                    target["desktop_session_id"].as_str().unwrap_or_default()
                );
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown member-run command: {other}"
            )))
        }
    }
    Ok(())
}

/// Resolve an explicit provider-owned UI target without changing the native
/// session binding. Claude Desktop imports SDK/CLI sessions through this deep
/// link and deterministically exposes them as `local_<native-id>`.
pub(super) fn native_session_open_target(
    member: &ProviderRuntimeProjection,
) -> CliResult<serde_json::Value> {
    let session = member.native_session.as_ref().ok_or_else(|| {
        CliError::Usage(format!(
            "member run {} has no bound provider-native session",
            member.id
        ))
    })?;
    if member.provider != "claude" || session.provider != "claude" {
        return Err(CliError::Usage(format!(
            "member run {} uses provider {}; open-native currently supports only Claude Agent SDK sessions",
            member.id, member.provider
        )));
    }
    if session.execution_mode != "claude_agent_sdk" {
        return Err(CliError::Usage(format!(
            "member run {} uses Claude mode {}; Desktop import is verified only for claude_agent_sdk",
            member.id, session.execution_mode
        )));
    }
    let native_id = session.native_session_id.trim();
    if native_id.is_empty()
        || !native_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CliError::Usage(format!(
            "member run {} has an unsafe native session id; refusing to build a Desktop deep link",
            member.id
        )));
    }
    Ok(serde_json::json!({
        "member_run_id": member.id,
        "provider": "claude",
        "execution_mode": session.execution_mode,
        "native_session_id": native_id,
        "uri": format!("claude://resume?session={native_id}"),
        "desktop_session_id": format!("local_{native_id}"),
        "ownership": "provider_native",
        "concurrency_warning": "Use Claude Desktop for observation while Harness drives this Member; simultaneous SDK and Desktop generation is not verified.",
    }))
}

pub(super) fn open_native_session_target(uri: &str) -> CliResult<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open").arg(uri).status()?;
        if !status.success() {
            return Err(CliError::Usage(format!(
                "macOS could not open the provider-native session target: {uri}"
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(CliError::Usage(format!(
            "opening provider-native session targets is currently supported only on macOS; inspect it with --print-only: {uri}"
        )))
    }
}

pub(super) fn member_run_detail_json(
    store: &HarnessStore,
    member_run_id: &str,
) -> CliResult<serde_json::Value> {
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    let team_run = latest_team_run(store, &member.team_run_id)?;

    let messages = canonical_team_messages_for_run(store, &member.team_run_id)?;
    let inbox = messages
        .iter()
        .filter(|message| {
            message
                .recipient_runtime_ids
                .iter()
                .any(|id| id == member_run_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let outbox = messages
        .iter()
        .filter(|message| message.sender_runtime_id == member_run_id)
        .cloned()
        .collect::<Vec<_>>();
    let works = store
        .latest_works()?
        .into_iter()
        .filter(|work| work.team_run_id == member.team_run_id)
        .filter(|work| {
            work.active_member_run_id.as_deref() == Some(member_run_id)
                || work.owner_member_id.as_deref() == Some(member.agent_member_id.as_str())
                || work
                    .eligible_member_ids
                    .iter()
                    .any(|id| id == member_run_id || id == &member.agent_member_id)
        })
        .collect::<Vec<_>>();
    let latest_handoff = outbox
        .iter()
        .rev()
        .find(|message| message.kind == ProviderDispatchIntent::Message);
    let actions = visible_member_actions_in_append_order(store)?
        .into_iter()
        .filter(|action| {
            action.team_run_id == member.team_run_id && action.member_run_id == member_run_id
        })
        .collect::<Vec<_>>();
    let supervisor = store.latest_team_supervisor_lease(&member.team_run_id)?;
    let close_request = store.latest_team_member_close_request(member_run_id)?;
    let actionable_inbox = if member.coordination_is_active() {
        inbox
            .iter()
            .filter(|message| {
                message.deliveries.iter().any(|delivery| {
                    delivery.member_id == member_run_id
                        && matches!(
                            delivery.status,
                            TeamDeliveryStatus::Queued | TeamDeliveryStatus::Delivered
                        )
                })
            })
            .count()
    } else {
        0
    };

    Ok(serde_json::json!({
        "member_run": member,
        "team_run": team_run,
        "mission_id": team_run_mission_id(store, &team_run)?,
        "agent_team_id": team_run.agent_team_id,
        "works": works,
        "workspace": member.provider_environment_observation,
        "provider_profile": member.provider_profile,
        "native_session": member.native_session,
        "mailbox": {
            "inbox": inbox,
            "outbox": outbox,
            "actionable_inbox_count": actionable_inbox,
        },
        "supervisor": supervisor,
        "close_request": close_request,
        "actions": actions,
        "latest_handoff": latest_handoff,
    }))
}
