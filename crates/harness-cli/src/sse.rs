//! Server-Sent Events (SSE) streaming for real-time harness events
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam::channel::{bounded, Receiver, Sender};
use harness_core::{AgentEvent, Message, ProviderSession, WorkflowRun, WorkflowStep};
use harness_store::HarnessStore;

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
}

/// Manages SSE client subscriptions and broadcasts
pub struct SseManager {
    // All connected client senders. Drop a sender to unsubscribe a client.
    clients: Arc<Mutex<Vec<Sender<SseEventFrame>>>>,
}

impl SseManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Subscribe a new client to the event stream
    pub fn subscribe(&self) -> Receiver<SseEventFrame> {
        let (tx, rx) = bounded(100); // Buffered channel
        let mut clients = self.clients.lock().unwrap();
        clients.push(tx);
        rx
    }

    /// Broadcast an event to all connected clients
    pub fn broadcast(&self, frame: SseEventFrame) {
        let mut clients = self.clients.lock().unwrap();
        // Remove clients whose receivers are dropped
        clients.retain(|tx| tx.try_send(frame.clone()).is_ok());
    }

    /// Return number of currently connected clients (for debugging)
    #[allow(dead_code)]
    pub fn client_count(&self) -> usize {
        let clients = self.clients.lock().unwrap();
        clients.len()
    }
}

impl Clone for SseManager {
    fn clone(&self) -> Self {
        Self {
            clients: Arc::clone(&self.clients),
        }
    }
}

/// Start a background watcher thread that monitors jsonl files for appends
/// and broadcasts new records to all SSE clients. WP2: added workflow_runs.jsonl
/// and workflow_steps.jsonl.
pub fn start_sse_watcher(store: &HarnessStore, manager: SseManager) -> std::io::Result<()> {
    let store_root = store.root().to_path_buf();

    thread::spawn(move || {
        // Track, per file, the byte offset through the last *complete*
        // (newline-terminated) line we have already broadcast. A torn trailing
        // fragment (a row still mid-write by the store) leaves the offset short
        // of EOF so it is re-read and emitted exactly once on a later poll,
        // rather than being parsed-as-garbage-and-dropped.
        let mut consumed_offsets: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();

        // Seed offsets at current EOF so we only stream rows appended after the
        // watcher starts (the initial snapshot covers pre-existing rows).
        for filename in &[
            "agent_events.jsonl",
            "messages.jsonl",
            "provider_sessions.jsonl",
            "workflow_runs.jsonl",
            "workflow_steps.jsonl",
            "provider_turn_events.jsonl",
        ] {
            let path = store_root.join(filename);
            if let Ok(metadata) = fs::metadata(&path) {
                consumed_offsets.insert(filename.to_string(), metadata.len());
            }
        }

        // Poll for new appends at a low floor (~150ms) so the operator sees
        // near-real-time updates. Each poll only opens files that grew, reads
        // the new byte range, and sleeps otherwise — CPU stays negligible.
        loop {
            thread::sleep(POLL_INTERVAL);

            // Check agent_events.jsonl
            check_and_broadcast_appends(
                &store_root,
                "agent_events.jsonl",
                &mut consumed_offsets,
                |line| {
                    if let Ok(event) = serde_json::from_str::<AgentEvent>(line) {
                        Some(SseEventFrame::AgentEvent(event))
                    } else {
                        None
                    }
                },
                &manager,
            );

            // Check messages.jsonl
            check_and_broadcast_appends(
                &store_root,
                "messages.jsonl",
                &mut consumed_offsets,
                |line| {
                    if let Ok(msg) = serde_json::from_str::<Message>(line) {
                        Some(SseEventFrame::Message(msg))
                    } else {
                        None
                    }
                },
                &manager,
            );

            // Check provider_sessions.jsonl
            check_and_broadcast_appends(
                &store_root,
                "provider_sessions.jsonl",
                &mut consumed_offsets,
                |line| {
                    if let Ok(session) = serde_json::from_str::<ProviderSession>(line) {
                        Some(SseEventFrame::ProviderSession(session))
                    } else {
                        None
                    }
                },
                &manager,
            );

            // Check workflow_runs.jsonl (WP2)
            check_and_broadcast_appends(
                &store_root,
                "workflow_runs.jsonl",
                &mut consumed_offsets,
                |line| {
                    if let Ok(run) = serde_json::from_str::<WorkflowRun>(line) {
                        Some(SseEventFrame::WorkflowRun(run))
                    } else {
                        None
                    }
                },
                &manager,
            );

            // Check workflow_steps.jsonl (WP2)
            check_and_broadcast_appends(
                &store_root,
                "workflow_steps.jsonl",
                &mut consumed_offsets,
                |line| {
                    if let Ok(step) = serde_json::from_str::<WorkflowStep>(line) {
                        Some(SseEventFrame::WorkflowStep(step))
                    } else {
                        None
                    }
                },
                &manager,
            );

            // Check provider_turn_events.jsonl (Stage B): each line is a raw
            // {session_id, event} teed during a claude delivery; broadcast it so
            // the agent TUI streams live without polling.
            check_and_broadcast_appends(
                &store_root,
                "provider_turn_events.jsonl",
                &mut consumed_offsets,
                |line| {
                    serde_json::from_str::<serde_json::Value>(line)
                        .ok()
                        .map(SseEventFrame::ProviderTurnEvent)
                },
                &manager,
            );
        }
    });

    Ok(())
}

