//! Server-Sent Events (SSE) streaming for real-time harness events
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam::channel::{bounded, Receiver, Sender};
use harness_core::{AgentEvent, Message, ProviderSession, WorkflowRun, WorkflowStep};

/// An event frame sent to SSE clients (WP2: added WorkflowRun and WorkflowStep)
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum SseEventFrame {
    /// Snapshot of all current events (sent on initial connection)
    Snapshot {
        agent_events: Vec<AgentEvent>,
        messages: Vec<Message>,
        provider_sessions: Vec<ProviderSession>,
        generated_at: String,
    },
    /// A new agent event was recorded
    AgentEvent(AgentEvent),
    /// A message was created or delivery status changed
    Message(Message),
    /// A provider session status changed
    ProviderSession(ProviderSession),
    /// A workflow run status changed (WP2)
    WorkflowRun(WorkflowRun),
    /// A workflow step started or completed (WP2)
    WorkflowStep(WorkflowStep),
    /// A single raw provider turn event ({session_id, event}), teed live during
    /// a delivery so the agent TUI streams sub-second instead of polling (Stage B).
    ProviderTurnEvent(serde_json::Value),
    /// Normalized companion to ProviderTurnEvent for live Stage B consumers:
    /// {session_id, events: HarnessTurnEvent[]}.
    ProviderTurnEventNormalized(serde_json::Value),
}

/// Manages SSE client subscriptions and broadcasts, keyed by project id
/// (goal-multi-project P6). Each project has its own list of client senders, so a
/// frame appended to project A's store is only delivered to clients subscribed to A
/// — project B never sees it. A subscriber to an unknown project simply receives no
/// frames (the watcher only broadcasts ids it knows about), which is harmless.
pub struct SseManager {
    // project_id → connected client senders. Drop a sender to unsubscribe a client.
    clients: Arc<Mutex<HashMap<String, Vec<Sender<SseEventFrame>>>>>,
}

impl SseManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Subscribe a new client to a single project's event stream.
    pub fn subscribe(&self, project_id: &str) -> Receiver<SseEventFrame> {
        let (tx, rx) = bounded(100); // Buffered channel
        let mut clients = self.clients.lock().unwrap();
        clients.entry(project_id.to_string()).or_default().push(tx);
        rx
    }

    /// Broadcast an event to the clients subscribed to a single project.
    pub fn broadcast(&self, project_id: &str, frame: SseEventFrame) {
        let mut clients = self.clients.lock().unwrap();
        if let Some(senders) = clients.get_mut(project_id) {
            // Remove clients whose receivers are dropped.
            senders.retain(|tx| tx.try_send(frame.clone()).is_ok());
        }
    }

    /// Return number of currently connected clients for a project (for debugging).
    #[allow(dead_code)]
    pub fn client_count(&self, project_id: &str) -> usize {
        let clients = self.clients.lock().unwrap();
        clients.get(project_id).map(|v| v.len()).unwrap_or(0)
    }
}

impl Clone for SseManager {
    fn clone(&self) -> Self {
        Self {
            clients: Arc::clone(&self.clients),
        }
    }
}

/// A live-turn-event normalizer, scoped to one project's store (so the provider
/// session lookup hits the right ledger). Boxed so each project can carry its own.
pub type Normalizer = Box<dyn Fn(&str, &serde_json::Value) -> Vec<serde_json::Value> + Send>;

