use super::*;

pub(super) use harness_provider_claude::{
    extract_claude_reply_text, infer_claude_session_status, status_to_terminal_source,
    ClaudeStreamEvent,
};
pub(super) use harness_provider_codex::{
    extract_codex_reply_text, extract_thread_id_from_exec_events, extract_turn_id_from_exec_events,
    infer_provider_execution_status, CodexExecEvent,
};

/// Write a temporary MCP config JSON file for Claude.
/// Returns the path to the temporary file, or None if mcp is empty/None.
pub(super) fn write_temp_mcp_config(mcp: Option<&LaunchMcp>) -> CliResult<Option<String>> {
    harness_provider_claude::write_claude_mcp_config(mcp)
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
        .map_err(CliError::Usage)
}

pub(super) fn run_codex_exec_process(
    session_dir: &Path,
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<CodexExecDeliveryRun> {
    // Build the command: `codex exec --json <prompt>`
    // The LaunchSpec is composed from the member/message; the exec arg is the message_content.
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let developer_instructions = provider_developer_instructions(member);
    // cwd precedence (P3, Stage 3): member.provider_cwd_hint → selected
    // project.project_root → process cwd. Codex discovers AGENTS.md from its cwd,
    // so a `serve` that switched projects must still spawn here in the project root.
    let cwd = delivery_worker_cwd(member, project);

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    let run = harness_provider_codex::run_codex_compatibility(
        &spec,
        &message_content,
        &developer_instructions,
        Path::new(&cwd),
        session_dir,
        Duration::from_millis(timeout_ms),
    )
    .map_err(CliError::Usage)?;
    Ok((run.process_success, run.events, run.raw_events, run.stderr))
}

/// This is the exec-stream variant of run_codex_app_server_exchange.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_codex_exec_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store
        .root()
        .join("runtimes")
        .join("deliveries")
        .join(delivery_id);
    fs::create_dir_all(&session_dir)?;
    let spec = build_launch_spec(member, message);

    let (process_success, events, raw_events, _stderr_log) =
        run_codex_exec_process(&session_dir, member, message, timeout_ms, project)?;
    let (tokens, cost_usd, model) = codex_delivery_telemetry(&raw_events, &spec);

    // Infer the delivery status from events and process exit.
    let status = infer_provider_execution_status(&events, process_success);
    let terminal_source = if matches!(status, ProviderExecutionStatus::Succeeded) {
        events
            .iter()
            .find_map(|e| e.terminal_source())
            .or(Some(MessageTerminalSource::Unknown))
    } else {
        Some(MessageTerminalSource::Failed)
    };

    let provider_thread_id = extract_thread_id_from_exec_events(&events);
    let provider_turn_id = extract_turn_id_from_exec_events(&events);
    let exit_code = if process_success { Some(0) } else { Some(1) };
    let reply = extract_codex_reply_text(&events);
    let structured =
        structured_for_status(&status, codex_delivery_structured(reply.as_deref(), &spec));

    let summary = match status {
        ProviderExecutionStatus::Succeeded => {
            "Codex provider delivery completed; transcript remains provider-native".into()
        }
        ProviderExecutionStatus::Failed => {
            "Codex provider delivery failed; inspect the provider-native session".into()
        }
        ProviderExecutionStatus::Stale => {
            "Codex exec --json produced output but did not complete before timeout".into()
        }
        _ => "Codex exec --json session ended".into(),
    };

    let _ = fs::remove_dir_all(&session_dir);
    let native_session = provider_thread_id
        .as_ref()
        .map(|id| provider_native_session_ref("codex", id));
    Ok(DeliveryOutcome {
        status: status.clone(),
        native_session,
        provider_thread_id,
        provider_turn_id,
        terminal_source,
        provider_request_id: None, // exec stream does not use request_id
        exit_code,
        tokens,
        cost_usd,
        model,
        structured,
        response_text: reply,
        summary,
    })
}

