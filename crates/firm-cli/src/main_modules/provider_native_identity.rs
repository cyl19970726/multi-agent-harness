use super::*;

pub(super) fn native_session_ref(
    member: &ProviderRuntimeProjection,
    native_session_id: impl Into<String>,
    native_locator_kind: &str,
) -> NativeSessionRef {
    let profile = member.provider_profile.as_ref();
    let native_session_id = native_session_id.into();
    let parent_native_session_id = member.native_session.as_ref().and_then(|session| {
        if session.native_session_id == native_session_id {
            // Resume preserves one native session; it is not a fork of itself.
            session
                .parent_native_session_id
                .clone()
                .filter(|parent| parent != &native_session_id)
        } else {
            Some(session.native_session_id.clone())
        }
    });
    NativeSessionRef {
        provider: member.provider.clone(),
        execution_mode: profile
            .map(|value| value.execution_mode.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        native_session_id,
        native_locator_kind: native_locator_kind.to_string(),
        provider_version: profile.and_then(|value| value.provider_version.clone()),
        adapter_contract_version: profile
            .and_then(|value| value.adapter_contract_version.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        availability: NativeSessionAvailability::Available,
        supports_resume: profile.is_some_and(|value| value.supports_resume),
        last_verified_at: Some(now_string()),
        parent_native_session_id,
    }
}

pub(super) fn provider_native_session_ref(
    provider: &str,
    native_session_id: impl Into<String>,
) -> NativeSessionRef {
    let profile = team_member_provider_profile(provider);
    let native_locator_kind = match provider {
        "codex" => "codex_rollout",
        "kimi" => "kimi_code_session",
        "claude" => "claude_project_session",
        _ => "provider_native_session",
    };
    NativeSessionRef {
        provider: provider.to_string(),
        execution_mode: profile.execution_mode,
        native_session_id: native_session_id.into(),
        native_locator_kind: native_locator_kind.to_string(),
        provider_version: profile.provider_version,
        adapter_contract_version: profile
            .adapter_contract_version
            .unwrap_or_else(|| "unknown".to_string()),
        availability: NativeSessionAvailability::Available,
        supports_resume: profile.supports_resume,
        last_verified_at: Some(now_string()),
        parent_native_session_id: None,
    }
}

pub(super) fn provider_version_output(provider: &str) -> Result<String, String> {
    let binary = match provider {
        "kimi" => resolve_kimi_bin(),
        "codex" => "codex".to_string(),
        "claude" => "claude".to_string(),
        "pi" => resolve_pi_bin(),
        other => other.to_string(),
    };
    let output = Command::new(&binary)
        .arg("--version")
        .output()
        .map_err(|error| format!("failed to run {binary} --version: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{binary} --version exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err(format!("{binary} --version returned no version"));
    }
    Ok(match provider {
        "codex" => raw.strip_prefix("codex-cli ").unwrap_or(&raw).to_string(),
        "claude" => raw.split_whitespace().next().unwrap_or(&raw).to_string(),
        _ => raw,
    })
}

/// Probe the executable that actually backs the persistent Team mode.
///
/// Claude Agent SDK bundles its own Claude Code executable. The unrelated
/// `claude` on PATH may be a different version, so using `claude --version`
/// here would audit the Workflow adapter while labeling the result as the Team
/// adapter. Live MemberRuns still replace this static package fact with
/// `system(init).claude_code_version`.
pub(super) fn team_member_provider_version_output(provider: &str) -> Result<String, String> {
    if provider != "claude" {
        return provider_version_output(provider);
    }

    let cwd = std::env::current_dir()
        .map_err(|error| format!("failed to resolve current directory: {error}"))?;
    let runner = claude_agent_sdk_runner_path(&cwd).map_err(|error| error.to_string())?;
    let mut visited = HashSet::new();
    for root in runner
        .ancestors()
        .chain(cwd.ancestors())
        .map(Path::to_path_buf)
        .filter(|root| visited.insert(root.clone()))
    {
        let package = root.join("node_modules/@anthropic-ai/claude-agent-sdk/package.json");
        if !package.is_file() {
            continue;
        }
        let bytes = fs::read(&package)
            .map_err(|error| format!("failed to read {}: {error}", package.display()))?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse {}: {error}", package.display()))?;
        if let Some(version) = json
            .get("claudeCodeVersion")
            .and_then(|value| value.as_str())
        {
            return Ok(version.to_string());
        }
        return Err(format!("{} has no claudeCodeVersion", package.display()));
    }

    Err(
        "Claude Agent SDK package.json was not found beside the configured member runner"
            .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Provider capacity preflight
//
// Capacity answers "can this account execute a turn right now"; it is NEVER
// derived from `ProviderIntegrationProfile.compatibility_status`, which answers
// "is this adapter reviewed against the installed provider version". Wave 2
// proved the two are independent: a reviewed-`current` Claude adapter returned
// 403 because the Harness process had no proxy, and a reviewed-`current` Kimi
// adapter returned a quota 403.
// ---------------------------------------------------------------------------

/// Non-secret environment keys that decide whether a Claude request can leave
/// this machine at all. Only presence is recorded; values are never copied
/// except for proxy URLs, which are not credentials.
pub(super) const CLAUDE_RUNTIME_CONTEXT_KEYS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "ANTHROPIC_BASE_URL",
];

/// Keys that indicate a credential exists, without revealing it.
pub(super) const CLAUDE_CREDENTIAL_ENV_KEYS: &[&str] =
    &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

#[derive(Clone, Copy)]
pub(super) struct CapacityProbeOptions {
    /// Issue a real, minimal provider request. Off by default because a canary
    /// consumes real quota; auth metadata alone must never be sold as one.
    pub(super) canary: bool,
    pub(super) timeout: Duration,
}

impl Default for CapacityProbeOptions {
    fn default() -> Self {
        Self {
            canary: false,
            timeout: Duration::from_secs(30),
        }
    }
}

pub(super) fn capacity_now() -> (String, u64) {
    let millis = current_unix_ms_u64();
    (format!("unix-ms:{millis}"), millis)
}

/// Resolve the execution mode a capacity snapshot describes.
///
/// Capacity claims are mode-specific: `codex_exec` and `codex_app_server` are
/// different products even though both are spelled "codex". Only the mode this
/// preflight actually probes may be named, so a caller cannot ask for one mode
/// and receive another mode's observation under its label.
pub(super) fn capacity_execution_mode(
    provider: &str,
    requested: Option<&str>,
) -> CliResult<String> {
    let probed = team_member_provider_profile(provider).execution_mode;
    match requested.map(str::trim).filter(|mode| !mode.is_empty()) {
        Some(mode) if mode == probed => Ok(probed),
        Some(mode) => Err(CliError::Usage(format!(
            "capacity is observed for {provider}'s Agent Team mode `{probed}`, not `{mode}`; \
             a snapshot must never label another mode's observation"
        ))),
        None => Ok(probed),
    }
}

/// Read one Codex account's capacity through the reviewed app-server account
/// RPCs. The client is torn down without ever opening a thread.
pub(super) fn codex_capacity_probe(
    execution_mode: &str,
    cwd: &Path,
    options: CapacityProbeOptions,
) -> ProviderCapacitySnapshot {
    let (observed_at, observed_unix_ms) = capacity_now();
    match codex_app_server::CodexAppServerClient::connect(cwd, &[])
        .and_then(|mut client| client.read_account_capacity(options.timeout))
    {
        Ok(read) => codex_app_server::codex_capacity_snapshot(
            execution_mode,
            &read,
            &observed_at,
            observed_unix_ms,
        ),
        Err(error) => ProviderCapacitySnapshot::unknown(
            "codex",
            execution_mode,
            observed_at,
            observed_unix_ms,
            ProviderCapacityEvidence::ProbeFailed,
            format!(
                "codex app-server account read failed before returning a provider answer: {error}"
            ),
        ),
    }
}

/// Observe the non-secret runtime facts that decide whether a Claude request
/// can reach the API. This is what turns "403" into an actionable diagnosis.
/// Reduce a proxy/base URL to the routing facts an operator needs, with any
/// credential removed.
///
/// Corporate proxies are routinely `http://user:secret@host:8080`, and a
/// gateway base URL can carry a token in its path or query. The durable ledger
/// and CI logs must never receive either, so keep only scheme, host, and port.
pub(super) fn redact_url_to_origin(raw: &str) -> String {
    let trimmed = raw.trim();
    let (scheme, rest) = match trimmed.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, trimmed),
    };
    // Cut the authority FIRST (RFC 3986: it ends at the first `/`, `?`, or
    // `#`), then strip userinfo INSIDE it. Searching the whole string for `@`
    // would mis-parse `https://host/p?email=a@b.com` as host `b.com`, dropping
    // the real origin and echoing part of the query.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let redacted_userinfo = authority.contains('@');
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
        .trim();
    let origin = match scheme {
        Some(scheme) if !host.is_empty() => format!("{scheme}://{host}"),
        _ if !host.is_empty() => host.to_string(),
        // An unparsable value is reported as present without content rather
        // than echoed: presence is the fact, the value is not.
        _ => return "set (value withheld)".to_string(),
    };
    if redacted_userinfo {
        format!("{origin} (credentials redacted)")
    } else {
        origin
    }
}

pub(super) fn claude_runtime_context_facts() -> Vec<ProviderRuntimeContextFact> {
    let mut facts: Vec<ProviderRuntimeContextFact> = CLAUDE_RUNTIME_CONTEXT_KEYS
        .iter()
        .map(|key| {
            let value = std::env::var(key).ok().filter(|raw| !raw.trim().is_empty());
            ProviderRuntimeContextFact {
                key: (*key).to_string(),
                present: value.is_some(),
                // Routing only. A proxy URL can embed credentials, so it is
                // reduced to its origin before it reaches the ledger.
                note: Some(
                    value
                        .as_deref()
                        .map(redact_url_to_origin)
                        .unwrap_or_else(|| "absent".to_string()),
                ),
            }
        })
        .collect();
    for key in CLAUDE_CREDENTIAL_ENV_KEYS {
        facts.push(ProviderRuntimeContextFact {
            key: (*key).to_string(),
            present: std::env::var(key)
                .ok()
                .is_some_and(|raw| !raw.trim().is_empty()),
            // Never record the value: presence is the only non-secret fact.
            note: Some("value withheld".to_string()),
        });
    }
    facts
}

pub(super) fn claude_has_proxy_configured(facts: &[ProviderRuntimeContextFact]) -> bool {
    facts.iter().any(|fact| {
        fact.present
            && matches!(
                fact.key.as_str(),
                "HTTPS_PROXY"
                    | "https_proxy"
                    | "HTTP_PROXY"
                    | "http_proxy"
                    | "ALL_PROXY"
                    | "all_proxy"
            )
    })
}

/// Local credential metadata for Claude. `credentials.json` is only one of the
/// stores Claude Code uses (the macOS Keychain is another), so its absence is
/// NOT evidence of a missing credential and must never become `unauthorized`.
pub(super) fn claude_auth_metadata() -> (ProviderAccountRef, String) {
    for key in CLAUDE_CREDENTIAL_ENV_KEYS {
        if std::env::var(key)
            .ok()
            .is_some_and(|raw| !raw.trim().is_empty())
        {
            return (
                ProviderAccountRef {
                    source: "api_key_env".to_string(),
                    identifier: Some((*key).to_string()),
                    plan: None,
                },
                format!("{key} is set in the Harness process environment"),
            );
        }
    }
    let credentials = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".claude/.credentials.json"));
    match credentials {
        Some(path) if path.is_file() => (
            ProviderAccountRef {
                source: "oauth_credentials_file".to_string(),
                identifier: Some(path.display().to_string()),
                plan: None,
            },
            "a local Claude credential file exists".to_string(),
        ),
        _ => (
            ProviderAccountRef::unknown(),
            "no Claude credential was found in the process environment or the local credential \
             file; Claude Code may still hold one in an OS keychain, so this is not evidence of a \
             missing credential"
                .to_string(),
        ),
    }
}

