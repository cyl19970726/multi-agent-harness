use super::*;

// Provider dispatch seam (BE-WP6)
//
// The harness core stays provider-neutral (ADR 0011); all provider-specific
// behaviour lives behind these four dispatch points keyed on `member.provider`.
// Codex, Claude, and Kimi route to their registered adapters. Unknown providers
// fail fast with an explicit error rather than silently assuming Codex.
// ---------------------------------------------------------------------------

/// Provider-specific behaviour boundary. Every current provider dispatch site
/// routes through this trait and the `provider_adapter` registry.
pub(super) trait ProviderAdapter: Sync {
    /// Canonical provider id as used in `member.provider` and `agent(provider=...)`.
    fn name(&self) -> &'static str;

    /// What this provider's platform can technically support — streaming, resume,
    /// mid-turn approval, subagents, MCP, hooks, native schema, billed cost
    /// (goal-provider-neutral). Drives declarative capability degradation: a
    /// provider that lacks an axis returns `false` and the caller falls back to
    /// the shared mechanism (text-extract for schema, token-estimate for cost),
    /// never a per-provider branch. The default is the conservative "exec
    /// streaming agent with no native schema/cost" posture, which a new provider
    /// can adopt unchanged; codex/claude override with their real presets.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            resume: true,
            mid_turn_approval: false,
            subagents: false,
            mcp: false,
            hooks: false,
            schema: false,
            cost: false,
            // Conservative default: a provider that adopts the default posture is
            // assumed UNABLE to enforce read-only, so its read-only leaves are
            // worktree-isolated rather than trusted (matches the serde default and the
            // unknown-provider fallback).
            enforces_read_only: false,
        }
    }

    /// Filename used only inside the short-lived process transport directory.
    /// It is reduced in memory and removed; it is never a Harness history record.
    fn live_ndjson_file_name(&self) -> &'static str;

    /// Map a LaunchPermission to this provider's CLI permission flag value
    /// (codex `--sandbox`, claude `--permission-mode`).
    fn map_permission(&self, perm: LaunchPermission) -> &'static str;

    /// Spawn (or attach) the persistent runtime for a member of this provider.
    fn start_runtime(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
    ) -> CliResult<ProviderProcess>;

    /// Run a single message delivery against this provider's persistent runtime.
    ///
    /// `project` is the selected [`ProjectContext`] (goal-multi-project P3): the
    /// worker's cwd derives from `project.project_root` when the member is not
    /// pinned to a specific `provider_cwd_hint`, so a long-running `serve` that switched
    /// projects (and never `cd`d) still spawns the worker in the right tree where
    /// its `CLAUDE.md` / `AGENTS.md` live.
    #[allow(clippy::too_many_arguments)]
    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
        runtime: &ProviderProcess,
        message: &RegistryMessage,
        delivery_id: &str,
        timeout_ms: u64,
        project: &ProjectContext,
    ) -> CliResult<DeliveryOutcome>;
}

pub(super) struct CodexAdapter;
pub(super) struct ClaudeAdapter;
pub(super) struct KimiAdapter;
pub(super) struct PiAdapter;

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::codex_exec()
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "codex.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            LaunchPermission::ReadOnly => "read-only",
            LaunchPermission::WorkspaceWrite => "workspace-write",
            LaunchPermission::FullAccess => "danger-full-access",
        }
    }

    fn start_runtime(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
    ) -> CliResult<ProviderProcess> {
        start_codex_exec_runtime(store, member)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
        runtime: &ProviderProcess,
        message: &RegistryMessage,
        delivery_id: &str,
        timeout_ms: u64,
        project: &ProjectContext,
    ) -> CliResult<DeliveryOutcome> {
        run_codex_exec_delivery(
            store,
            member,
            runtime,
            message,
            delivery_id,
            timeout_ms,
            project,
        )
    }
}
impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::claude_exec()
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "claude.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            LaunchPermission::ReadOnly => "plan",
            LaunchPermission::WorkspaceWrite => "acceptEdits",
            LaunchPermission::FullAccess => "bypassPermissions",
        }
    }

    fn start_runtime(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
    ) -> CliResult<ProviderProcess> {
        start_claude_runtime(store, member)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
        runtime: &ProviderProcess,
        message: &RegistryMessage,
        delivery_id: &str,
        timeout_ms: u64,
        project: &ProjectContext,
    ) -> CliResult<DeliveryOutcome> {
        run_claude_delivery(
            store,
            member,
            runtime,
            message,
            delivery_id,
            timeout_ms,
            project,
        )
    }
}

