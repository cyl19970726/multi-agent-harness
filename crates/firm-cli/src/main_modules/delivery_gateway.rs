use super::*;

pub(super) fn deliver_agent_messages_value(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    options: DeliveryOptions,
) -> CliResult<serde_json::Value> {
    let DeliveryOptions {
        agent_id,
        message_filter,
        dry_run,
        start_runtime,
        timeout_ms,
    } = options;
    // Project Binding is independent from the Execution Space that owns these
    // messages. Prefer the explicit request/CLI binding; only compatibility
    // stores may infer a project from their historical metadata.
    let project = project_context
        .cloned()
        .unwrap_or_else(|| default_project_context(store));
    let mut member = latest_member(store, &agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    let mut runtime = match member.provider_runtime_id.as_deref() {
        Some(runtime_id) => latest_runtime(store, runtime_id)?,
        None => None,
    };
    if runtime
        .as_ref()
        .is_some_and(|runtime| !runtime_is_alive(runtime))
    {
        append_harness_runtime_control_fact(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            None,
            "runtime_stale",
            "Runtime pid or socket is not healthy",
            None,
        )?;
        mark_running_delivery_attempts_terminal(
            store,
            &member.id,
            ProviderExecutionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )?;
        runtime = None;
        member = latest_member(store, &agent_id)?;
        ensure_member_accepts_delivery(&member)?;
    }
    if has_unresolved_delivery_attempt(store, &member.id)? {
        return Err(CliError::Usage(format!(
            "agent {} still has an unresolved provider turn; ingest a terminal provider event or close the runtime before delivering more messages",
            member.id
        )));
    }
    let queued: Vec<RegistryMessage> = latest_messages_in_append_order(store)?
        .into_iter()
        .filter(|message| message.to_agent_id.as_deref() == Some(agent_id.as_str()))
        .filter(|message| message.delivery_status == RegistryDeliveryStatus::Queued)
        .filter(|message| {
            message_filter
                .as_ref()
                .is_none_or(|message_id| message.id == *message_id)
        })
        .collect();

    if queued.is_empty() {
        return Ok(serde_json::json!({
            "agent_member_id": agent_id,
            "delivered": [],
            "note": "no queued messages"
        }));
    }

    let mut results = Vec::new();
    for message in queued {
        member = latest_member(store, &agent_id)?;
        ensure_member_accepts_delivery(&member)?;
        let delivery_id = generated_id("delivery");
        let claimed_message = match claim_message_for_delivery(
            store,
            &member,
            runtime.as_ref(),
            &message,
            &delivery_id,
        )? {
            Some(message) => message,
            None => continue,
        };

        member.status = ProviderLaunchStatus::Running;
        member.current_task_id = claimed_message.task_id.clone();
        member.last_seen_at = Some(now_string());
        store.append_member(&member)?;
        append_harness_runtime_control_fact(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            claimed_message.task_id.as_deref(),
            "delivery_claimed",
            "Claimed message delivery before provider side effects",
            None,
        )?;

        let delivery = if dry_run {
            let provider_thread_id = member
                .provider_thread_id
                .clone()
                .or_else(|| Some(format!("dry-thread-{}", member.id)));
            let provider_turn_id = Some(format!("dry-turn-{}", claimed_message.id));
            record_claimed_delivery_terminal(
                store,
                &delivery_id,
                &claimed_message,
                ProviderExecutionStatus::Succeeded,
                provider_thread_id.clone(),
                provider_turn_id.clone(),
                Some(MessageTerminalSource::DryRun),
                "dry-run delivery completed",
                Some("dry-run"),
                Some(0),
            )?;
            DeliveryOutcome {
                status: ProviderExecutionStatus::Succeeded,
                native_session: None,
                provider_thread_id,
                provider_turn_id,
                terminal_source: Some(MessageTerminalSource::DryRun),
                provider_request_id: None,
                exit_code: Some(0),
                tokens: None,
                cost_usd: None,
                model: None,
                structured: None,
                response_text: None,
                summary: "dry-run delivery completed".into(),
            }
        } else {
            let start_error = if runtime.is_none() && start_runtime {
                match start_agent_runtime(store, &agent_id) {
                    Ok(started_member) => {
                        member = started_member;
                        runtime = member
                            .provider_runtime_id
                            .as_deref()
                            .and_then(|runtime_id| {
                                latest_runtime(store, runtime_id).ok().flatten()
                            });
                        None
                    }
                    Err(error) => Some(error.to_string()),
                }
            } else {
                None
            };
            if let Some(error) = start_error {
                let summary = format!(
                    "{} runtime start failed after claim: {error}",
                    member.provider
                );
                record_claimed_delivery_terminal(
                    store,
                    &delivery_id,
                    &claimed_message,
                    ProviderExecutionStatus::Failed,
                    member.provider_thread_id.clone(),
                    None,
                    Some(MessageTerminalSource::Failed),
                    &summary,
                    None,
                    Some(1),
                )?;
                DeliveryOutcome {
                    status: ProviderExecutionStatus::Failed,
                    native_session: None,
                    provider_thread_id: member.provider_thread_id.clone(),
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                    provider_request_id: None,
                    exit_code: Some(1),
                    tokens: None,
                    cost_usd: None,
                    model: None,
                    structured: None,
                    response_text: None,
                    summary,
                }
            } else if runtime.is_none() {
                let summary = format!("agent {agent_id} has no running provider runtime");
                record_claimed_delivery_terminal(
                    store,
                    &delivery_id,
                    &claimed_message,
                    ProviderExecutionStatus::Failed,
                    member.provider_thread_id.clone(),
                    None,
                    Some(MessageTerminalSource::Failed),
                    &summary,
                    None,
                    Some(1),
                )?;
                DeliveryOutcome {
                    status: ProviderExecutionStatus::Failed,
                    native_session: None,
                    provider_thread_id: member.provider_thread_id.clone(),
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                    provider_request_id: None,
                    exit_code: Some(1),
                    tokens: None,
                    cost_usd: None,
                    model: None,
                    structured: None,
                    response_text: None,
                    summary,
                }
            } else {
                let runtime = runtime.clone().expect("runtime checked");
                run_provider_delivery(
                    store,
                    &member,
                    &runtime,
                    &claimed_message,
                    &delivery_id,
                    timeout_ms,
                    &project,
                )?
            }
        };

        let delivery_unresolved = provider_status_blocks_delivery(&delivery.status);
        let mut delivered_message = latest_message(store, &claimed_message.id)?;
        delivered_message.delivery_status = message_status_for_delivery(&delivery.status);
        delivered_message.delivery = Some(RegistryDeliveryAttempt {
            delivery_id: Some(delivery_id.clone()),
            execution_status: Some(delivery.status.clone()),
            native_session: delivery.native_session.clone(),
            started_at: claimed_message
                .delivery
                .as_ref()
                .and_then(|delivery| delivery.started_at.clone()),
            provider_request_id: delivery.provider_request_id.clone(),
            provider_thread_id: delivery.provider_thread_id.clone(),
            provider_turn_id: delivery.provider_turn_id.clone(),
            terminal_source: delivery.terminal_source.clone(),
            delivered_at: Some(now_string()),
            last_error: delivery_error_message(&delivery.status, &delivery.summary),
        });
        store.append_message(&delivered_message)?;
        if let Some(thread_id) = delivery.provider_thread_id.clone() {
            member.provider_thread_id = Some(thread_id);
        }
        if let Some(native_session) = delivery.native_session.clone() {
            member.native_session = Some(native_session);
        }
        if let Some(mut runtime_value) = runtime.clone() {
            runtime_value.health.delivery_probe = Some(match &delivery.status {
                ProviderExecutionStatus::Succeeded => {
                    format!(
                        "pass: {}",
                        delivery
                            .terminal_source
                            .as_ref()
                            .map(terminal_source_label)
                            .unwrap_or_else(|| "unknown terminal source".into())
                    )
                }
                ProviderExecutionStatus::Running => format!("pending: {}", delivery.summary),
                ProviderExecutionStatus::Stale => format!("stale: {}", delivery.summary),
                _ => format!("failed: {}", delivery.summary),
            });
            runtime_value.health.checked_at = Some(now_string());
            runtime_value.last_event_at = Some(now_string());
            store.append_runtime(&runtime_value)?;
            runtime = Some(runtime_value);
        }
        if delivery.status == ProviderExecutionStatus::Running {
            member.status = ProviderLaunchStatus::Running;
            member.current_task_id = delivered_message.task_id.clone();
        } else if delivery.status == ProviderExecutionStatus::Stale {
            member.status = ProviderLaunchStatus::Stale;
            member.current_task_id = delivered_message.task_id.clone();
        } else {
            member.status = ProviderLaunchStatus::Idle;
            member.current_task_id = None;
        }
        member.last_seen_at = Some(now_string());
        store.append_member(&member)?;
        append_harness_runtime_control_fact(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            delivered_message.task_id.as_deref(),
            match &delivery.status {
                ProviderExecutionStatus::Succeeded => "delivery_delivered",
                ProviderExecutionStatus::Running => "delivery_running",
                ProviderExecutionStatus::Stale => "delivery_stale",
                _ => "delivery_failed",
            },
            &delivery.summary,
            None,
        )?;

        results.push(serde_json::json!({
            "message_id": delivered_message.id,
            "delivery_status": delivered_message.delivery_status,
            "provider_status": delivery.status,
            "provider_thread_id": member.provider_thread_id,
            "provider_turn_id": delivery.provider_turn_id,
            "terminal_source": delivery.terminal_source,
            "provider_request_id": delivery.provider_request_id,
            "exit_code": delivery.exit_code,
            "tokens": delivery.tokens.map(TokenUsage::into_json),
            "cost_usd": delivery.cost_usd,
            "model": delivery.model,
            "structured": delivery.structured
        }));
        if delivery_unresolved {
            break;
        }
    }

    Ok(serde_json::json!({
        "agent_member_id": agent_id,
        "delivered": results
    }))
}