/// Explain a failed Claude canary using the observed runtime context.
///
/// The Wave 2 failure was NOT an account limit: `claude auth status` reported
/// logged-in while the request returned 403, because the Harness process had no
/// HTTP(S)_PROXY and this machine's direct egress to the API is blocked. The
/// same request succeeded through the proxy.
/// The single wording for a proxy-shaped Claude failure.
///
/// Shared by the live canary and by the recorded-failure merge so the two
/// paths cannot drift into contradicting each other about the same 403.
pub(super) fn claude_missing_proxy_diagnosis() -> String {
    "a real Claude request failed while the Harness process has no HTTP(S)_PROXY set. \
     Live Wave 2 evidence: local auth metadata reported logged-in and the identical \
     request succeeded once the proxy was exported, so treat this as missing proxy/runtime \
     context rather than an account limit until the request is retried through the proxy."
        .to_string()
}

pub(super) fn claude_canary_diagnosis(
    failure: &str,
    facts: &[ProviderRuntimeContextFact],
) -> (ProviderCapacityState, String) {
    let lowered = failure.to_lowercase();
    let auth_shaped = ["403", "401", "forbidden", "not allowed", "authenticate"]
        .iter()
        .any(|needle| lowered.contains(needle));
    let network_shaped = [
        "econnrefused",
        "enotfound",
        "etimedout",
        "connect",
        "network",
        "tls",
        "certificate",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    if !claude_has_proxy_configured(facts) && (auth_shaped || network_shaped) {
        return (
            // A blocked egress path is a runtime-context gap, not proof that
            // the account is unauthorized. Reporting it as `unauthorized`
            // would gate a healthy account behind a missing env var.
            ProviderCapacityState::Unknown,
            claude_missing_proxy_diagnosis(),
        );
    }
    if auth_shaped {
        return (
            ProviderCapacityState::Unauthorized,
            "a real Claude request was rejected as unauthorized while a proxy is configured, so \
             the credential itself is the most likely cause"
                .to_string(),
        );
    }
    if ["429", "rate limit", "quota", "usage limit"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        return (
            ProviderCapacityState::Exhausted,
            "a real Claude request was rejected for exceeding a usage limit".to_string(),
        );
    }
    (
        ProviderCapacityState::Unknown,
        "a real Claude request failed for a reason this adapter has not reviewed".to_string(),
    )
}

/// Run one bounded, real Claude request.
///
/// This shares credentials and HTTP egress with the `claude_agent_sdk` Team
/// mode but is NOT the SDK runtime; the snapshot says so explicitly rather than
/// implying the Team runtime itself was exercised.
pub(super) fn claude_execution_canary(cwd: &Path, timeout: Duration) -> Result<String, String> {
    let mut command = Command::new("claude");
    command
        .arg("-p")
        .arg("Reply with exactly: HARNESS-CAPACITY-OK")
        .arg("--output-format")
        .arg("json")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_provider_child_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to spawn the Claude canary: {error}"))?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });
    let mut guard = ProviderChildGuard::new(child);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match guard.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(format!("failed to inspect the Claude canary: {error}")),
        }
        if Instant::now() >= deadline {
            // Dropping the guard kills the whole process group, so a wedged
            // canary can never outlive the preflight.
            return Err(format!(
                "the Claude canary did not answer within {}s",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // Kill the whole isolated group BEFORE joining. `claude` can leave a
    // grandchild (an MCP stdio server, a helper) holding the inherited stdout
    // fd; without this the reader never sees EOF and `join()` blocks past the
    // caller's timeout. This is the same failure the NDJSON worker path
    // already documents.
    kill_worker_tree(&mut guard);
    guard.disarm();
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default().trim().to_string();
    if !status.success() {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(format!(
            "claude canary exited {}: {}",
            status,
            detail.trim()
        ));
    }
    // `claude -p --output-format json` reports API failures in-band: the
    // process still exits 0 with `is_error: true`. Trusting the exit code here
    // is exactly the "provider-down round looked completed" defect.
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|error| format!("claude canary returned unparsable JSON: {error}"))?;
    if parsed.get("is_error").and_then(|value| value.as_bool()) == Some(true) {
        return Err(parsed
            .get("result")
            .and_then(|value| value.as_str())
            .unwrap_or("claude canary reported is_error without a result")
            .to_string());
    }
    Ok(parsed
        .get("result")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string())
}

/// Claude capacity. Anthropic does not permit third-party products to surface
/// claude.ai rate limits without prior approval, so this NEVER reports a quota
/// percentage. It reports the auth/runtime facts it can observe and, when a
/// canary is requested, what a real request actually did.
pub(super) fn claude_capacity_probe(
    execution_mode: &str,
    cwd: &Path,
    options: CapacityProbeOptions,
) -> ProviderCapacitySnapshot {
    let (observed_at, observed_unix_ms) = capacity_now();
    let runtime_context = claude_runtime_context_facts();
    let (account, metadata_detail) = claude_auth_metadata();
    let mut snapshot = ProviderCapacitySnapshot {
        provider: "claude".to_string(),
        execution_mode: execution_mode.to_string(),
        account,
        // Auth metadata proves a credential exists, never that a request would
        // succeed. It must stay `unknown`.
        state: ProviderCapacityState::Unknown,
        observed_at,
        observed_unix_ms,
        reset_at: None,
        evidence_source: ProviderCapacityEvidence::AuthMetadata,
        confidence: ProviderCapacityConfidence::Unknown,
        windows: Vec::new(),
        diagnosis: None,
        runtime_context,
        detail: Some(format!(
            "{metadata_detail}. Auth metadata cannot prove capacity; run the preflight with \
             --canary for a real request. Claude rate limits are not surfaced."
        )),
    };
    if !claude_has_proxy_configured(&snapshot.runtime_context) {
        snapshot.diagnosis = Some(
            "no HTTP(S)_PROXY is set in the Harness process. On a host whose direct egress to the \
             Claude API is blocked this alone makes every member turn fail with 403 while local \
             auth metadata still reports logged-in."
                .to_string(),
        );
    }
    if !options.canary {
        return snapshot;
    }
    match claude_execution_canary(cwd, options.timeout) {
        Ok(reply) => {
            snapshot.state = ProviderCapacityState::Available;
            snapshot.evidence_source = ProviderCapacityEvidence::ExecutionCanary;
            snapshot.confidence = ProviderCapacityConfidence::Observed;
            snapshot.diagnosis = None;
            snapshot.detail = Some(format!(
                "a real bounded `claude -p` request succeeded ({}). It shares credentials and HTTP \
                 egress with claude_agent_sdk but is not the Agent SDK runtime itself. No rate \
                 limit is reported.",
                reply.trim().chars().take(40).collect::<String>()
            ));
        }
        Err(failure) => {
            let (state, diagnosis) = claude_canary_diagnosis(&failure, &snapshot.runtime_context);
            snapshot.state = state;
            snapshot.evidence_source = ProviderCapacityEvidence::ExecutionCanary;
            snapshot.confidence = if state == ProviderCapacityState::Unknown {
                ProviderCapacityConfidence::Unknown
            } else {
                ProviderCapacityConfidence::Inferred
            };
            snapshot.diagnosis = Some(diagnosis);
            snapshot.detail = Some(format!(
                "a real bounded `claude -p` request failed: {failure}"
            ));
        }
    }
    snapshot
}

/// Kimi capacity. The reviewed ACP surface for `kimi_acp` is `initialize`,
/// `session/{new,resume,load,set_config_option,prompt,cancel,update,
/// request_permission}`. None of them reports quota, so the only honest answer
/// is `unknown` — never a synthesised percentage.
///
/// A terminal failure does not help either: ACP has no HTTP-status error
/// channel, and a real Kimi failure is journalled as `action_type=error`, not
/// as a structured `provider_error`. There is no source to promote, so this
/// stays `unknown` in every case.
pub(super) fn kimi_capacity_probe(execution_mode: &str) -> ProviderCapacitySnapshot {
    let (observed_at, observed_unix_ms) = capacity_now();
    let mut snapshot = ProviderCapacitySnapshot::unknown(
        "kimi",
        execution_mode,
        observed_at,
        observed_unix_ms,
        ProviderCapacityEvidence::NotExposed,
        "the reviewed Kimi ACP surface exposes no account, quota, or rate-limit method, so no \
         usage number can be reported. ACP also has no HTTP-status error channel, so a terminal \
         failure cannot make capacity observable either; Kimi stays unknown until a reviewed \
         quota or structured-error API exists.",
    );
    snapshot.account = ProviderAccountRef {
        source: "kimi_code_local_login".to_string(),
        identifier: None,
        plan: None,
    };
    snapshot
}

/// Provider-neutral entry point. Unregistered providers are honestly unknown
/// rather than inheriting another provider's answer.
pub(super) fn provider_capacity_probe(
    provider: &str,
    execution_mode: &str,
    cwd: &Path,
    options: CapacityProbeOptions,
) -> ProviderCapacitySnapshot {
    match provider {
        "codex" => codex_capacity_probe(execution_mode, cwd, options),
        "claude" => claude_capacity_probe(execution_mode, cwd, options),
        "kimi" => kimi_capacity_probe(execution_mode),
        other => {
            let (observed_at, observed_unix_ms) = capacity_now();
            ProviderCapacitySnapshot::unknown(
                other,
                execution_mode,
                observed_at,
                observed_unix_ms,
                ProviderCapacityEvidence::NotExposed,
                "no capacity probe is registered for this provider",
            )
        }
    }
}