// ============================================================================
// Kimi adapter (goal-provider-neutral S4): a NATIVE third provider, registered
// with ZERO new match arms.
//
// Kimi Code is non-interactive via `-p <prompt> --output-format stream-json`,
// emitting claude-shaped line-delimited JSON (NDJSON): a `system` init frame
// carrying `session_id`/`model`, `assistant` message frames, and a terminal
// `result` frame. The CLI FLAG surface is verified against `kimi --help` v0.18 —
// Kimi has NONE of claude's `--verbose` / `--permission-mode` / `--allowedTools` /
// `--json-schema` / `--mcp-config` / `--add-dir` / `--append-system-prompt`; it
// uses STANDALONE permission flags (`--plan` / `--auto` / `-y`), resumes with
// `-S/--session`, and has no native schema/budget/effort, which degrade to the
// harness fallbacks (see `ProviderCapabilities::kimi_exec`). The wire shape is
// still proven deterministically against a fake `kimi` shim on PATH; the LIVE
// authenticated run (post `kimi login`) is the operator's step.
//
// The binary is resolved by [`resolve_kimi_bin`] (KIMI_CODE_BIN override, else the
// bare name `kimi` on PATH so a test shim / the installer's PATH entry wins, else
// the default install path). Because Kimi is claude-shaped on the wire, the stream
// interpreters (status/reply/usage/model/structured/session-id), the durable-trace
// ingest, and the live NDJSON tee all reuse the existing claude-stream helpers —
// they key on the wire SHAPE, not on the claude binary. Only the binary, the
// live-file basename, and the CLI flags differ.
// ============================================================================

/// Resolve the `kimi` (Kimi Code) executable. Order: the `KIMI_CODE_BIN` env
/// override (explicit), then the bare name `kimi` when it resolves on `PATH` (so a
/// test PATH shim AND the installer's `~/.kimi-code/bin` PATH entry both win), then
/// the default install path `~/.kimi-code/bin/kimi`, then the bare name as a last
/// resort so a missing binary surfaces a clear spawn error. Keeping `kimi`-on-PATH
/// ahead of the home-dir fallback is what lets the deterministic fake-kimi test
/// intercept the spawn.
pub(super) fn resolve_kimi_bin() -> String {
    if let Ok(explicit) = std::env::var("KIMI_CODE_BIN") {
        if !explicit.trim().is_empty() {
            return explicit;
        }
    }
    let on_path = Command::new("which")
        .arg("kimi")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if on_path {
        return "kimi".into();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let candidate = Path::new(&home).join(".kimi-code/bin/kimi");
        if candidate.is_file() {
            return candidate.display().to_string();
        }
    }
    "kimi".into()
}

// ============================================================================
// Kimi-native stream parsing. Verified LIVE against `kimi -p --output-format
// stream-json` (v0.18): the stream is FLAT NDJSON, NOT claude-shaped —
//   {"role":"assistant","content":"<text>"}                       (the reply)
//   {"role":"meta","type":"session.resume_hint",
//    "session_id":"session_<uuid>","command":"kimi -r <id>", ...} (resume token)
// There is no claude `system.init`/`result`/`usage` frame and no model frame in
// `-p` mode, so success is the child exit code and tokens/model/cost degrade per
// `ProviderCapabilities::kimi_exec`. `content` is normally a string but may be an
// array of blocks (tool/structured turns) — both are handled.
// ============================================================================

