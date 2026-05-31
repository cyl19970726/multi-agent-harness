//! Server-Sent Events (SSE) streaming for real-time harness events
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam::channel::{bounded, Receiver, Sender};
use harness_core::{AgentEvent, Message, ProviderSession};
use harness_store::HarnessStore;


/// An event frame sent to SSE clients
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
/// and broadcasts new records to all SSE clients
pub fn start_sse_watcher(store: &HarnessStore, manager: SseManager) -> std::io::Result<()> {
    let store_root = store.root().to_path_buf();
    
    thread::spawn(move || {
        // Track file sizes to detect new appends
        let mut file_sizes: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        
        // Initialize file sizes
        for filename in &["agent_events.jsonl", "messages.jsonl", "provider_sessions.jsonl"] {
            let path = store_root.join(filename);
            if let Ok(metadata) = fs::metadata(&path) {
                file_sizes.insert(filename.to_string(), metadata.len());
            }
        }

        // Poll for new appends every ~500ms (simple, reliable approach)
        loop {
            thread::sleep(Duration::from_millis(500));

            // Check agent_events.jsonl
            check_and_broadcast_appends(
                &store_root,
                "agent_events.jsonl",
                &mut file_sizes,
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
                &mut file_sizes,
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
                &mut file_sizes,
                |line| {
                    if let Ok(session) = serde_json::from_str::<ProviderSession>(line) {
                        Some(SseEventFrame::ProviderSession(session))
                    } else {
                        None
                    }
                },
                &manager,
            );
        }
    });

    Ok(())
}

fn check_and_broadcast_appends<F>(
    store_root: &Path,
    filename: &str,
    file_sizes: &mut std::collections::HashMap<String, u64>,
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
    let old_size = file_sizes.get(filename).copied().unwrap_or(0);

    if current_size > old_size {
        // File grew; read new lines
        if let Ok(mut file_handle) = fs::File::open(&path) {
            let _ = file_handle.seek(SeekFrom::Start(old_size));
            let mut reader = BufReader::new(file_handle);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(frame) = parse_line(trimmed) {
                        manager.broadcast(frame);
                    }
                }
                line.clear();
            }
        }
        file_sizes.insert(filename.to_string(), current_size);
    }
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
pub fn write_sse_frame(stream: &mut TcpStream, event_kind: &str, data: &serde_json::Value) -> std::io::Result<()> {
    let frame = format!(
        "event: {}\ndata: {}\n\n",
        event_kind,
        data.to_string()
    );
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
