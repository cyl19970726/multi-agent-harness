use super::*;

#[derive(Debug, Clone)]
pub(super) struct GatewayOptions {
    pub(super) dry_run: bool,
    pub(super) start_runtime: bool,
    pub(super) timeout_ms: u64,
    pub(super) claim_ttl_ms: u64,
}

pub(super) fn provider_gateway_tick_value(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    options: GatewayOptions,
) -> CliResult<serde_json::Value> {
    let expired_claims = expire_safe_delivery_claims_value(store, options.claim_ttl_ms)?;
    let mut agent_ids = Vec::new();
    for message in latest_messages_in_append_order(store)? {
        if message.delivery_status == RegistryDeliveryStatus::Queued {
            if let Some(agent_id) = message.to_agent_id {
                if !agent_ids.contains(&agent_id) {
                    agent_ids.push(agent_id);
                }
            }
        }
    }
    let mut results = Vec::new();
    for agent_id in agent_ids {
        match deliver_agent_messages_value(
            store,
            project_context,
            DeliveryOptions {
                agent_id: agent_id.clone(),
                message_filter: None,
                dry_run: options.dry_run,
                start_runtime: options.start_runtime,
                timeout_ms: options.timeout_ms,
            },
        ) {
            Ok(result) => results.push(serde_json::json!({
                "agent_member_id": agent_id,
                "ok": true,
                "result": result
            })),
            Err(error) => results.push(serde_json::json!({
                "agent_member_id": agent_id,
                "ok": false,
                "error": error.to_string()
            })),
        }
    }
    Ok(serde_json::json!({
        "generated_at": now_string(),
        "agent_count": results.len(),
        "expired_claims": expired_claims,
        "results": results
    }))
}

pub(super) fn expire_safe_delivery_claims_value(
    store: &HarnessStore,
    claim_ttl_ms: u64,
) -> CliResult<Vec<serde_json::Value>> {
    if claim_ttl_ms == 0 {
        return Ok(Vec::new());
    }
    let now_ms = current_unix_ms();
    let messages = latest_messages(store)?;
    let mut expired = Vec::new();
    for message in messages.values() {
        if message.delivery_status != RegistryDeliveryStatus::Acknowledged {
            continue;
        }
        let Some(delivery) = message.delivery.as_ref() else {
            continue;
        };
        if delivery.execution_status != Some(ProviderExecutionStatus::Running)
            || delivery.provider_request_id.is_some()
            || delivery.provider_turn_id.is_some()
        {
            continue;
        }
        let Some(started_ms) = delivery.started_at.as_deref().and_then(parse_unix_ms) else {
            continue;
        };
        if now_ms.saturating_sub(started_ms) < u128::from(claim_ttl_ms) {
            continue;
        }
        let Some(agent_id) = message.to_agent_id.as_deref() else {
            continue;
        };
        let delivery_id = delivery.delivery_id.as_deref();
        match retry_delivery_value(
            store,
            agent_id,
            &message.id,
            delivery_id,
            "gateway expired unreconciled pre-provider delivery claim",
            false,
        ) {
            Ok(result) => expired.push(serde_json::json!({"ok": true, "result": result})),
            Err(error) => expired.push(serde_json::json!({
                "ok": false,
                "delivery_id": delivery_id,
                "message_id": message.id,
                "error": error.to_string()
            })),
        }
    }
    Ok(expired)
}

#[derive(Debug)]
pub(super) struct DeliveryOutcome {
    pub(super) status: ProviderExecutionStatus,
    pub(super) native_session: Option<NativeSessionRef>,
    pub(super) provider_thread_id: Option<String>,
    pub(super) provider_turn_id: Option<String>,
    pub(super) terminal_source: Option<MessageTerminalSource>,
    pub(super) provider_request_id: Option<String>,
    pub(super) exit_code: Option<i32>,
    pub(super) tokens: Option<TokenUsage>,
    pub(super) cost_usd: Option<f64>,
    pub(super) model: Option<String>,
    pub(super) structured: Option<serde_json::Value>,
    /// Provider-authored response retained only for the current in-process
    /// consumer. It is never copied into RegistryMessage, Evidence, runtime
    /// health, or another Harness history store.
    pub(super) response_text: Option<String>,
    /// Harness-owned delivery/control fact safe for durable coordination rows.
    pub(super) summary: String,
}