/// SSE watcher poll interval. Lowered from the original 500ms floor so the
/// operator (the first real consumer of live SSE) sees near-real-time updates.
/// 150ms keeps perceived latency low while the grew-only read path keeps idle
/// CPU negligible.
const POLL_INTERVAL: Duration = Duration::from_millis(150);

fn check_and_broadcast_appends<F>(
    store_root: &Path,
    filename: &str,
    consumed_offsets: &mut std::collections::HashMap<String, u64>,
    parse_line: F,
    manager: &SseManager,
) where
    F: Fn(&str) -> Option<SseEventFrame>,
{
    let path = store_root.join(filename);
    let Ok(metadata) = fs::metadata(&path) else {
        return;
    };

    let current_size = metadata.len();
    let consumed = consumed_offsets.get(filename).copied().unwrap_or(0);

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
        if let Some(frame) = parse_line(trimmed) {
            manager.broadcast(frame);
        }
    }

    // Advance only past the complete lines we just consumed.
    consumed_offsets.insert(filename.to_string(), consumed + (last_newline as u64) + 1);
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
            spec: None,
            trace_retention: "durable".into(),
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
        }
    }

    fn message_frame(line: &str) -> Option<SseEventFrame> {
        serde_json::from_str::<Message>(line)
            .ok()
            .map(SseEventFrame::Message)
    }

    fn workflow_run_frame(line: &str) -> Option<SseEventFrame> {
        serde_json::from_str::<WorkflowRun>(line)
            .ok()
            .map(SseEventFrame::WorkflowRun)
    }

    fn workflow_step_frame(line: &str) -> Option<SseEventFrame> {
        serde_json::from_str::<WorkflowStep>(line)
            .ok()
            .map(SseEventFrame::WorkflowStep)
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
        let rx = manager.subscribe();
        let mut offsets: HashMap<String, u64> = HashMap::new();

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
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        // Poll 1.5: nothing new on disk, the torn fragment must NOT be emitted.
        check_and_broadcast_appends(
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
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );

        // Poll 3: idempotent — no re-delivery.
        check_and_broadcast_appends(
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
        let rx = manager.subscribe();
        let mut offsets: HashMap<String, u64> = HashMap::new();

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
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );
        check_and_broadcast_appends(
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

    /// Workflow runs and steps should be streamed via SSE like other events (WP2).
    #[test]
    fn workflow_run_and_step_broadcast_exactly_once() {
        let root = unique_dir("workflow");
        std::fs::create_dir_all(&root).expect("create root");
        let run_path = root.join("workflow_runs.jsonl");
        let step_path = root.join("workflow_steps.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe();
        let mut offsets: HashMap<String, u64> = HashMap::new();

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
            &root,
            "workflow_runs.jsonl",
            &mut offsets,
            workflow_run_frame,
            &manager,
        );
        check_and_broadcast_appends(
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
}