/// The assistant reply: concatenate the `content` of every `role=="assistant"`
/// frame in order. `content` is a string, or an array of blocks (each block's own
/// string, or its `text`/`content` field). None when the turn produced no text.
pub(super) fn extract_kimi_reply_text(frames: &[serde_json::Value]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for frame in frames {
        if frame.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        match frame.get("content") {
            Some(serde_json::Value::String(s)) => {
                if !s.trim().is_empty() {
                    parts.push(s.trim().to_string());
                }
            }
            Some(serde_json::Value::Array(blocks)) => {
                for block in blocks {
                    let text = block.as_str().or_else(|| {
                        block
                            .get("text")
                            .or_else(|| block.get("content"))
                            .and_then(|v| v.as_str())
                    });
                    if let Some(s) = text {
                        if !s.trim().is_empty() {
                            parts.push(s.trim().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// The resumable session id from the `session.resume_hint` meta frame, if present.
pub(super) fn extract_kimi_session_id(frames: &[serde_json::Value]) -> Option<String> {
    frames.iter().find_map(|frame| {
        if frame.get("type").and_then(|t| t.as_str()) == Some("session.resume_hint") {
            frame
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        } else {
            None
        }
    })
}

/// Session status for a `kimi -p` turn. There is no terminal success frame, so a
/// clean child exit IS success; a non-zero exit (e.g. an arg error on stderr) is a
/// failure; a clean exit with zero frames is stale (no reply produced).
pub(super) fn infer_kimi_status(
    frames: &[serde_json::Value],
    process_success: bool,
) -> ProviderExecutionStatus {
    if !process_success {
        ProviderExecutionStatus::Failed
    } else if frames.is_empty() {
        ProviderExecutionStatus::Stale
    } else {
        ProviderExecutionStatus::Succeeded
    }
}

pub(super) fn start_kimi_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;
    let endpoint = format!("kimi-runtime://{}", runtime_dir.display());
    // Probe the same binary spawn_kimi_ephemeral would resolve.
    let bin = resolve_kimi_bin();
    let process_alive = if bin.contains('/') {
        Path::new(&bin).is_file()
    } else {
        Command::new("which")
            .arg(&bin)
            .output()
            .ok()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    Ok(ProviderProcess {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: ProviderProcessStatus::Running,
        pid: None, // Kimi runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "kimi".into(),
        args: Vec::new(),
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: ProviderProcessHealth {
            process_alive,
            socket_exists: true,
            protocol_probe: Some("unknown".into()),
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

/// Spawn `kimi -p --output-format stream-json` (real kimi flags) for one member
/// delivery and parse the claude-shaped NDJSON. Mirrors
/// [`run_claude_exec_delivery_real`] but on Kimi's CLI surface: the developer
/// instructions are folded into the prompt (no `--append-system-prompt`), resume
/// uses `-S/--session`, and claude-only flags (`--verbose` / `--permission-mode` /
/// `--allowedTools` / `--json-schema` / `--mcp-config` / `--add-dir`) are dropped.
pub(super) fn run_kimi_exec_delivery_real(
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
    let system_prompt = provider_developer_instructions(member);
    let cwd = delivery_worker_cwd(member, project);
    let spec = build_launch_spec(member, message);

    // Kimi has no `--append-system-prompt`; fold the developer instructions into
    // the prompt text (a leading system block, which claude-shaped models honor).
    let prompt_text = if system_prompt.is_empty() {
        message_content
    } else {
        format!("{system_prompt}\n\n{message_content}")
    };

    let mut cmd = Command::new(resolve_kimi_bin());
    cmd.arg("-p")
        .arg(&prompt_text)
        .arg("--output-format")
        .arg("stream-json");
    // Resume uses `-S/--session <id>` in real kimi (not claude's `--resume`).
    if let Some(resume_id) = &spec.resume {
        cmd.arg("--session").arg(resume_id);
    }
    if let Some(model) = &spec.model {
        cmd.arg("--model").arg(model);
    }
    // Headless `kimi -p` REJECTS permission flags (--plan/--auto/--yolo all error
    // "Cannot combine --prompt with ..."), so none is passed. `--effort` /
    // `--json-schema` / `--allowedTools` / `--mcp-config` / `--add-dir` are likewise
    // not real kimi flags; schema/mcp/cost degrade to the harness fallbacks
    // (capabilities().{schema,mcp,cost} = false) and writable roots are bounded by
    // the harness-owned worktree, not a CLI flag.
    cmd.current_dir(&cwd);

    let delivery_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let run = run_ndjson_child(
        cmd,
        session_dir,
        &delivery_id,
        KimiAdapter.live_ndjson_file_name(),
        timeout_ms,
        None,
        None,
        "kimi -p process",
    )?;
    // Kimi -p stream-json is not claude-shaped — derive the session id from the raw
    // frames (the caller parses reply/status the same way). The `events` slot of the
    // shared tuple is unused for kimi (left empty); the raw frames carry the data.
    let session_id = extract_kimi_session_id(&run.events);
    Ok((
        run.process_success,
        Vec::new(),
        run.events,
        session_id,
        run.stderr,
    ))
}

/// Run one Kimi member delivery. The short-lived transport stream is reduced in
/// memory and removed; Kimi's own session remains the execution history.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_kimi_delivery(
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
    let (process_success, _events, raw_events, session_id, _stderr_log) =
        run_kimi_exec_delivery_real(&session_dir, member, message, timeout_ms, project)?;
    // Kimi -p stream-json carries no usage/model/cost/structured frame; degrade per
    // kimi_exec(). Reply/status/session come from the kimi-native parsers on the raw
    // frames.
    let (tokens, cost_usd, model): (Option<TokenUsage>, Option<f64>, Option<String>) =
        (None, None, None);
    let raw_structured: Option<serde_json::Value> = None;

    let status = infer_kimi_status(&raw_events, process_success);
    let structured = structured_for_status(&status, raw_structured);
    let terminal_source = status_to_terminal_source(&status);
    // Only a real session id parsed from the stream is resumable; the synthetic
    // fallback is not surfaced as a resume token (claude-identical).
    let resumable_session_id = session_id.clone();

    let _ = fs::remove_dir_all(&session_dir);
    Ok(DeliveryOutcome {
        native_session: resumable_session_id
            .as_ref()
            .map(|id| provider_native_session_ref("kimi", id)),
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
            .then(|| extract_kimi_reply_text(&raw_events))
            .flatten(),
        summary: if process_success {
            "Kimi provider delivery completed; transcript remains provider-native".to_string()
        } else {
            "Kimi provider delivery failed; inspect the provider-native session".to_string()
        },
    })
}

impl ProviderAdapter for KimiAdapter {
    fn name(&self) -> &'static str {
        "kimi"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::kimi_exec()
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "kimi.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        // Real Kimi Code exposes STANDALONE permission flags (`kimi --help` v0.18):
        // `--plan` / `--auto` / `-y/--yolo`. NOTE: the headless `-p` path does NOT
        // use this — `kimi -p` REJECTS every permission flag ("Cannot combine
        // --prompt with ..."), so the spawn/delivery paths pass none. Retained for
        // trait conformance and a potential future interactive/acp invocation; it
        // returns the standalone flag itself (not a `--permission-mode` value).
        match perm {
            LaunchPermission::ReadOnly => "--plan",
            LaunchPermission::WorkspaceWrite => "--auto",
            LaunchPermission::FullAccess => "--yolo",
        }
    }

    fn start_runtime(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
    ) -> CliResult<ProviderProcess> {
        start_kimi_runtime(store, member)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &ProviderLaunchProfile,
        runtime: &ProviderProcess,
        message: &RegistryMessage,
        delivery_id: &str,
        timeout_ms: u64,
        project: &ProjectContext,
    ) -> CliResult<DeliveryOutcome> {
        run_kimi_delivery(
            store,
            member,
            runtime,
            message,
            delivery_id,
            timeout_ms,
            project,
        )
    }
}

impl ProviderAdapter for PiAdapter {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            resume: true,
            mid_turn_approval: false,
            subagents: false,
            mcp: false,
            hooks: false,
            schema: false,
            cost: false,
            enforces_read_only: false,
        }
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "pi.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            // pi print mode: limit tools to read-only operations.
            LaunchPermission::ReadOnly => "--tools read,grep,find,ls",
            LaunchPermission::WorkspaceWrite => "",
            LaunchPermission::FullAccess => "",
        }
    }

    fn start_runtime(
        &self,
        _store: &HarnessStore,
        _member: &ProviderLaunchProfile,
    ) -> CliResult<ProviderProcess> {
        Err(CliError::Usage(
            "pi persistent Team Member is orchestrated by run_pi_team_member, not start_provider_runtime"
                .to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn run_delivery(
        &self,
        _store: &HarnessStore,
        _member: &ProviderLaunchProfile,
        _runtime: &ProviderProcess,
        _message: &RegistryMessage,
        _delivery_id: &str,
        _timeout_ms: u64,
        _project: &ProjectContext,
    ) -> CliResult<DeliveryOutcome> {
        Err(CliError::Usage(
            "pi one-shot delivery is not yet implemented; use the persistent Team Member path"
                .to_string(),
        ))
    }
}

/// All providers the harness recognises, in canonical display order.
pub(super) fn provider_registry() -> &'static [&'static dyn ProviderAdapter] {
    &[&CodexAdapter, &ClaudeAdapter, &KimiAdapter, &PiAdapter]
}

/// The adapter for a provider id, or `None` if unrecognised.
pub(super) fn provider_adapter(name: &str) -> Option<&'static dyn ProviderAdapter> {
    provider_registry()
        .iter()
        .copied()
        .find(|adapter| adapter.name() == name)
}

/// The supported provider ids, derived from the registry (single source of truth).
pub(super) fn supported_provider_names() -> Vec<&'static str> {
    provider_registry().iter().map(|a| a.name()).collect()
}

/// Build the standard error for a provider the harness does not recognise.
pub(super) fn unknown_provider_error(provider: &str, concern: &str) -> CliError {
    CliError::Usage(format!(
        "unknown provider {provider:?} for {concern}; supported providers: {}",
        supported_provider_names().join(", ")
    ))
}

/// Spawn (or attach) the runtime for a member, routed by `member.provider`.
pub(super) fn start_provider_runtime(
    store: &HarnessStore,
    member: &ProviderLaunchProfile,
) -> CliResult<ProviderProcess> {
    match provider_adapter(&member.provider) {
        Some(adapter) => adapter.start_runtime(store, member),
        None => Err(unknown_provider_error(&member.provider, "runtime start")),
    }
}