pub(super) fn claim_message_for_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: Option<&ProviderProcess>,
    message: &RegistryMessage,
    delivery_id: &str,
) -> CliResult<Option<RegistryMessage>> {
    let delivery = RegistryDeliveryAttempt {
        delivery_id: Some(delivery_id.to_string()),
        execution_status: Some(ProviderExecutionStatus::Running),
        native_session: member.native_session.clone(),
        started_at: Some(now_string()),
        provider_request_id: None,
        provider_thread_id: member.provider_thread_id.clone(),
        provider_turn_id: None,
        terminal_source: None,
        delivered_at: None,
        last_error: None,
    };
    match store.claim_queued_message_delivery(&member.id, &message.id, delivery)? {
        MessageDeliveryClaimResult::Claimed(message) => Ok(Some(*message)),
        MessageDeliveryClaimResult::NotQueued => Ok(None),
        MessageDeliveryClaimResult::BlockedByDelivery(session_id) => Err(CliError::Usage(format!(
            "agent {} has unresolved provider session {}; cannot claim another delivery",
            member.id, session_id
        ))),
    }
}

pub(super) fn retry_delivery_value(
    store: &HarnessStore,
    agent_id: &str,
    message_id: &str,
    session_id: Option<&str>,
    reason: &str,
    force: bool,
) -> CliResult<serde_json::Value> {
    let member = latest_member(store, agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    let mut message = latest_message(store, message_id)?;
    if message.to_agent_id.as_deref() != Some(agent_id) {
        return Err(CliError::Usage(format!(
            "message {message_id} is not addressed to agent {agent_id}"
        )));
    }
    let delivery = message.delivery.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "message {message_id} has no delivery claim to retry"
        ))
    })?;
    let delivery_id = session_id
        .map(str::to_string)
        .or(delivery.delivery_id.clone())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "message {message_id} has no delivery attempt id to retry"
            ))
        })?;
    if delivery.delivery_id.as_deref() != Some(delivery_id.as_str()) {
        return Err(CliError::Usage(format!(
            "delivery attempt {delivery_id} does not belong to message {message_id}"
        )));
    }
    let safe_without_force = delivery.provider_request_id.is_none()
        && delivery.provider_turn_id.is_none()
        && !matches!(
            delivery.execution_status,
            Some(ProviderExecutionStatus::Succeeded)
        );
    if !force && !safe_without_force {
        return Err(CliError::Usage(format!(
            "delivery retry for message {message_id} is not safe without --force; reconcile provider output first or pass --force explicitly"
        )));
    }

    let evidence_id = record_operator_evidence(
        store,
        message.task_id.clone(),
        "delivery_retry",
        &format!("delivery-attempt:{delivery_id}"),
        reason,
    )?;
    message.delivery_status = RegistryDeliveryStatus::Queued;
    message.delivery = None;
    store.append_message(&message)?;
    append_harness_runtime_control_fact(
        store,
        agent_id,
        member.provider_runtime_id.as_deref(),
        message.task_id.as_deref(),
        "delivery_requeued",
        reason,
        None,
    )?;

    Ok(serde_json::json!({
        "agent_member_id": agent_id,
        "message_id": message_id,
        "delivery_id": delivery_id,
        "delivery_status": message.delivery_status,
        "execution_status": ProviderExecutionStatus::Canceled,
        "evidence_id": evidence_id,
        "forced": force
    }))
}