/// Start a background watcher thread that monitors each project's jsonl files for
/// appends and broadcasts new records to that project's SSE clients only
/// (goal-multi-project P6). One thread polls every watched project serially; the
/// `consumed_offsets` map is keyed by `(project_id, filename)` so identical
/// filenames across projects are tracked independently and never cross streams.
///
/// `rescan` returns the live project-id → store-root map and is called EVERY poll,
/// not just at startup, so a project created or switched-to after serve starts
/// (`POST /v1/projects/switch` or a CLI `project add`) gets a live event channel
/// without a serve restart (goal-multi-project #147 follow-up). `make_normalizer`
/// builds a project's live-turn-event normalizer lazily on first sight, scoped to
/// that project's store.
///
/// Seeding policy: projects present at startup are seeded at current EOF so only
/// rows appended after the watcher starts are streamed (the initial snapshot covers
/// pre-existing rows). A project that appears LATER is intentionally NOT EOF-seeded;
/// its offsets default to 0 so its freshly-created ledger streams from the first
/// byte, which makes a row appended right after registration deliverable with no
/// seed-vs-append race (a post-startup project is newly created, so its history is
/// empty/small and the full replay is cheap and deduped by id on the client).
pub fn start_sse_watcher(
    rescan: impl Fn() -> HashMap<String, PathBuf> + Send + 'static,
    make_normalizer: impl Fn(&Path) -> Normalizer + Send + 'static,
    manager: SseManager,
) -> std::io::Result<()> {
    thread::spawn(move || {
        // Track, per (project_id, file), the byte offset through the last *complete*
        // (newline-terminated) line we have already broadcast. A torn trailing
        // fragment (a row still mid-write by the store) leaves the offset short of
        // EOF so it is re-read and emitted exactly once on a later poll, rather than
        // being parsed-as-garbage-and-dropped. Keying by project id keeps two
        // projects with the same filename (e.g. both have `messages.jsonl`)
        // completely independent.
        let mut consumed_offsets: HashMap<(String, String), u64> = HashMap::new();
        // project_id → its live-turn-event normalizer, built lazily so a project
        // registered AFTER serve starts gets one on first sight. Membership also
        // marks which projects we have already seen (EOF-seeded vs. stream-from-0).
        let mut normalizers: HashMap<String, Normalizer> = HashMap::new();

        // Seed offsets at current EOF for the projects known at startup so we only
        // stream rows appended after the watcher starts.
        for (project_id, store_root) in rescan() {
            seed_offsets_at_eof(&project_id, &store_root, &mut consumed_offsets);
            normalizers.insert(project_id, make_normalizer(store_root.as_path()));
        }

        // Poll for new appends at a low floor (~150ms) so the operator sees
        // near-real-time updates. Each poll only opens files that grew, reads the
        // new byte range, and sleeps otherwise — CPU stays negligible.
        loop {
            thread::sleep(POLL_INTERVAL);
            // Re-scan the registry live so newly-registered projects join the watch
            // set mid-run. `store_for` already resolves new projects live for
            // `/v1/snapshot`; this closes the matching gap for `/v1/events`.
            for (project_id, store_root) in rescan() {
                // First sight of a post-startup project: build its normalizer now.
                // We do NOT EOF-seed it, so its offsets stay 0 and this first poll
                // streams its (freshly-created, hence small) ledger live.
                if !normalizers.contains_key(&project_id) {
                    normalizers.insert(project_id.clone(), make_normalizer(store_root.as_path()));
                }
                let normalize = normalizers.get(&project_id);
                poll_project(
                    &project_id,
                    &store_root,
                    &mut consumed_offsets,
                    normalize,
                    &manager,
                );
            }
        }
    });

    Ok(())
}

/// Seed each watched file's consumed offset at its current EOF so the watcher only
/// streams rows appended after this point. Files that do not yet exist are skipped
/// (their offset defaults to 0, so they stream from the first byte once created).
fn seed_offsets_at_eof(
    project_id: &str,
    store_root: &Path,
    consumed_offsets: &mut HashMap<(String, String), u64>,
) {
    for filename in WATCHED_FILES {
        let path = store_root.join(filename);
        if let Ok(metadata) = fs::metadata(&path) {
            consumed_offsets.insert(
                (project_id.to_string(), filename.to_string()),
                metadata.len(),
            );
        }
    }
}

/// The JSONL files the watcher tails in every project store.
const WATCHED_FILES: &[&str] = &[
    "agent_events.jsonl",
    "messages.jsonl",
    "provider_sessions.jsonl",
    "workflow_runs.jsonl",
    "workflow_steps.jsonl",
    "provider_turn_events.jsonl",
];

