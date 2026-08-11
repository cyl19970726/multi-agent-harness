//! Minimal Codex app-server V2 client for interactive Agent Team Members.
//!
//! The client intentionally owns only transport and provider lifecycle. Product
//! routing, durable PendingInteraction records, MemberAction reduction, and
//! authority remain in the TeamRun orchestrator.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use harness_core::{
    ProviderAccountRef, ProviderCapacityConfidence, ProviderCapacityEvidence,
    ProviderCapacitySnapshot, ProviderCapacityState, ProviderCapacityWindow,
};

use crate::{kill_worker_tree, CliError, CliResult};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DEVELOPMENT_SANDBOX: &str = "danger-full-access";
const DEVELOPMENT_APPROVAL_POLICY: &str = "never";

/// Reviewed app-server RPCs used by the capacity preflight. Both are reads;
/// neither opens, resumes, or names a thread.
pub(crate) const ACCOUNT_READ_METHOD: &str = "account/read";
pub(crate) const ACCOUNT_RATE_LIMITS_READ_METHOD: &str = "account/rateLimits/read";

/// A window at or above this used percentage is reported as `limited`.
const LIMITED_USED_PERCENT: i64 = 90;
/// A window at or above this used percentage is reported as `exhausted`.
const EXHAUSTED_USED_PERCENT: i64 = 100;

fn thread_open_params(
    cwd: &Path,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    resume_thread_id: Option<&str>,
    sandbox: &str,
    approval_policy: &str,
) -> serde_json::Value {
    let mut params = match resume_thread_id {
        Some(thread_id) => serde_json::json!({
            "threadId": thread_id,
            "cwd": cwd,
            "model": model,
            "serviceTier": service_tier,
            "sandbox": sandbox,
            "approvalPolicy": approval_policy
        }),
        None => serde_json::json!({
            "cwd": cwd,
            "model": model,
            "serviceTier": service_tier,
            "sandbox": sandbox,
            "approvalPolicy": approval_policy,
            "ephemeral": false
        }),
    };
    if let Some(reasoning_effort) = reasoning_effort {
        params["config"] = serde_json::json!({
            "model_reasoning_effort": reasoning_effort,
        });
    }
    params
}

fn thread_name_params(thread_id: &str, member_name: &str) -> serde_json::Value {
    serde_json::json!({
        "threadId": thread_id,
        "name": format!("Agent Team · {}", member_name.trim())
    })
}

fn effective_thread_model(response: &serde_json::Value) -> Option<String> {
    [
        // Current app-server responses expose the effective model alongside
        // `thread`, because it is resolved from the active provider/config.
        response
            .pointer("/result/model")
            .and_then(|value| value.as_str()),
        // Keep accepting the earlier nested response shape used by reviewed
        // app-server versions and deterministic fixtures.
        response
            .pointer("/result/thread/model")
            .and_then(|value| value.as_str()),
    ]
    .into_iter()
    .flatten()
    .find(|model| !model.trim().is_empty())
    .map(str::to_string)
}

fn effective_thread_reasoning_effort(response: &serde_json::Value) -> Option<String> {
    response
        .pointer("/result/reasoningEffort")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn effective_thread_service_tier(response: &serde_json::Value) -> Option<String> {
    response
        .pointer("/result/serviceTier")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn require_requested_setting(
    label: &str,
    requested: Option<&str>,
    effective: Option<&str>,
) -> CliResult<()> {
    if let Some(requested) = requested {
        if effective != Some(requested) {
            return Err(CliError::Usage(format!(
                "codex app-server did not confirm requested {label} `{requested}`; effective value was {}",
                effective.unwrap_or("<none>")
            )));
        }
    }
    Ok(())
}

pub(crate) struct CodexAppServerClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    next_request_id: u64,
    pending: Arc<Mutex<HashMap<u64, Sender<serde_json::Value>>>>,
    incoming: Receiver<serde_json::Value>,
    reader: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<String>>,
    thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    collaboration_mode: &'static str,
}

pub(crate) struct CodexAppServerSpawnOptions<'a> {
    pub(crate) model: Option<&'a str>,
    pub(crate) reasoning_effort: Option<&'a str>,
    pub(crate) service_tier: Option<&'a str>,
    pub(crate) resume_thread_id: Option<&'a str>,
    pub(crate) member_name: &'a str,
    pub(crate) collaboration_env: &'a [(String, String)],
    pub(crate) plan_mode: bool,
    /// NodeDaemon-owned AgentSessions pass the frozen AgentIdentity ceiling
    /// here. Team-runtime callers may omit both to retain their separately
    /// governed development policy.
    pub(crate) sandbox: Option<&'a str>,
    pub(crate) approval_policy: Option<&'a str>,
}