pub(super) fn record_operator_evidence(
    store: &HarnessStore,
    task_id: Option<String>,
    source_type: &str,
    source_ref: &str,
    summary: &str,
) -> CliResult<String> {
    let evidence = Evidence {
        id: generated_id("evidence"),
        task_id,
        source_type: source_type.into(),
        source_ref: source_ref.into(),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    let id = evidence.id.clone();
    store.append_evidence(&evidence)?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record_claimed_delivery_terminal(
    store: &HarnessStore,
    delivery_id: &str,
    message: &RegistryMessage,
    _status: ProviderExecutionStatus,
    _provider_thread_id: Option<String>,
    _provider_turn_id: Option<String>,
    _terminal_source: Option<MessageTerminalSource>,
    summary: &str,
    source_ref: Option<&str>,
    _exit_code: Option<i32>,
) -> CliResult<Vec<String>> {
    let evidence_id = generated_id("evidence");
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: message.task_id.clone(),
        source_type: "delivery_attempt".into(),
        source_ref: source_ref
            .map(str::to_string)
            .unwrap_or_else(|| format!("delivery-attempt:{delivery_id}")),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;

    Ok(vec![evidence_id])
}

pub(super) fn message_status_for_delivery(
    status: &ProviderExecutionStatus,
) -> RegistryDeliveryStatus {
    message_status_for_terminal(status, None)
}

pub(super) fn message_status_for_terminal(
    status: &ProviderExecutionStatus,
    terminal_source: Option<&MessageTerminalSource>,
) -> RegistryDeliveryStatus {
    match status {
        ProviderExecutionStatus::Succeeded => RegistryDeliveryStatus::Delivered,
        ProviderExecutionStatus::Running => RegistryDeliveryStatus::Acknowledged,
        ProviderExecutionStatus::Stale
            if terminal_source != Some(&MessageTerminalSource::Failed) =>
        {
            RegistryDeliveryStatus::Acknowledged
        }
        _ => RegistryDeliveryStatus::Failed,
    }
}

pub(super) fn provider_status_blocks_delivery(status: &ProviderExecutionStatus) -> bool {
    matches!(
        status,
        ProviderExecutionStatus::Running | ProviderExecutionStatus::Stale
    )
}

pub(super) fn delivery_error_message(
    status: &ProviderExecutionStatus,
    summary: &str,
) -> Option<String> {
    matches!(
        status,
        ProviderExecutionStatus::Failed
            | ProviderExecutionStatus::Canceled
            | ProviderExecutionStatus::Stale
    )
    .then(|| summary.to_string())
}

pub(super) fn provider_developer_instructions(member: &ProviderLaunchProfile) -> String {
    let Some(prompt_ref) = member.prompt_ref.as_deref() else {
        return "Use harness messages as source of truth.".into();
    };
    let path = PathBuf::from(prompt_ref);
    if path.exists() {
        fs::read_to_string(path).unwrap_or_else(|_| prompt_ref.to_string())
    } else {
        prompt_ref.to_string()
    }
}

// Test-only helper: builds the codex app-server turn input envelope. Exercised by
// unit tests; not yet wired into the live delivery path (kept for the WP that lands it).
#[cfg(test)]
pub(super) fn build_turn_input(
    message: &RegistryMessage,
    delivery_attempt_id: &str,
) -> serde_json::Value {
    serde_json::json!([{
        "type": "text",
        "text": format!(
            "Harness message envelope:\nmessage_id: {}\nkind: {}\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: {}\ndelivery_attempt: {}\ncontent:\n{}",
            message.id,
            message_kind_label(&message.kind),
            message.task_id.as_deref().unwrap_or("-"),
            message.from_agent_id,
            message.to_agent_id.as_deref().unwrap_or("-"),
            message.channel.as_deref().unwrap_or("-"),
            delivery_attempt_id,
            message.content
        )
    }])
}

/// Resolve a control endpoint to a filesystem path.
///
/// Codex uses a `unix://` socket endpoint, so its path is the prefix-stripped
/// value. Other providers (e.g. the claude CLI shape, or HTTP/stdio transports)
/// do not present a unix-socket endpoint; for any non-`unix://` scheme we return
/// the endpoint verbatim so callers that only inspect existence/format keep
/// working without assuming a unix socket. This keeps the seam provider-neutral
/// per ADR 0011 — the endpoint format is the one place Codex assumed a socket.
#[cfg(test)]
pub(super) fn reconcile_running_delivery_attempts(
    store: &HarnessStore,
    agent_member_id: &str,
    task_id: Option<&str>,
    provider_thread_id: Option<&str>,
    provider_turn_id: Option<&str>,
    terminal_source: MessageTerminalSource,
) -> CliResult<bool> {
    if provider_thread_id.is_none() && provider_turn_id.is_none() {
        return Ok(false);
    }
    let mut reconciled_task_ids = BTreeSet::new();
    let mut reconciled_any = false;
    for mut message in latest_messages_in_append_order(store)?
        .into_iter()
        .filter(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message.delivery_status == RegistryDeliveryStatus::Acknowledged
                && task_id.is_none_or(|task_id| message.task_id.as_deref() == Some(task_id))
                && message.delivery.as_ref().is_some_and(|delivery| {
                    matches!(
                        delivery.execution_status,
                        Some(ProviderExecutionStatus::Running | ProviderExecutionStatus::Stale)
                    ) && provider_thread_id.is_none_or(|thread_id| {
                        delivery.provider_thread_id.as_deref() == Some(thread_id)
                    }) && provider_turn_id.is_none_or(|turn_id| {
                        delivery
                            .provider_turn_id
                            .as_deref()
                            .is_none_or(|delivery_turn_id| delivery_turn_id == turn_id)
                    })
                })
        })
    {
        message.delivery_status = RegistryDeliveryStatus::Delivered;
        if let Some(delivery) = message.delivery.as_mut() {
            delivery.execution_status = Some(ProviderExecutionStatus::Succeeded);
            delivery.terminal_source = Some(terminal_source.clone());
            if delivery.provider_thread_id.is_none() {
                delivery.provider_thread_id = provider_thread_id.map(str::to_string);
            }
            if delivery.provider_turn_id.is_none() {
                delivery.provider_turn_id = provider_turn_id.map(str::to_string);
            }
            delivery.delivered_at = Some(now_string());
            delivery.last_error = None;
        }
        if let Some(task_id) = message.task_id.clone() {
            reconciled_task_ids.insert(task_id);
        }
        store.append_message(&message)?;
        reconciled_any = true;
    }
    if reconciled_any {
        if let Ok(mut member) = latest_member(store, agent_member_id) {
            if let Some(runtime_id) = member.provider_runtime_id.clone() {
                mark_runtime_delivery_reconciled(store, &runtime_id, &terminal_source)?;
            }
            if matches!(
                member.status,
                ProviderLaunchStatus::Running | ProviderLaunchStatus::Stale
            ) && member
                .current_task_id
                .as_ref()
                .map_or_else(|| true, |task_id| reconciled_task_ids.contains(task_id))
            {
                member.status = ProviderLaunchStatus::Idle;
                member.current_task_id = None;
                member.last_seen_at = Some(now_string());
                store.append_member(&member)?;
            }
        }
    }
    Ok(reconciled_any)
}

#[cfg(test)]
pub(super) fn mark_runtime_delivery_reconciled(
    store: &HarnessStore,
    runtime_id: &str,
    terminal_source: &MessageTerminalSource,
) -> CliResult<()> {
    if let Some(mut runtime) = latest_runtime(store, runtime_id)? {
        runtime.health.delivery_probe =
            Some(format!("pass: {}", terminal_source_label(terminal_source)));
        runtime.health.checked_at = Some(now_string());
        runtime.last_event_at = Some(now_string());
        store.append_runtime(&runtime)?;
    }
    Ok(())
}

pub(super) fn mark_runtime_delivery_terminal(
    store: &HarnessStore,
    runtime_id: &str,
    status: &ProviderExecutionStatus,
    terminal_source: Option<&MessageTerminalSource>,
) -> CliResult<()> {
    if let Some(mut runtime) = latest_runtime(store, runtime_id)? {
        runtime.health.delivery_probe = Some(match status {
            ProviderExecutionStatus::Succeeded => format!(
                "pass: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| "unknown".into())
            ),
            ProviderExecutionStatus::Stale => format!(
                "stale: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| "unknown".into())
            ),
            _ => format!(
                "failed: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| provider_status_label(status).into())
            ),
        });
        runtime.health.checked_at = Some(now_string());
        runtime.last_event_at = Some(now_string());
        store.append_runtime(&runtime)?;
    }
    Ok(())
}

pub(super) fn has_unresolved_delivery_attempt(
    store: &HarnessStore,
    agent_member_id: &str,
) -> CliResult<bool> {
    Ok(latest_messages_in_append_order(store)?
        .into_iter()
        .any(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message.delivery.as_ref().is_some_and(|delivery| {
                    matches!(
                        delivery.execution_status,
                        Some(ProviderExecutionStatus::Queued | ProviderExecutionStatus::Running)
                    ) || (delivery.execution_status == Some(ProviderExecutionStatus::Stale)
                        && !matches!(
                            delivery.terminal_source,
                            Some(MessageTerminalSource::Failed)
                        ))
                })
        }))
}