/// Execute the reviewed Claude exact-session headless Host binding.
///
/// This deliberately bypasses the direct-delivery compatibility registry:
/// sharing the Claude CLI transport does not make Host execution a
/// compatibility route or an Agent Team fallback.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_claude_host_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    let binding = harness_application::provider_descriptor("claude")
        .and_then(|descriptor| descriptor.external_host_transport)
        .ok_or_else(|| {
            CliError::Usage(
                "HEADLESS_HOST_UNSUPPORTED: Claude has no declared Host binding".to_string(),
            )
        })?;
    if member.provider != "claude"
        || binding.binding != harness_application::ExternalHostTransportKind::ClaudeCli
    {
        return Err(CliError::Usage(format!(
            "HEADLESS_HOST_BINDING_MISMATCH: expected claude/claude_cli, got {}/{}",
            member.provider, binding.execution_mode
        )));
    }
    run_claude_delivery_surface(
        store,
        member,
        runtime,
        message,
        delivery_id,
        timeout_ms,
        project,
        true,
    )
}

/// Run one `/v1/agents/*` compatibility delivery, routed only through the
/// explicit compatibility registry.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_compatibility_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    match compatibility_delivery_binding(&member.provider) {
        Some(adapter) => adapter.run_delivery(
            store,
            member,
            runtime,
            message,
            delivery_id,
            timeout_ms,
            project,
        ),
        None => Err(unknown_provider_error(&member.provider, "delivery")),
    }
}

// WP-5: Codex exec-stream runtime (no persistent process).
// Each delivery spawns `codex exec --json`, so no app-server socket is needed.

pub(super) type CodexExecDeliveryRun = (bool, Vec<CodexExecEvent>, Vec<serde_json::Value>, String);
pub(super) type ClaudeDeliveryRun = (
    bool,
    Vec<ClaudeStreamEvent>,
    Vec<serde_json::Value>,
    Option<String>,
    String,
);

