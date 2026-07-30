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

use crate::{kill_worker_tree, CliError, CliResult};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DEVELOPMENT_SANDBOX: &str = "danger-full-access";
const DEVELOPMENT_APPROVAL_POLICY: &str = "never";

fn thread_open_params(
    cwd: &Path,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    resume_thread_id: Option<&str>,
) -> serde_json::Value {
    let mut params = match resume_thread_id {
        Some(thread_id) => serde_json::json!({
            "threadId": thread_id,
            "cwd": cwd,
            "model": model,
            "serviceTier": service_tier,
            "sandbox": DEVELOPMENT_SANDBOX,
            "approvalPolicy": DEVELOPMENT_APPROVAL_POLICY
        }),
        None => serde_json::json!({
            "cwd": cwd,
            "model": model,
            "serviceTier": service_tier,
            "sandbox": DEVELOPMENT_SANDBOX,
            "approvalPolicy": DEVELOPMENT_APPROVAL_POLICY,
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
}

impl CodexAppServerClient {
    pub(crate) fn spawn(cwd: &Path, options: CodexAppServerSpawnOptions<'_>) -> CliResult<Self> {
        let mut command = Command::new("codex");
        command
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .envs(options.collaboration_env.iter().cloned())
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
            collaboration_mode: if options.plan_mode { "plan" } else { "default" },
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
}