pub(super) fn mark_running_delivery_attempts_terminal(
    store: &HarnessStore,
    agent_member_id: &str,
    status: ProviderExecutionStatus,
    terminal_source: Option<MessageTerminalSource>,
) -> CliResult<()> {
    let mut changed = false;
    for mut message in latest_messages_in_append_order(store)?
        .into_iter()
        .filter(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message.delivery.as_ref().is_some_and(|delivery| {
                    matches!(
                        delivery.execution_status,
                        Some(ProviderExecutionStatus::Running | ProviderExecutionStatus::Stale)
                    )
                })
        })
    {
        message.delivery_status = message_status_for_terminal(&status, terminal_source.as_ref());
        if let Some(delivery) = message.delivery.as_mut() {
            delivery.execution_status = Some(status.clone());
            delivery.terminal_source = terminal_source.clone();
            delivery.delivered_at = Some(now_string());
            delivery.last_error = delivery_error_message(&status, "provider delivery ended");
        }
        store.append_message(&message)?;
        changed = true;
    }
    if changed {
        if let Ok(mut member) = latest_member(store, agent_member_id) {
            if matches!(
                member.status,
                ProviderLaunchStatus::Running | ProviderLaunchStatus::Stale
            ) {
                if let Some(runtime_id) = member.provider_runtime_id.clone() {
                    mark_runtime_delivery_terminal(
                        store,
                        &runtime_id,
                        &status,
                        terminal_source.as_ref(),
                    )?;
                }
                member.status = ProviderLaunchStatus::Idle;
                member.current_task_id = None;
                member.last_seen_at = Some(now_string());
                store.append_member(&member)?;
            }
        }
    }
    Ok(())
}