pub(super) fn start_codex_exec_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Codex, we use exec-stream delivery (no persistent app-server).
    // Each delivery spawns `codex exec --json`, so there's no long-lived process.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("codex-exec-runtime://{}", runtime_dir.display());

    let args = vec![
        // Codex will be spawned on each delivery via codex exec --json
    ];

    // Check if codex binary is available
    let process_alive = Command::new("which")
        .arg("codex")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(ProviderProcess {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None, // Codex exec runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "codex".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive,
            socket_exists: true,                        // Runtime dir exists
            protocol_probe: Some("exec-stream".into()), // Codex uses exec-stream
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

// --- Claude runtime (BE-WP7) ---
// The claude CLI shape: spawn the claude binary as a local process, run message
// delivery exchanges via stdin/stdout, record sessions and evidence.

pub(super) fn start_claude_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Claude CLI, we don't spawn a persistent process on runtime start.
    // Instead, we record the runtime as "ready" and each delivery will spawn
    // claude with the message. This matches the behavior of claude as a
    // request-response tool rather than a persistent app-server.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("claude-runtime://{}", runtime_dir.display());

    let args = vec![
        // Claude CLI will be spawned on each delivery with the message prompt
    ];

    // Check if claude binary is available, but don't require it at test time
    let process_alive = Command::new("which")
        .arg("claude")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(ProviderProcess {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None, // Claude runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "claude".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive,
            socket_exists: true,                    // Runtime dir exists
            protocol_probe: Some("unknown".into()), // Will probe on first delivery
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_claude_delivery(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<DeliveryOutcome> {
    run_claude_delivery_surface(
        store,
        member,
        _runtime,
        message,
        delivery_id,
        timeout_ms,
        project,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_claude_delivery_surface(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
    _runtime: &ProviderProcess,
    message: &RegistryMessage,
    delivery_id: &str,
    timeout_ms: u64,
    project: &ProjectContext,
    host_surface: bool,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store
        .root()
        .join("runtimes")
        .join("deliveries")
        .join(delivery_id);
    fs::create_dir_all(&session_dir)?;

    // The provider package executes and reduces the native Claude transport.
    //
    // Opt-in resident path (HARNESS_CLAUDE_RESIDENT=1): instead of spawning a
    // fresh `claude -p <prompt>` that exits per turn, hold a `claude
    // --input-format stream-json` process open and feed the turn as a stdin
    // frame (see `resident.rs`). The returned tuple shape is identical to the
    // default path, so status inference and telemetry stay provider-neutral.
    let resident = !host_surface && env::var("HARNESS_CLAUDE_RESIDENT").as_deref() == Ok("1");
    let (process_success, events, raw_events, session_id, _stderr_log) = if resident {
        run_claude_resident_delivery_real(&session_dir, member, message, timeout_ms, project)?
    } else {
        run_claude_exec_delivery_real(member, message, timeout_ms, project, host_surface)?
    };
    let (tokens, cost_usd, model, raw_structured) = claude_delivery_telemetry(&raw_events);

    let status = infer_claude_session_status(&events, process_success);
    let structured = structured_for_status(&status, raw_structured);
    let terminal_source = status_to_terminal_source(&status);
    // The id we hand back as the member's provider thread for the NEXT delivery
    // to resume. Only a real session id parsed from the provider output is
    // resumable; the synthetic fallback id above is not, so it is not surfaced
    // as a resume token.
    let resumable_session_id = session_id.clone();

    let _ = fs::remove_dir_all(&session_dir);
    Ok(DeliveryOutcome {
        native_session: resumable_session_id
            .as_ref()
            .map(|id| provider_native_session_ref("claude", id)),
        // Surface the real claude session id as the member's provider thread so
        // the next delivery resumes this conversation (memory across deliveries).
        provider_thread_id: resumable_session_id,
        provider_turn_id: None,
        terminal_source,
        status,
        provider_request_id: None,
        exit_code: if process_success { Some(0) } else { Some(1) },
        tokens,
        cost_usd,
        model,
        structured,
        response_text: process_success
            .then(|| extract_claude_reply_text(&events))
            .flatten(),
        summary: if process_success {
            "Claude provider delivery completed; transcript remains provider-native".to_string()
        } else {
            "Claude provider delivery failed; inspect the provider-native session".to_string()
        },
    })
}

/// Compose one Claude request and delegate the selected Host/compatibility
/// transport to the provider package.
pub(super) fn run_claude_exec_delivery_real(
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    timeout_ms: u64,
    project: &ProjectContext,
    host_surface: bool,
) -> CliResult<ClaudeDeliveryRun> {
    // Build the message content envelope (harness context).
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    // Compose system prompt (developer instructions from member prompt_ref).
    let system_prompt = provider_developer_instructions(member);

    // cwd precedence (P3, Stage 3): member.provider_cwd_hint → selected
    // project.project_root → process cwd. Claude Code discovers CLAUDE.md /
    // .claude/ (and keys per-project memory) from its cwd, so a `serve` that
    // switched projects must still spawn here in the selected project root.
    let cwd = delivery_worker_cwd(member, project);

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    let run = if host_surface {
        harness_provider_claude::run_claude_host_turn(
            &spec,
            &message_content,
            &system_prompt,
            Path::new(&cwd),
            Duration::from_millis(timeout_ms),
        )
    } else {
        harness_provider_claude::run_claude_compatibility(
            &spec,
            &message_content,
            &system_prompt,
            Path::new(&cwd),
            Duration::from_millis(timeout_ms),
        )
    }
    .map_err(CliError::Usage)?;
    Ok((
        run.process_success,
        run.events,
        run.raw_events,
        run.session_id,
        run.stderr,
    ))
}

/// Build a provider-owned [`harness_provider_claude::ResidentConfig`] from the same launch inputs the default
/// path uses, so the resident invocation surface matches `claude -p` flag for
/// flag (only `-p <prompt>` becomes `--input-format stream-json`).
pub(super) fn build_resident_config(
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    project: &ProjectContext,
) -> harness_provider_claude::ResidentConfig {
    let spec = build_launch_spec(member, message);
    let system_prompt = provider_developer_instructions(member);
    // Same cwd precedence as the default Claude path (P3, Stage 3):
    // member.provider_cwd_hint → selected project.project_root → process cwd.
    let cwd = delivery_worker_cwd(member, project);

    let mcp_config_path = write_temp_mcp_config(spec.mcp.as_ref()).ok().flatten();

    let mut add_dirs = Vec::new();
    if let Some(workspace) = &spec.workspace {
        add_dirs.push(workspace.clone());
    }
    for root in &spec.writable_roots {
        add_dirs.push(root.clone());
    }

    harness_provider_claude::ResidentConfig {
        binary: "claude".into(),
        model: spec.model.clone(),
        effort: spec.effort.clone(),
        output_schema_json: spec
            .output_schema
            .as_ref()
            .map(|schema| schema_to_json_schema(schema).to_string()),
        permission_mode: harness_provider_claude::claude_compatibility_permission(spec.permission)
            .to_string(),
        tools: spec.tools.clone(),
        system_prompt,
        mcp_config_path,
        add_dirs,
        cwd,
        resume: spec.resume.clone(),
    }
}

/// Opt-in resident sibling of [`run_claude_exec_delivery_real`]. Holds a
/// `claude --input-format stream-json` process open and feeds the turn as a
/// stdin frame, returning the SAME `(success, events, raw_events, session_id, stderr)`
/// tuple shape as the default path so `run_claude_delivery` can share the same
/// status, telemetry, and recording logic.
///
/// This transport is process-local: the machine NodeDaemon is the only durable
/// daemon authority. The resident is dropped after the turn, which closes
/// stdin and reaps the child without creating a second lifecycle controller.
pub(super) fn run_claude_resident_delivery_real(
    session_dir: &Path,
    member: &ProviderLaunchProfile,
    message: &RegistryMessage,
    timeout_ms: u64,
    project: &ProjectContext,
) -> CliResult<ClaudeDeliveryRun> {
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let config = build_resident_config(member, message, project);
    let stderr_path = session_dir.join("claude.stderr");
    let timeout = Duration::from_millis(timeout_ms.max(1));

    let mut resident = harness_provider_claude::ResidentClaude::spawn(config, &stderr_path)
        .map_err(|error| {
            CliError::Usage(format!("failed to spawn resident claude process: {error}"))
        })?;

    // Drive exactly one turn. On error (timeout / dead child) the resident is
    // dropped (stdin closed, child reaped) and we surface a failed tuple,
    // mirroring the default path's timeout behavior.
    let turn = match resident.send_turn(&message_content, timeout) {
        Ok(turn) => turn,
        Err(error) => {
            let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();
            let session_id = resident.session_id();
            drop(resident);
            return Ok((
                false,
                Vec::new(),
                Vec::new(),
                session_id,
                format!("{error}\n{stderr_log}"),
            ));
        }
    };

    // Map ResidentEvent -> ClaudeStreamEvent (same shape, local type bridge).
    let events: Vec<ClaudeStreamEvent> = turn
        .events
        .into_iter()
        .map(|event| ClaudeStreamEvent {
            event_type: event.event_type,
            payload: event.payload,
        })
        .collect();
    let raw_events = events.iter().map(|event| event.payload.clone()).collect();
    let session_id = turn.session_id;
    let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();

    // Clean shutdown: closes stdin (EOF) and reaps the child. v1 is one turn
    // per delivery so we do not keep the resident across `run_claude_delivery`
    // calls; the in-process pool (resident.rs) is the seam for that later.
    resident.shutdown();

    Ok((turn.success, events, raw_events, session_id, stderr_log))
}

pub(super) fn append_harness_runtime_control_fact(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    event_type: &str,
    summary: &str,
    payload_ref: Option<&str>,
) -> CliResult<()> {
    let event = Evidence {
        id: generated_id("event"),
        task_id: task_id.map(str::to_string),
        source_type: "harness_runtime_control_fact".into(),
        source_ref: serde_json::json!({
            "agent_member_id": agent_member_id,
            "provider_runtime_id": runtime_id,
            "provider": "codex",
            "event_type": event_type,
            "payload_ref": payload_ref,
        })
        .to_string(),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: Some("harness_runtime_control_fact".into()),
        goal_id: None,
    };
    store.append_evidence(&event)?;
    Ok(())
}