/// Poll one project's ledgers once and broadcast any new rows to that project's
/// channel only.
fn poll_project(
    project_id: &str,
    store_root: &Path,
    consumed_offsets: &mut HashMap<(String, String), u64>,
    normalize: Option<&Normalizer>,
    manager: &SseManager,
) {
    check_and_broadcast_appends(
        project_id,
        store_root,
        "agent_events.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(event) = serde_json::from_str::<AgentEvent>(line) {
                vec![SseEventFrame::AgentEvent(event)]
            } else {
                Vec::new()
            }
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "messages.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(msg) = serde_json::from_str::<Message>(line) {
                vec![SseEventFrame::Message(msg)]
            } else {
                Vec::new()
            }
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "provider_sessions.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(session) = serde_json::from_str::<ProviderSession>(line) {
                vec![SseEventFrame::ProviderSession(session)]
            } else {
                Vec::new()
            }
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "workflow_runs.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(run) = serde_json::from_str::<WorkflowRun>(line) {
                vec![SseEventFrame::WorkflowRun(run)]
            } else {
                Vec::new()
            }
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "workflow_steps.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(step) = serde_json::from_str::<WorkflowStep>(line) {
                vec![SseEventFrame::WorkflowStep(step)]
            } else {
                Vec::new()
            }
        },
        manager,
    );

    // provider_turn_events.jsonl (Stage B): each line is a raw {session_id, event}
    // teed during a provider delivery; broadcast it so the agent TUI streams live
    // without polling. Normalized companion frames use this project's normalizer.
    check_and_broadcast_appends(
        project_id,
        store_root,
        "provider_turn_events.jsonl",
        consumed_offsets,
        |line| {
            let Ok(envelope) = serde_json::from_str::<serde_json::Value>(line) else {
                return Vec::new();
            };

            let mut frames = vec![SseEventFrame::ProviderTurnEvent(envelope.clone())];
            if let (Some(normalize), Some(session_id)) = (
                normalize,
                envelope
                    .get("session_id")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty()),
            ) {
                let raw = &envelope["event"];
                let normalized = normalize(session_id, raw);
                if !normalized.is_empty() {
                    frames.push(SseEventFrame::ProviderTurnEventNormalized(
                        serde_json::json!({
                            "session_id": session_id,
                            "events": normalized,
                        }),
                    ));
                }
            }
            frames
        },
        manager,
    );
}

/// SSE watcher poll interval. Lowered from the original 500ms floor so the
/// operator (the first real consumer of live SSE) sees near-real-time updates.
/// 150ms keeps perceived latency low while the grew-only read path keeps idle
/// CPU negligible.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

fn check_and_broadcast_appends<F>(
    project_id: &str,
    store_root: &Path,
    filename: &str,
    consumed_offsets: &mut HashMap<(String, String), u64>,
    parse_line: F,
    manager: &SseManager,
) where
    F: Fn(&str) -> Vec<SseEventFrame>,
{
    let path = store_root.join(filename);
    let Ok(metadata) = fs::metadata(&path) else {
        return;
    };

    let current_size = metadata.len();
    let key = (project_id.to_string(), filename.to_string());
    let consumed = consumed_offsets.get(&key).copied().unwrap_or(0);

    if current_size <= consumed {
        return;
    }

    // Read the new byte range [consumed, current_size). We deliberately work in
    // bytes (not read_line) so we can distinguish a complete, newline-terminated
    // line from a torn trailing fragment that the store is still mid-append on.
    let Ok(mut file_handle) = fs::File::open(&path) else {
        return;
    };
    if file_handle.seek(SeekFrom::Start(consumed)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if file_handle.read_to_end(&mut buf).is_err() {
        return;
    }

    // Only consume through the last newline. Any bytes after it are a torn
    // partial line: leave the offset short of them so the now-complete line is
    // re-read and broadcast exactly once on a later poll, never dropped.
    let Some(last_newline) = buf.iter().rposition(|&b| b == b'\n') else {
        // No complete line yet — the whole new range is a torn fragment. Do not
        // advance the offset; retry next poll.
        return;
    };

    let complete = &buf[..=last_newline];
    for line in complete.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        // Lossy is safe: JSONL rows are UTF-8; a partial multi-byte char can
        // only occur in the trailing fragment we already excluded above.
        let text = String::from_utf8_lossy(line);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        for frame in parse_line(trimmed) {
            manager.broadcast(project_id, frame);
        }
    }

    // Advance only past the complete lines we just consumed.
    consumed_offsets.insert(key, consumed + (last_newline as u64) + 1);
}