// Test-only helper: extracts JSON-RPC error strings; covered by unit tests only.
#[cfg(test)]
pub(super) fn jsonrpc_error_messages(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.get("error"))
        .map(|error| {
            error
                .get("message")
                .and_then(|message| message.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| summarize_json_value(error))
        })
        .collect()
}

// Test-only helper: validates a codex app-server turn-start exchange; unit-tested only.
#[cfg(test)]
pub(super) fn turn_exchange_confirms_turn_start(
    values: &[serde_json::Value],
    request_id: &str,
) -> bool {
    values.iter().any(|value| {
        value.get("id").and_then(|id| id.as_str()) == Some(request_id)
            && value.get("error").is_none()
    }) || values.iter().any(|value| {
        value
            .get("method")
            .and_then(|method| method.as_str())
            .is_some_and(|method| {
                matches!(
                    method,
                    "turn/started"
                        | "turn/completed"
                        | "turn/status/changed"
                        | "turn/plan/updated"
                        | "turn/diff/updated"
                )
            })
    })
}

// Test-only helper: maps codex app-server values to a terminal source; unit-tested only.
#[cfg(test)]
pub(super) fn terminal_source_from_values(
    values: &[serde_json::Value],
) -> Option<MessageTerminalSource> {
    for value in values {
        let method = value.get("method").and_then(|method| method.as_str());
        if method == Some("turn/completed") {
            return Some(MessageTerminalSource::TurnCompleted);
        }
    }
    for value in values {
        let method = value.get("method").and_then(|method| method.as_str());
        if method == Some("thread/status/changed")
            && value
                .get("params")
                .and_then(|params| params.get("status"))
                .and_then(|status| status.get("type"))
                .and_then(|status_type| status_type.as_str())
                == Some("idle")
        {
            return Some(MessageTerminalSource::ThreadIdle);
        }
    }
    None
}

// Test-only helper: extracts a thread id from codex app-server values; unit-tested only.
#[cfg(test)]
pub(super) fn extract_thread_id(values: &[serde_json::Value], request_id: &str) -> Option<String> {
    for value in values {
        if value.get("id").and_then(|id| id.as_str()) == Some(request_id) {
            if let Some(result) = value.get("result") {
                if let Some(thread_id) = thread_id_from_container(result) {
                    return Some(thread_id);
                }
            }
        }
    }

    for value in values {
        let method = value
            .get("method")
            .and_then(|method| method.as_str())
            .unwrap_or_default();
        if method == "thread/started" || method == "thread_started" {
            if let Some(params) = value.get("params") {
                if let Some(thread_id) = thread_id_from_container(params) {
                    return Some(thread_id);
                }
            }
        }
    }
    None
}

#[cfg(test)]
pub(super) fn thread_id_from_container(value: &serde_json::Value) -> Option<String> {
    for path in [
        &["thread", "id"][..],
        &["thread", "threadId"][..],
        &["threadId"][..],
        &["thread_id"][..],
        &["id"][..],
    ] {
        if let Some(thread_id) = json_path_string(value, path) {
            return Some(thread_id);
        }
    }
    None
}

#[cfg(test)]
pub(super) fn json_path_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

/// Truncate `s` to at most `max` BYTES without splitting a UTF-8 char: byte
/// slicing (`&s[..max]`) panics when `max` lands inside a multi-byte char (CJK,
/// emoji, …), so back off to the nearest char boundary at or below `max` first.
/// Used on every summary/error path that bounds an arbitrary (possibly non-ASCII)
/// provider string — a formatting nicety must never be able to panic a live run
/// after the agent work (and its tokens) are already spent. (issue #89, item 1)
#[allow(dead_code)]
pub(super) fn truncate_on_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
pub(super) fn summarize_json_value(value: &serde_json::Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "provider event".into());
    if raw.len() > 240 {
        format!("{}...", truncate_on_char_boundary(&raw, 240))
    } else {
        raw
    }
}