impl CodexAppServerClient {
    pub(crate) fn spawn(cwd: &Path, options: CodexAppServerSpawnOptions<'_>) -> CliResult<Self> {
        // Transport + protocol handshake only. A thread is opened below; the
        // capacity preflight deliberately stops after this call.
        let mut client = Self::connect(cwd, options.collaboration_env)?;
        client.collaboration_mode = if options.plan_mode { "plan" } else { "default" };
        // Temporary development policy: interactive Agent Team members receive
        // the same full execution permission as batch Codex members. Owned paths
        // remain a coordination/acceptance boundary, not a provider sandbox.
        let method = if options.resume_thread_id.is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        let params = thread_open_params(
            cwd,
            options.model,
            options.reasoning_effort,
            options.service_tier,
            options.resume_thread_id,
            options.sandbox.unwrap_or(DEVELOPMENT_SANDBOX),
            options
                .approval_policy
                .unwrap_or(DEVELOPMENT_APPROVAL_POLICY),
        );
        let response = client.request_blocking(method, params, HANDSHAKE_TIMEOUT)?;
        client.thread_id = response
            .pointer("/result/thread/id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                CliError::Usage(format!("codex {method} omitted thread id: {response}"))
            })?
            .to_string();
        client.model = effective_thread_model(&response)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "codex {method} omitted the effective thread model required for collaborationMode: {response}"
                ))
            })?;
        client.reasoning_effort = effective_thread_reasoning_effort(&response);
        client.service_tier = effective_thread_service_tier(&response);
        require_requested_setting(
            "reasoning effort",
            options.reasoning_effort,
            client.reasoning_effort.as_deref(),
        )?;
        require_requested_setting(
            "service tier",
            options.service_tier,
            client.service_tier.as_deref(),
        )?;
        client.request_blocking(
            "thread/name/set",
            thread_name_params(&client.thread_id, options.member_name),
            HANDSHAKE_TIMEOUT,
        )?;
        Ok(client)
    }

    /// Start one app-server process and complete the protocol handshake
    /// WITHOUT opening a thread.
    ///
    /// `initialize` + `initialized` already produce a valid JSON-RPC peer, so
    /// account reads can run here. Keeping this phase separate is what lets the
    /// capacity preflight observe an account without creating a native session,
    /// a rollout, or a billable turn.
    pub(crate) fn connect(cwd: &Path, env: &[(String, String)]) -> CliResult<Self> {
        let mut command = Command::new("codex");
        command
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .envs(env.iter().cloned())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            CliError::Usage(format!("failed to spawn codex app-server: {error}"))
        })?;
        let stdin =
            BufWriter::new(child.stdin.take().ok_or_else(|| {
                CliError::Usage("codex app-server stdin unavailable".to_string())
            })?);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CliError::Usage("codex app-server stdout unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CliError::Usage("codex app-server stderr unavailable".to_string()))?;
        let pending: Arc<Mutex<HashMap<u64, Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = Arc::clone(&pending);
        let (incoming_tx, incoming) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(frame) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                let response_id = frame.get("id").and_then(|id| id.as_u64());
                if frame.get("method").is_none() {
                    if let Some(id) = response_id {
                        if let Some(sender) = pending_reader
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .remove(&id)
                        {
                            let _ = sender.send(frame);
                            continue;
                        }
                    }
                }
                if incoming_tx.send(frame).is_err() {
                    break;
                }
            }
            // Drop response senders so an RPC waiting beneath a lost
            // transport observes disconnection immediately instead of
            // misreporting a handshake timeout.
            pending_reader
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
        });
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let stderr_writer = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            *stderr_writer
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = text;
        });

        let mut client = Self {
            child,
            stdin,
            next_request_id: 0,
            pending,
            incoming,
            reader: Some(reader),
            stderr_tail,
            thread_id: String::new(),
            model: String::new(),
            reasoning_effort: None,
            service_tier: None,
            collaboration_mode: "default",
        };
        client.request_blocking(
            "initialize",
            serde_json::json!({
                "clientInfo": {
                    "name": "star_harness",
                    "title": "Star Harness Agent Team",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {"experimentalApi": true}
            }),
            HANDSHAKE_TIMEOUT,
        )?;
        client.notify("initialized", serde_json::json!({}))?;
        Ok(client)
    }

    /// Read the account identity and rate limits of the signed-in Codex
    /// account. Both RPCs are reads; no thread is started, resumed, or named,
    /// so this consumes no rollout and no model turn.
    pub(crate) fn read_account_capacity(
        &mut self,
        timeout: Duration,
    ) -> CliResult<CodexAccountCapacityRead> {
        // Assert BEFORE the reads. Checking afterwards would pass trivially,
        // because neither read can set a thread id; the invariant worth
        // guarding is that the CALLER never opened one.
        debug_assert!(
            self.thread_id.is_empty(),
            "the capacity preflight must run on a client that never opened a thread"
        );
        let account = self.request_blocking(ACCOUNT_READ_METHOD, serde_json::json!({}), timeout)?;
        let rate_limits = self.request_blocking(
            ACCOUNT_RATE_LIMITS_READ_METHOD,
            serde_json::json!({}),
            timeout,
        )?;
        Ok(CodexAccountCapacityRead {
            account: account
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            rate_limits: rate_limits
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        })
    }

    pub(crate) fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    pub(crate) fn service_tier(&self) -> Option<&str> {
        self.service_tier.as_deref()
    }

    pub(crate) fn ensure_transport_alive(&mut self) -> CliResult<()> {
        let reader_ended = self
            .reader
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished);
        let child_ended = self.child.try_wait().map_err(|error| {
            CliError::Usage(format!("failed to inspect codex app-server: {error}"))
        })?;
        if reader_ended || child_ended.is_some() {
            return Err(CliError::Usage(format!(
                "codex app-server transport disconnected{}",
                self.stderr_suffix()
            )));
        }
        Ok(())
    }

    pub(crate) fn start_turn(&mut self, text: &str) -> CliResult<String> {
        let response = self.request_blocking(
            "turn/start",
            serde_json::json!({
                "threadId": self.thread_id,
                "input": [{"type": "text", "text": text}],
                "model": self.model,
                "effort": self.reasoning_effort,
                "serviceTier": self.service_tier,
                // app-server collaboration modes are a per-turn experimental
                // protocol field, not a `codex -c` configuration key. Send a
                // complete preset so the provider's native turn_context
                // records the requested Plan/default boundary.
                "collaborationMode": {
                    "mode": self.collaboration_mode,
                    "settings": {
                        "model": self.model,
                        "reasoning_effort": self.reasoning_effort,
                        "developer_instructions": null
                    }
                }
            }),
            HANDSHAKE_TIMEOUT,
        )?;
        response
            .pointer("/result/turn/id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| CliError::Usage(format!("codex turn/start omitted turn id: {response}")))
    }

    pub(crate) fn steer(&mut self, turn_id: &str, text: &str) -> CliResult<String> {
        let response = self.request_blocking(
            "turn/steer",
            serde_json::json!({
                "threadId": self.thread_id,
                "expectedTurnId": turn_id,
                "input": [{"type": "text", "text": text}]
            }),
            HANDSHAKE_TIMEOUT,
        )?;
        Ok(response
            .pointer("/result/turnId")
            .and_then(|value| value.as_str())
            .unwrap_or(turn_id)
            .to_string())
    }

    pub(crate) fn interrupt(&mut self, turn_id: &str) -> CliResult<()> {
        self.request_blocking(
            "turn/interrupt",
            serde_json::json!({"threadId": self.thread_id, "turnId": turn_id}),
            HANDSHAKE_TIMEOUT,
        )?;
        Ok(())
    }

    pub(crate) fn recv(&self, timeout: Duration) -> Result<serde_json::Value, RecvTimeoutError> {
        self.incoming.recv_timeout(timeout)
    }

    pub(crate) fn respond(
        &mut self,
        id: &serde_json::Value,
        result: serde_json::Value,
    ) -> CliResult<()> {
        self.write(&serde_json::json!({"id": id, "result": result}))
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> CliResult<()> {
        self.write(&serde_json::json!({"method": method, "params": params}))
    }

    fn request_blocking(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> CliResult<serde_json::Value> {
        self.next_request_id += 1;
        let id = self.next_request_id;
        let (tx, rx) = channel();
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, tx);
        self.write(&serde_json::json!({"id": id, "method": method, "params": params}))?;
        let frame = rx.recv_timeout(timeout).map_err(|error| {
            self.pending
                .lock()
                .unwrap_or_else(|lock_error| lock_error.into_inner())
                .remove(&id);
            let failure = match error {
                RecvTimeoutError::Timeout => "timed out",
                RecvTimeoutError::Disconnected => "transport disconnected",
            };
            CliError::Usage(format!(
                "codex app-server {method} {failure}{}",
                self.stderr_suffix()
            ))
        })?;
        if let Some(error) = frame.get("error") {
            return Err(CliError::Usage(format!(
                "codex app-server {method} failed: {error}{}",
                self.stderr_suffix()
            )));
        }
        Ok(frame)
    }

    fn write(&mut self, frame: &serde_json::Value) -> CliResult<()> {
        serde_json::to_writer(&mut self.stdin, frame)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn stderr_suffix(&self) -> String {
        let tail = self
            .stderr_tail
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let trimmed = tail.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            format!(
                "; stderr: {}",
                trimmed
                    .chars()
                    .rev()
                    .take(1200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )
        }
    }
}

/// Raw `result` payloads of the two reviewed account reads.
pub(crate) struct CodexAccountCapacityRead {
    pub(crate) account: serde_json::Value,
    pub(crate) rate_limits: serde_json::Value,
}

/// Translate `account/read` into the provider-neutral account boundary.
///
/// The account is the unit capacity applies to: two Codex members can hold
/// different logins, so a snapshot without an account source is not usable.
pub(crate) fn account_ref_from_account_read(account: &serde_json::Value) -> ProviderAccountRef {
    let Some(inner) = account.get("account").filter(|value| !value.is_null()) else {
        return ProviderAccountRef {
            source: "signed_out".to_string(),
            identifier: None,
            plan: None,
        };
    };
    match inner.get("type").and_then(|value| value.as_str()) {
        Some("chatgpt") => ProviderAccountRef {
            source: "chatgpt".to_string(),
            identifier: inner
                .get("email")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            plan: inner
                .get("planType")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        },
        Some("apiKey") => ProviderAccountRef {
            source: "api_key".to_string(),
            identifier: None,
            plan: None,
        },
        Some("amazonBedrock") => ProviderAccountRef {
            source: "amazon_bedrock".to_string(),
            identifier: None,
            plan: None,
        },
        _ => ProviderAccountRef::unknown(),
    }
}

fn unix_seconds_to_harness_timestamp(seconds: i64) -> Option<String> {
    (seconds > 0).then(|| format!("unix-ms:{}", (seconds as i128) * 1000))
}

/// Read a provider number that the schema types as an integer but a future
/// wire format could send as a float. `as_i64` alone returns `None` for `3.0`,
/// which would silently empty every window and make the probe inert.
fn provider_number(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn window_from_json(
    label: &str,
    limit_id: Option<&str>,
    window: &serde_json::Value,
) -> Option<ProviderCapacityWindow> {
    // `usedPercent` is the only required field of a provider window. Without it
    // there is no number to report, and inventing one is exactly what this
    // Work forbids.
    let used_percent = provider_number(window.get("usedPercent"))?;
    Some(ProviderCapacityWindow {
        label: label.to_string(),
        limit_id: limit_id.map(str::to_string),
        used_percent: Some(used_percent),
        window_duration_mins: provider_number(window.get("windowDurationMins")),
        resets_at: provider_number(window.get("resetsAt"))
            .and_then(unix_seconds_to_harness_timestamp),
    })
}

/// Windows of ONE bucket snapshot, used both for reporting and for the state
/// claim made from that same bucket.
fn windows_of_snapshot(
    limit_id: Option<&str>,
    snapshot: &serde_json::Value,
) -> Vec<ProviderCapacityWindow> {
    let name = limit_id
        .or_else(|| snapshot.get("limitId").and_then(|value| value.as_str()))
        .unwrap_or("account");
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let window = snapshot.get(key).filter(|value| !value.is_null())?;
            window_from_json(&format!("{name}.{key}"), Some(name), window)
        })
        .collect()
}

/// The single bucket a state claim may be made from.
///
/// Buckets are NOT interchangeable: a saturated per-model bucket must not be
/// read as "the account is out of capacity" while the account bucket still has
/// headroom. `rateLimits` is the provider's own account-level mirror, so it is
/// the only bucket that speaks for the account. A payload that reports several
/// buckets and no account mirror is not attributable, and stays unknown.
fn classification_snapshot(rate_limits: &serde_json::Value) -> Option<&serde_json::Value> {
    if let Some(account) = rate_limits
        .get("rateLimits")
        .filter(|value| value.is_object())
    {
        return Some(account);
    }
    let by_limit = rate_limits
        .get("rateLimitsByLimitId")
        .and_then(|value| value.as_object())?;
    match by_limit.len() {
        1 => by_limit.values().next(),
        _ => None,
    }
}

/// Flatten every reported bucket into provider-neutral windows FOR REPORTING.
///
/// Every metered bucket is visible so an operator can see which one is hot.
/// The state claim is made from one bucket only — see
/// [`classification_snapshot`] — because a saturated per-model bucket is not an
/// account-level verdict.
pub(crate) fn capacity_windows_from_rate_limits(
    rate_limits: &serde_json::Value,
) -> Vec<ProviderCapacityWindow> {
    match rate_limits
        .get("rateLimitsByLimitId")
        .and_then(|value| value.as_object())
    {
        Some(by_limit) if !by_limit.is_empty() => {
            let mut keys = by_limit.keys().collect::<Vec<_>>();
            keys.sort();
            keys.into_iter()
                .flat_map(|key| windows_of_snapshot(Some(key.as_str()), &by_limit[key]))
                .collect()
        }
        _ => rate_limits
            .get("rateLimits")
            .filter(|value| !value.is_null())
            .map(|snapshot| windows_of_snapshot(None, snapshot))
            .unwrap_or_default(),
    }
}

/// Classify the reviewed `account/rateLimits/read` payload.
///
/// Only provider-reported signals move the state off `available`: a reached
/// rate-limit type, a reached spend control, or a window the provider itself
/// reported at or above the thresholds. An absent or unparsable payload stays
/// `unknown`.
pub(crate) fn codex_capacity_snapshot(
    execution_mode: &str,
    read: &CodexAccountCapacityRead,
    observed_at: &str,
    observed_unix_ms: u64,
) -> ProviderCapacitySnapshot {
    let account = account_ref_from_account_read(&read.account);
    let windows = capacity_windows_from_rate_limits(&read.rate_limits);
    let requires_auth = read
        .account
        .get("requiresOpenaiAuth")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    // A signed-out account outranks any usage number: there is nothing to spend.
    if account.source == "signed_out" && requires_auth {
        return ProviderCapacitySnapshot {
            provider: "codex".to_string(),
            execution_mode: execution_mode.to_string(),
            account,
            state: ProviderCapacityState::Unauthorized,
            observed_at: observed_at.to_string(),
            observed_unix_ms,
            reset_at: None,
            // This came from the provider's account endpoint, not from a quota
            // reading: it proves credential absence, not spent capacity.
            evidence_source: ProviderCapacityEvidence::AuthMetadata,
            confidence: ProviderCapacityConfidence::Observed,
            windows,
            diagnosis: None,
            runtime_context: Vec::new(),
            detail: Some(
                "codex app-server reports no signed-in account while OpenAI auth is required"
                    .to_string(),
            ),
        };
    }
    // Every state signal below is read from ONE bucket, so a reached flag can
    // never be paired with another bucket's percentage.
    let Some(bucket) = classification_snapshot(&read.rate_limits) else {
        let detail = if read.rate_limits.is_null() {
            "codex app-server returned no rate-limit payload".to_string()
        } else {
            "codex app-server reported several metered buckets and no account-level mirror, so no \
             bucket speaks for the account"
                .to_string()
        };
        return ProviderCapacitySnapshot {
            provider: "codex".to_string(),
            execution_mode: execution_mode.to_string(),
            account,
            state: ProviderCapacityState::Unknown,
            observed_at: observed_at.to_string(),
            observed_unix_ms,
            reset_at: None,
            evidence_source: ProviderCapacityEvidence::ProviderQuotaApi,
            confidence: ProviderCapacityConfidence::Unknown,
            windows,
            diagnosis: None,
            runtime_context: Vec::new(),
            detail: Some(detail),
        };
    };
    let bucket_windows = windows_of_snapshot(None, bucket);
    let reached_type = bucket
        .get("rateLimitReachedType")
        .and_then(|value| value.as_str());
    let spend_control_reached = bucket
        .get("spendControlReached")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let peak = bucket_windows
        .iter()
        .filter_map(|window| window.used_percent)
        .max();
    let (state, detail) = match (reached_type, spend_control_reached, peak) {
        (Some(reached), _, _) => (
            ProviderCapacityState::Exhausted,
            format!("provider reported rateLimitReachedType {reached}"),
        ),
        (None, true, _) => (
            ProviderCapacityState::Exhausted,
            "provider reported spendControlReached".to_string(),
        ),
        // A payload with no parsable window is not proof of headroom.
        (None, false, None) => (
            ProviderCapacityState::Unknown,
            "codex app-server reported no usable rate-limit window".to_string(),
        ),
        (None, false, Some(peak)) if peak >= EXHAUSTED_USED_PERCENT => (
            ProviderCapacityState::Exhausted,
            format!("provider reported {peak}% of the account window used"),
        ),
        (None, false, Some(peak)) if peak >= LIMITED_USED_PERCENT => (
            ProviderCapacityState::Limited,
            format!("provider reported {peak}% of the account window used"),
        ),
        (None, false, Some(peak)) => (
            ProviderCapacityState::Available,
            format!("highest reported account window usage is {peak}%"),
        ),
    };
    // Report a reset only for a state a reset would clear, and take it from the
    // LATEST constraining window: the account is usable again only once every
    // window that is holding it back has reopened.
    let reset_at = match state {
        ProviderCapacityState::Exhausted | ProviderCapacityState::Limited => {
            let latest_reset = |windows: &mut dyn Iterator<Item = &ProviderCapacityWindow>| {
                windows
                    .filter_map(|window| window.resets_at.as_deref())
                    .filter_map(harness_core::parse_harness_unix_ms)
                    .max()
                    .map(|millis| format!("unix-ms:{millis}"))
            };
            latest_reset(
                &mut bucket_windows
                    .iter()
                    .filter(|window| window.used_percent.unwrap_or(0) >= LIMITED_USED_PERCENT),
            )
            // A reached flag can arrive with low percentages. The bucket that
            // produced the verdict still owns the reset, so fall back to its
            // own windows rather than reporting no reset at all.
            .or_else(|| latest_reset(&mut bucket_windows.iter()))
        }
        _ => None,
    };
    let confidence = if state == ProviderCapacityState::Unknown {
        ProviderCapacityConfidence::Unknown
    } else {
        ProviderCapacityConfidence::Observed
    };
    let evidence_source = ProviderCapacityEvidence::ProviderQuotaApi;
    let detail = Some(detail);
    ProviderCapacitySnapshot {
        provider: "codex".to_string(),
        execution_mode: execution_mode.to_string(),
        account,
        state,
        observed_at: observed_at.to_string(),
        observed_unix_ms,
        reset_at,
        evidence_source,
        confidence,
        windows,
        diagnosis: None,
        runtime_context: Vec::new(),
        detail,
    }
}

impl Drop for CodexAppServerClient {
    fn drop(&mut self) {
        kill_worker_tree(&mut self.child);
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + READER_SHUTDOWN_TIMEOUT;
            while !reader.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_thread_uses_temporary_full_access_policy() {
        let params = thread_open_params(
            Path::new("/tmp/project"),
            Some("gpt-test"),
            Some("max"),
            Some("priority"),
            None,
            DEVELOPMENT_SANDBOX,
            DEVELOPMENT_APPROVAL_POLICY,
        );

        assert_eq!(params["sandbox"], "danger-full-access");
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["ephemeral"], false);
        assert_eq!(params["config"]["model_reasoning_effort"], "max");
        assert_eq!(params["serviceTier"], "priority");
        assert!(params.get("threadId").is_none());
    }

    #[test]
    fn resumed_thread_keeps_temporary_full_access_policy() {
        let params = thread_open_params(
            Path::new("/tmp/project"),
            Some("gpt-test"),
            Some("high"),
            Some("default"),
            Some("thread-123"),
            DEVELOPMENT_SANDBOX,
            DEVELOPMENT_APPROVAL_POLICY,
        );

        assert_eq!(params["sandbox"], "danger-full-access");
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["threadId"], "thread-123");
        assert!(params.get("ephemeral").is_none());
    }

    #[test]
    fn native_thread_name_uses_the_member_identity() {
        assert_eq!(
            thread_name_params("thread-123", "RuntimeFixer"),
            serde_json::json!({
                "threadId": "thread-123",
                "name": "Agent Team · RuntimeFixer"
            })
        );
    }

    #[test]
    fn effective_model_prefers_current_top_level_app_server_shape() {
        let response = serde_json::json!({
            "result": {
                "model": "gpt-current",
                "thread": {"id": "thread-123", "model": "gpt-legacy"}
            }
        });

        assert_eq!(
            effective_thread_model(&response).as_deref(),
            Some("gpt-current")
        );
    }

    #[test]
    fn effective_model_accepts_legacy_nested_but_never_invents_a_requested_receipt() {
        let legacy = serde_json::json!({
            "result": {"thread": {"id": "thread-123", "model": "gpt-legacy"}}
        });
        let omitted = serde_json::json!({
            "result": {"thread": {"id": "thread-123"}}
        });

        assert_eq!(
            effective_thread_model(&legacy).as_deref(),
            Some("gpt-legacy")
        );
        assert_eq!(effective_thread_model(&omitted), None);
    }

    #[test]
    fn requested_reasoning_and_service_controls_require_native_confirmation() {
        let response = serde_json::json!({
            "result": {
                "reasoningEffort": "max",
                "serviceTier": "priority"
            }
        });
        assert_eq!(
            effective_thread_reasoning_effort(&response).as_deref(),
            Some("max")
        );
        assert_eq!(
            effective_thread_service_tier(&response).as_deref(),
            Some("priority")
        );
        require_requested_setting("reasoning effort", Some("max"), Some("max"))
            .expect("matching native receipt");
        assert!(
            require_requested_setting("service tier", Some("priority"), Some("default")).is_err()
        );
    }

    /// Verbatim shape of a live `codex-cli 0.145.0` reply, captured by driving
    /// `initialize` + `initialized` + the two account reads over stdio without
    /// ever sending `thread/start`.
    fn live_capacity_read(used_percent: i64, reached: Option<&str>) -> CodexAccountCapacityRead {
        CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": "operator@example.com", "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: serde_json::json!({
                "rateLimits": {
                    "limitId": "codex",
                    "limitName": null,
                    "primary": {
                        "usedPercent": used_percent,
                        "windowDurationMins": 10080,
                        "resetsAt": 1_786_161_121i64
                    },
                    "secondary": null,
                    "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                    "individualLimit": null,
                    "spendControlReached": false,
                    "planType": "pro",
                    "rateLimitReachedType": reached
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "primary": {
                            "usedPercent": used_percent,
                            "windowDurationMins": 10080,
                            "resetsAt": 1_786_161_121i64
                        },
                        "secondary": null,
                        "rateLimitReachedType": reached
                    },
                    "codex_bengalfox": {
                        "limitId": "codex_bengalfox",
                        "limitName": "GPT-5.3-Codex-Spark",
                        "primary": {
                            "usedPercent": 0,
                            "windowDurationMins": 10080,
                            "resetsAt": 1_786_177_280i64
                        },
                        "secondary": null
                    }
                },
                "rateLimitResetCredits": {"availableCount": 0, "credits": []}
            }),
        }
    }

    #[test]
    fn account_read_maps_every_reviewed_credential_source() {
        assert_eq!(
            account_ref_from_account_read(&serde_json::json!({
                "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
                "requiresOpenaiAuth": true
            })),
            ProviderAccountRef {
                source: "chatgpt".into(),
                identifier: Some("a@b.c".into()),
                plan: Some("pro".into())
            }
        );
        assert_eq!(
            account_ref_from_account_read(&serde_json::json!({"account": {"type": "apiKey"}}))
                .source,
            "api_key"
        );
        assert_eq!(
            account_ref_from_account_read(
                &serde_json::json!({"account": null, "requiresOpenaiAuth": true})
            )
            .source,
            "signed_out"
        );
    }

    #[test]
    fn live_codex_payload_reports_available_with_observed_windows() {
        let snapshot = codex_capacity_snapshot(
            "codex_app_server",
            &live_capacity_read(3, None),
            "unix-ms:1000",
            1_000,
        );

        assert_eq!(snapshot.state, ProviderCapacityState::Available);
        assert_eq!(
            snapshot.evidence_source,
            ProviderCapacityEvidence::ProviderQuotaApi
        );
        assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Observed);
        assert_eq!(snapshot.account.source, "chatgpt");
        assert_eq!(snapshot.account.plan.as_deref(), Some("pro"));
        // Both metered buckets are reported, keyed by their provider limit id.
        let labels: Vec<&str> = snapshot
            .windows
            .iter()
            .map(|window| window.label.as_str())
            .collect();
        assert_eq!(labels, vec!["codex.primary", "codex_bengalfox.primary"]);
        assert_eq!(snapshot.windows[0].used_percent, Some(3));
        assert_eq!(
            snapshot.windows[0].resets_at.as_deref(),
            Some("unix-ms:1786161121000")
        );
        // An available account has no reset to report.
        assert_eq!(snapshot.reset_at, None);
    }

    #[test]
    fn provider_reported_limits_drive_limited_and_exhausted() {
        let limited = codex_capacity_snapshot(
            "codex_app_server",
            &live_capacity_read(93, None),
            "unix-ms:1000",
            1_000,
        );
        assert_eq!(limited.state, ProviderCapacityState::Limited);
        assert_eq!(
            limited.reset_at.as_deref(),
            Some("unix-ms:1786161121000"),
            "a constrained window reports when it reopens"
        );

        let saturated = codex_capacity_snapshot(
            "codex_app_server",
            &live_capacity_read(100, None),
            "unix-ms:1000",
            1_000,
        );
        assert_eq!(saturated.state, ProviderCapacityState::Exhausted);

        let reached = codex_capacity_snapshot(
            "codex_app_server",
            &live_capacity_read(4, Some("rate_limit_reached")),
            "unix-ms:1000",
            1_000,
        );
        assert_eq!(
            reached.state,
            ProviderCapacityState::Exhausted,
            "a provider-reported reached type outranks a low percentage"
        );
        assert!(reached
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("rate_limit_reached"));
    }

    #[test]
    fn spend_control_reached_is_exhausted_even_with_headroom() {
        let mut read = live_capacity_read(1, None);
        read.rate_limits["rateLimits"]["spendControlReached"] = serde_json::json!(true);

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
        assert!(snapshot
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("spendControlReached"));
    }

    #[test]
    fn missing_or_unusable_payloads_stay_unknown_and_never_invent_a_number() {
        let empty = CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": null, "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: serde_json::Value::Null,
        };
        let snapshot = codex_capacity_snapshot("codex_app_server", &empty, "unix-ms:1", 1);
        assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
        assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Unknown);
        assert!(snapshot.windows.is_empty());

        // A payload whose windows omit `usedPercent` yields no window at all
        // rather than a fabricated zero.
        let unusable = CodexAccountCapacityRead {
            account: empty.account.clone(),
            rate_limits: serde_json::json!({"rateLimits": {"primary": {"resetsAt": 1}}}),
        };
        let snapshot = codex_capacity_snapshot("codex_app_server", &unusable, "unix-ms:1", 1);
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
    }

    /// The VERBATIM payload shape captured from `codex-cli 0.145.0` by driving
    /// `initialize` + `initialized` + the two account reads over stdio. Kept
    /// whole — extra keys included — so a regression is exercised against the
    /// real wire shape, not a convenience subset.
    fn live_multi_bucket_payload() -> serde_json::Value {
        serde_json::json!({
            "rateLimits": {
                "limitId": "codex",
                "limitName": null,
                "primary": {"usedPercent": 3, "windowDurationMins": 10080, "resetsAt": 1786161121i64},
                "secondary": null,
                "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                "individualLimit": null,
                "spendControlReached": false,
                "planType": "pro",
                "rateLimitReachedType": null
            },
            "rateLimitsByLimitId": {
                "codex_bengalfox": {
                    "limitId": "codex_bengalfox",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": {"usedPercent": 0, "windowDurationMins": 10080, "resetsAt": 1786177280i64},
                    "secondary": null,
                    "credits": null,
                    "individualLimit": null,
                    "spendControlReached": null,
                    "planType": "pro",
                    "rateLimitReachedType": null
                },
                "codex": {
                    "limitId": "codex",
                    "limitName": null,
                    "primary": {"usedPercent": 3, "windowDurationMins": 10080, "resetsAt": 1786161121i64},
                    "secondary": null,
                    "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
                    "individualLimit": null,
                    "spendControlReached": false,
                    "planType": "pro",
                    "rateLimitReachedType": null
                }
            },
            "rateLimitResetCredits": {"availableCount": 0, "credits": []}
        })
    }

    #[test]
    fn the_real_live_multi_bucket_payload_classifies_from_the_account_bucket() {
        let read = CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": "operator@example.com", "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: live_multi_bucket_payload(),
        };

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(snapshot.state, ProviderCapacityState::Available);
        assert_eq!(snapshot.account.plan.as_deref(), Some("pro"));
        // Both real buckets are reported, sorted and keyed by provider limit id.
        let labels: Vec<&str> = snapshot
            .windows
            .iter()
            .map(|window| window.label.as_str())
            .collect();
        assert_eq!(labels, vec!["codex.primary", "codex_bengalfox.primary"]);
        assert_eq!(snapshot.reset_at, None, "an available account has no reset");

        // Saturate ONLY the per-model bucket in that same real payload.
        let mut spark_spent = read.rate_limits.clone();
        spark_spent["rateLimitsByLimitId"]["codex_bengalfox"]["primary"]["usedPercent"] =
            serde_json::json!(100);
        spark_spent["rateLimitsByLimitId"]["codex_bengalfox"]["rateLimitReachedType"] =
            serde_json::json!("rate_limit_reached");
        let snapshot = codex_capacity_snapshot(
            "codex_app_server",
            &CodexAccountCapacityRead {
                account: read.account.clone(),
                rate_limits: spark_spent,
            },
            "unix-ms:1000",
            1_000,
        );
        assert_eq!(
            snapshot.state,
            ProviderCapacityState::Available,
            "a spent per-model bucket must not refuse an account at 3%: {:?}",
            snapshot.detail
        );

        // Saturate the ACCOUNT bucket in the same real payload.
        let mut account_spent = read.rate_limits.clone();
        account_spent["rateLimits"]["rateLimitReachedType"] =
            serde_json::json!("rate_limit_reached");
        let snapshot = codex_capacity_snapshot(
            "codex_app_server",
            &CodexAccountCapacityRead {
                account: read.account.clone(),
                rate_limits: account_spent,
            },
            "unix-ms:1000",
            1_000,
        );
        assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
        assert_eq!(
            snapshot.reset_at.as_deref(),
            Some("unix-ms:1786161121000"),
            "the reset comes from the account bucket's own window"
        );
    }

    #[test]
    fn a_saturated_per_model_bucket_is_not_an_account_verdict() {
        // Live payloads carry several metered buckets (`codex`,
        // `codex_bengalfox`). Reading the peak across all of them would refuse
        // every codex member because one per-model bucket is spent, while the
        // account bucket the member would actually draw on is at 3%.
        let mut read = live_capacity_read(3, None);
        read.rate_limits["rateLimitsByLimitId"]["codex_bengalfox"]["primary"]["usedPercent"] =
            serde_json::json!(100);

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(
            snapshot.state,
            ProviderCapacityState::Available,
            "the account bucket has headroom: {:?}",
            snapshot.detail
        );
        // The hot bucket is still visible so an operator can see it.
        assert!(snapshot
            .windows
            .iter()
            .any(
                |window| window.limit_id.as_deref() == Some("codex_bengalfox")
                    && window.used_percent == Some(100)
            ));
    }

    #[test]
    fn a_reached_flag_is_read_from_the_same_bucket_as_the_percentage() {
        // The account mirror and the keyed view must never be mixed: a reached
        // flag in one and a low percentage in the other previously produced
        // `available` with `observed` confidence.
        let read = CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: serde_json::json!({
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "primary": {"usedPercent": 40, "resetsAt": 1_786_161_121i64},
                        "rateLimitReachedType": "rate_limit_reached"
                    }
                }
            }),
        };

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
        assert!(snapshot
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("rate_limit_reached"));
    }

    #[test]
    fn several_buckets_without_an_account_mirror_are_not_attributable() {
        let read = CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: serde_json::json!({
                "rateLimitsByLimitId": {
                    "codex": {"limitId": "codex", "primary": {"usedPercent": 4}},
                    "codex_bengalfox": {"limitId": "codex_bengalfox", "primary": {"usedPercent": 99}}
                }
            }),
        };

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        // Neither `available` (which would ignore the hot bucket) nor
        // `exhausted` (which would refuse a healthy account) is honest here.
        assert_eq!(snapshot.state, ProviderCapacityState::Unknown);
        assert_eq!(snapshot.confidence, ProviderCapacityConfidence::Unknown);
        assert_eq!(snapshot.windows.len(), 2, "both buckets stay visible");
    }

    #[test]
    fn reset_reports_when_the_last_constraining_window_reopens() {
        let read = CodexAccountCapacityRead {
            account: serde_json::json!({
                "account": {"type": "chatgpt", "email": "a@b.c", "planType": "pro"},
                "requiresOpenaiAuth": true
            }),
            rate_limits: serde_json::json!({
                "rateLimits": {
                    "limitId": "codex",
                    // Saturated for five more days...
                    "primary": {"usedPercent": 100, "resetsAt": 1_786_600_000i64},
                    // ...while a second constrained window reopens in an hour.
                    "secondary": {"usedPercent": 95, "resetsAt": 1_786_100_000i64}
                }
            }),
        };

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(snapshot.state, ProviderCapacityState::Exhausted);
        assert_eq!(
            snapshot.reset_at.as_deref(),
            Some("unix-ms:1786600000000"),
            "the account is usable again only when the LAST constraint reopens"
        );
    }

    #[test]
    fn float_usage_percentages_are_read_rather_than_silently_dropped() {
        let mut read = live_capacity_read(3, None);
        read.rate_limits["rateLimits"]["primary"]["usedPercent"] = serde_json::json!(93.4);

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1000", 1_000);

        assert_eq!(
            snapshot.state,
            ProviderCapacityState::Limited,
            "a float window must not empty the probe: {:?}",
            snapshot.detail
        );
    }

    #[test]
    fn signed_out_account_is_unauthorized_not_exhausted() {
        let read = CodexAccountCapacityRead {
            account: serde_json::json!({"account": null, "requiresOpenaiAuth": true}),
            rate_limits: serde_json::json!({"rateLimits": {"primary": {"usedPercent": 0}}}),
        };

        let snapshot = codex_capacity_snapshot("codex_app_server", &read, "unix-ms:1", 1);

        assert_eq!(snapshot.state, ProviderCapacityState::Unauthorized);
        assert_eq!(
            snapshot.evidence_source,
            ProviderCapacityEvidence::AuthMetadata
        );
        assert_eq!(snapshot.account.source, "signed_out");
    }

    #[test]
    fn reviewed_account_read_methods_never_name_a_thread() {
        // Guards the acceptance requirement literally: the preflight speaks
        // only these two methods, and neither is a thread verb.
        for method in [ACCOUNT_READ_METHOD, ACCOUNT_RATE_LIMITS_READ_METHOD] {
            assert!(method.starts_with("account/"), "{method}");
            assert!(!method.contains("thread"), "{method}");
            assert!(!method.contains("turn"), "{method}");
        }
    }
}