/// Write an SSE response header
pub fn write_sse_header(stream: &mut TcpStream) -> std::io::Result<()> {
    let response = "HTTP/1.1 200 OK\r\n\
                    Content-Type: text/event-stream\r\n\
                    Cache-Control: no-cache\r\n\
                    Connection: keep-alive\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    \r\n";
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Write a single SSE frame to the client
pub fn write_sse_frame(
    stream: &mut TcpStream,
    event_kind: &str,
    data: &serde_json::Value,
) -> std::io::Result<()> {
    let frame = format!("event: {}\ndata: {}\n\n", event_kind, data);
    stream.write_all(frame.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Write a keepalive comment to keep the connection alive
pub fn write_sse_keepalive(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(b": keepalive\n\n")?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    use harness_core::{
        Message, MessageDeliveryStatus, MessageKind, SenderKind, WorkflowRunStatus,
        WorkflowStepStatus,
    };

    use super::*;

    /// A fixed project id used by the single-project unit tests below; the
    /// multi-project leakage coverage lives in tests/serve_sse_projects.rs.
    const TEST_PID: &str = "_test";

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "harness-sse-test-{tag}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    fn test_message(id: &str) -> Message {
        Message {
            id: id.into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Do the task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        }
    }

    fn test_workflow_run(id: &str) -> WorkflowRun {
        WorkflowRun {
            id: id.into(),
            workflow_name: "test".into(),
            status: WorkflowRunStatus::Running,
            step_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: None,
            design_intent: None,
            spec: None,
            trace_retention: "durable".into(),
            host_pid: None,
            dry_run: false,
            goal_id: None,
            phase_id: None,
            terminal_reason: None,
            partial_output_available: false,
        }
    }

    fn test_workflow_step(id: &str, run_id: &str) -> WorkflowStep {
        WorkflowStep {
            id: id.into(),
            run_id: run_id.into(),
            phase: "test".into(),
            label: "test-step".into(),
            provider_session_id: None,
            status: WorkflowStepStatus::Running,
            output_summary: None,
            result: None,
            started_at: "unix-ms:1".into(),
            ended_at: None,
            task_id: None,
            verdict_outcome: None,
            terminal_reason: None,
            partial: false,
        }
    }

    fn message_frame(line: &str) -> Vec<SseEventFrame> {
        serde_json::from_str::<Message>(line)
            .ok()
            .map(SseEventFrame::Message)
            .into_iter()
            .collect()
    }

    fn workflow_run_frame(line: &str) -> Vec<SseEventFrame> {
        serde_json::from_str::<WorkflowRun>(line)
            .ok()
            .map(SseEventFrame::WorkflowRun)
            .into_iter()
            .collect()
    }

    fn workflow_step_frame(line: &str) -> Vec<SseEventFrame> {
        serde_json::from_str::<WorkflowStep>(line)
            .ok()
            .map(SseEventFrame::WorkflowStep)
            .into_iter()
            .collect()
    }

    /// A JSONL row whose write is observed in two pieces (the watcher polls
    /// after only the first half has hit the file) must be delivered exactly
    /// once — never dropped as a torn line, never duplicated when it completes.
    #[test]
    fn torn_record_split_across_polls_delivered_exactly_once() {
        let root = unique_dir("torn");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("messages.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        // Two full rows as the store would write them: compact JSON + '\n'.
        let row_a = serde_json::to_string(&test_message("message-a")).expect("ser a");
        let row_b = serde_json::to_string(&test_message("message-b")).expect("ser b");
        let full = format!("{row_a}\n{row_b}\n");
        let bytes = full.as_bytes();

        // Split point lands mid-way through row_b (after row_a's newline), so
        // the first poll sees a complete row_a plus a torn fragment of row_b.
        let split = row_a.len() + 1 + (row_b.len() / 2);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open");
        file.write_all(&bytes[..split]).expect("write first half");
        file.flush().expect("flush first half");

        // Poll 1: row_a delivered, row_b fragment buffered (offset not advanced
        // past it).
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        // Poll 1.5: nothing new on disk, the torn fragment must NOT be emitted.
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        // Complete row_b.
        file.write_all(&bytes[split..]).expect("write second half");
        file.flush().expect("flush second half");

        // Poll 2: row_b now complete and delivered exactly once.
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        // Poll 3: idempotent — no re-delivery.
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        let mut received = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::Message(m) => received.push(m.id),
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(
            received,
            vec!["message-a".to_string(), "message-b".to_string()],
            "each row delivered exactly once and in order, torn fragment never dropped"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// The complete-line path must broadcast each appended row once and advance
    /// past them so a follow-up poll with no new bytes emits nothing.
    #[test]
    fn complete_rows_broadcast_once_and_offset_advances() {
        let root = unique_dir("complete");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("messages.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        let row = serde_json::to_string(&test_message("message-once")).expect("ser");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open");
        file.write_all(format!("{row}\n").as_bytes())
            .expect("write");
        file.flush().expect("flush");

        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(
            count, 1,
            "complete row broadcast exactly once across two polls"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// The generalized append parser must preserve the old single-frame file
    /// behavior: valid rows emit one frame, malformed rows emit zero frames.
    #[test]
    fn single_frame_rows_still_emit_one_and_parse_failures_emit_zero() {
        let root = unique_dir("single-frame");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("messages.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        let row = serde_json::to_string(&test_message("message-valid")).expect("ser");
        std::fs::write(&path, format!("{row}\nnot-json\n")).expect("write rows");

        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        let mut received = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::Message(message) => received.push(message.id),
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(received, vec!["message-valid".to_string()]);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// A parse callback may now fan out one complete JSONL row into multiple
    /// SSE frames; offset handling remains one-row-at-a-time.
    #[test]
    fn one_line_can_broadcast_multiple_frames() {
        let root = unique_dir("multi-frame");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("provider_turn_events.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        std::fs::write(
            &path,
            serde_json::json!({"session_id": "s-1", "event": {"type": "x"}}).to_string() + "\n",
        )
        .expect("write row");

        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "provider_turn_events.jsonl",
            &mut offsets,
            |_| {
                vec![
                    SseEventFrame::ProviderTurnEvent(serde_json::json!({"raw": true})),
                    SseEventFrame::ProviderTurnEventNormalized(serde_json::json!({
                        "session_id": "s-1",
                        "events": [],
                    })),
                ]
            },
            &manager,
        );

        let mut raw = 0;
        let mut normalized = 0;
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::ProviderTurnEvent(_) => raw += 1,
                SseEventFrame::ProviderTurnEventNormalized(_) => normalized += 1,
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(raw, 1);
        assert_eq!(normalized, 1);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Workflow runs and steps should be streamed via SSE like other events (WP2).
    #[test]
    fn workflow_run_and_step_broadcast_exactly_once() {
        let root = unique_dir("workflow");
        std::fs::create_dir_all(&root).expect("create root");
        let run_path = root.join("workflow_runs.jsonl");
        let step_path = root.join("workflow_steps.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        // Write a workflow run and a step
        let run = test_workflow_run("run-1");
        let step = test_workflow_step("step-1", "run-1");
        let run_row = serde_json::to_string(&run).expect("ser run");
        let step_row = serde_json::to_string(&step).expect("ser step");

        let mut run_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&run_path)
            .expect("open run");
        run_file
            .write_all(format!("{run_row}\n").as_bytes())
            .expect("write run");
        run_file.flush().expect("flush run");

        let mut step_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&step_path)
            .expect("open step");
        step_file
            .write_all(format!("{step_row}\n").as_bytes())
            .expect("write step");
        step_file.flush().expect("flush step");

        // Poll both files
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "workflow_runs.jsonl",
            &mut offsets,
            workflow_run_frame,
            &manager,
        );
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "workflow_steps.jsonl",
            &mut offsets,
            workflow_step_frame,
            &manager,
        );

        let mut run_count = 0;
        let mut step_count = 0;
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::WorkflowRun(r) => {
                    assert_eq!(r.id, "run-1");
                    run_count += 1;
                }
                SseEventFrame::WorkflowStep(s) => {
                    assert_eq!(s.id, "step-1");
                    step_count += 1;
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(run_count, 1, "workflow run broadcast exactly once");
        assert_eq!(step_count, 1, "workflow step broadcast exactly once");

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// A frame broadcast to project A must reach A's subscriber and NOT B's, and
    /// the offset map keys by (project, filename) so two projects with the same
    /// filename are independent (multi-project P6 leakage guard).
    #[test]
    fn broadcast_is_isolated_per_project() {
        let manager = SseManager::new();
        let rx_a = manager.subscribe("proj-a");
        let rx_b = manager.subscribe("proj-b");

        manager.broadcast("proj-a", SseEventFrame::Message(test_message("only-a")));

        // A receives it.
        match rx_a.try_recv() {
            Ok(SseEventFrame::Message(m)) => assert_eq!(m.id, "only-a"),
            other => panic!("project A should receive its own frame, got {other:?}"),
        }
        // B receives nothing.
        assert!(
            rx_b.try_recv().is_err(),
            "project B must not see project A's frame"
        );
        assert_eq!(manager.client_count("proj-a"), 1);
        assert_eq!(manager.client_count("proj-b"), 1);
    }

    /// Identical filenames across two project stores are tracked independently:
    /// appending to A's `messages.jsonl` advances only A's offset and broadcasts
    /// only to A.
    #[test]
    fn offsets_and_broadcasts_independent_across_projects() {
        let root_a = unique_dir("iso-a");
        let root_b = unique_dir("iso-b");
        std::fs::create_dir_all(&root_a).expect("a");
        std::fs::create_dir_all(&root_b).expect("b");

        let manager = SseManager::new();
        let rx_a = manager.subscribe("proj-a");
        let rx_b = manager.subscribe("proj-b");
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        // Write a row only into project A's messages.jsonl.
        let row = serde_json::to_string(&test_message("a-row")).expect("ser");
        std::fs::write(root_a.join("messages.jsonl"), format!("{row}\n")).expect("write a");

        check_and_broadcast_appends(
            "proj-a",
            &root_a,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );
        // Project B has no such file → no-op, no offset entry.
        check_and_broadcast_appends(
            "proj-b",
            &root_b,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        match rx_a.try_recv() {
            Ok(SseEventFrame::Message(m)) => assert_eq!(m.id, "a-row"),
            other => panic!("A should receive its row, got {other:?}"),
        }
        assert!(rx_b.try_recv().is_err(), "B must not see A's row");

        // A's offset advanced; B's is absent (no file to read).
        assert!(offsets.contains_key(&("proj-a".to_string(), "messages.jsonl".to_string())));
        assert!(!offsets.contains_key(&("proj-b".to_string(), "messages.jsonl".to_string())));

        std::fs::remove_dir_all(&root_a).expect("cleanup a");
        std::fs::remove_dir_all(&root_b).expect("cleanup b");
    }
}
