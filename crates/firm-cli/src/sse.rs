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
use harness_core::{
    AgentTeamRun, MemberAction, Mission, PendingInteraction, ProviderRuntimeProjection,
    RegistryMessage, TeamMemberCloseRequest, TeamRunEvent, TeamSupervisorLease, Wave, WorkflowRun,
    WorkflowStep,
};

/// An event frame sent to SSE clients. Durable frames are reconstructed by tailing
/// project-scoped JSONL ledgers; `LiveProviderActivity` is deliberately different: it
/// is a direct-only, volatile display signal and is never replayed from a ledger.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum SseEventFrame {
    /// Snapshot of all current events (sent on initial connection)
    Snapshot {
        messages: Vec<RegistryMessage>,
        generated_at: String,
    },
    /// A message was created or delivery status changed
    RegistryMessage(RegistryMessage),
    /// A workflow run status changed (WP2)
    WorkflowRun(WorkflowRun),
    /// A workflow step started or completed (WP2)
    WorkflowStep(WorkflowStep),
    /// A folded team-run event was recorded (Agent Team v0).
    TeamRunEvent(TeamRunEvent),
    /// A native Mission was created or updated.
    Mission(Mission),
    /// A native Wave was created, updated, or gated.
    Wave(Wave),
    /// An Agent Team attempt was created or updated.
    AgentTeamRun(AgentTeamRun),
    /// An Agent Team member's durable run state changed.
    ProviderRuntimeProjection(Box<ProviderRuntimeProjection>),
    /// Durable ownership of one TeamRun's provider-native controls.
    TeamSupervisorLease(TeamSupervisorLease),
    /// Durable Host Close latch for one ProviderRuntimeProjection.
    TeamMemberCloseRequest(TeamMemberCloseRequest),
    /// A durable member action was appended or updated. These rows are the
    /// operator-visible execution trace for an Agent Team attempt, so they are
    /// tail-replayed and merged latest-wins like the other run records.
    MemberAction(MemberAction),
    /// A provider request awaiting or carrying an operator/policy response.
    PendingInteraction(PendingInteraction),
    /// A durable source used by the Dashboard projection changed outside the
    /// serve process. The frame deliberately carries no business row: clients
    /// must refresh the scoped authoritative snapshot instead of treating this
    /// notification as a second truth store.
    ProjectionInvalidated(ProjectionInvalidation),
    /// Sanitized, transient member activity for live display only. This value is
    /// never written to JSONL, included in snapshots, or replayed to a later
    /// subscriber. Callers must not place provider thinking or other durable
    /// claims here.
    LiveProviderActivity(serde_json::Value),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectionInvalidation {
    pub scope: String,
    pub scope_id: String,
    pub ledger: String,
    /// Monotonic only within `stream_epoch` and one
    /// `(scope, scope_id, ledger)` key. This is not a durable cursor.
    pub revision: u64,
    pub reason: String,
    pub stream_epoch: String,
}

#[derive(Clone)]
struct SseClient {
    company_scope_id: Option<String>,
    /// Exact authenticated AgentIdentity eligible to receive its own private
    /// process-memory provider overlay. Team Host authority is intentionally
    /// not represented here and cannot widen this identity.
    private_agent_identity_id: Option<String>,
    sender: Sender<SseEventFrame>,
}

type InvalidationKey = (String, String, String);
type InvalidationRevisionMap = Arc<Mutex<HashMap<InvalidationKey, u64>>>;

/// Manages SSE client subscriptions and broadcasts, keyed by project id
/// (goal-multi-project P6). Each project has its own list of client senders, so a
/// frame appended to project A's store is only delivered to clients subscribed to A
/// — project B never sees it. A subscriber to an unknown project simply receives no
/// frames (the watcher only broadcasts ids it knows about), which is harmless.
pub struct SseManager {
    // Execution-Space id → connected clients. A Company Store selection is an
    // independent filter on each subscriber; Company truth never becomes an
    // Execution-Space ledger merely because both are composed in one snapshot.
    clients: Arc<Mutex<HashMap<String, Vec<SseClient>>>>,
    invalidation_revisions: InvalidationRevisionMap,
    stream_epoch: Arc<String>,
}

impl SseManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            invalidation_revisions: Arc::new(Mutex::new(HashMap::new())),
            stream_epoch: Arc::new(new_stream_epoch()),
        }
    }

    /// Subscribe a new client to a single project's event stream.
    #[allow(dead_code)]
    pub fn subscribe(&self, project_id: &str) -> Receiver<SseEventFrame> {
        self.subscribe_scoped(project_id, None)
    }

    /// Subscribe to one Execution Space and, optionally, one independently
    /// selected Company Store projection.
    pub fn subscribe_scoped(
        &self,
        execution_space_id: &str,
        company_scope_id: Option<&str>,
    ) -> Receiver<SseEventFrame> {
        self.subscribe_scoped_private(execution_space_id, company_scope_id, None)
    }

    pub fn subscribe_scoped_private(
        &self,
        execution_space_id: &str,
        company_scope_id: Option<&str>,
        private_agent_identity_id: Option<&str>,
    ) -> Receiver<SseEventFrame> {
        let (tx, rx) = bounded(100); // Buffered channel
        let mut clients = self.clients.lock().unwrap();
        clients
            .entry(execution_space_id.to_string())
            .or_default()
            .push(SseClient {
                company_scope_id: company_scope_id.map(str::to_string),
                private_agent_identity_id: private_agent_identity_id.map(str::to_string),
                sender: tx,
            });
        rx
    }

    /// Broadcast an event to the clients subscribed to a single project.
    pub fn broadcast(&self, project_id: &str, frame: SseEventFrame) {
        let mut clients = self.clients.lock().unwrap();
        if let Some(senders) = clients.get_mut(project_id) {
            // Remove clients whose receivers are dropped.
            senders.retain(|client| client.sender.try_send(frame.clone()).is_ok());
        }
    }

    /// Broadcast an invalidation to every active stream selecting this exact
    /// Company Store, regardless of Execution Space, and to no other Company.
    pub fn invalidate_company(&self, company_scope_id: &str, ledger: &str, reason: &str) {
        let frame = SseEventFrame::ProjectionInvalidated(self.next_invalidation(
            "company",
            company_scope_id,
            ledger,
            reason,
        ));
        let mut clients = self.clients.lock().unwrap();
        for subscribers in clients.values_mut() {
            subscribers.retain(|client| {
                if client.company_scope_id.as_deref() == Some(company_scope_id) {
                    client.sender.try_send(frame.clone()).is_ok()
                } else {
                    true
                }
            });
        }
    }

    pub fn invalidate_execution_space(&self, execution_space_id: &str, ledger: &str, reason: &str) {
        let frame = SseEventFrame::ProjectionInvalidated(self.next_invalidation(
            "execution_space",
            execution_space_id,
            ledger,
            reason,
        ));
        self.broadcast(execution_space_id, frame);
    }

    fn next_invalidation(
        &self,
        scope: &str,
        scope_id: &str,
        ledger: &str,
        reason: &str,
    ) -> ProjectionInvalidation {
        let mut revisions = self.invalidation_revisions.lock().unwrap();
        let revision = revisions
            .entry((scope.to_string(), scope_id.to_string(), ledger.to_string()))
            .or_insert(0);
        *revision += 1;
        ProjectionInvalidation {
            scope: scope.to_string(),
            scope_id: scope_id.to_string(),
            ledger: ledger.to_string(),
            revision: *revision,
            reason: reason.to_string(),
            stream_epoch: self.stream_epoch().to_string(),
        }
    }

    pub fn stream_epoch(&self) -> &str {
        self.stream_epoch.as_str()
    }

    /// Directly broadcast an ephemeral member-activity update to current
    /// subscribers of one project. Unlike the durable frame variants, this
    /// deliberately has no watched file: reconnecting clients do not receive a
    /// replay and the activity is not part of any persisted product contract.
    pub fn broadcast_live_provider_activity(
        &self,
        project_id: &str,
        owner_agent_identity_id: &str,
        activity: serde_json::Value,
    ) {
        let frame = SseEventFrame::LiveProviderActivity(activity);
        let mut clients = self.clients.lock().unwrap();
        if let Some(subscribers) = clients.get_mut(project_id) {
            subscribers.retain(|client| {
                if client.private_agent_identity_id.as_deref() == Some(owner_agent_identity_id) {
                    client.sender.try_send(frame.clone()).is_ok()
                } else {
                    true
                }
            });
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
            invalidation_revisions: Arc::clone(&self.invalidation_revisions),
            stream_epoch: Arc::clone(&self.stream_epoch),
        }
    }
}

fn new_stream_epoch() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("serve-{}-{nanos}", std::process::id())
}

/// A live-turn-event normalizer, scoped to one project's store (so the provider
/// session lookup hits the right ledger). Boxed so each project can carry its own.
/// Start a background watcher thread that monitors each project's jsonl files for
/// appends and broadcasts new records to that project's SSE clients only
/// (goal-multi-project P6). One thread polls every watched project serially; the
/// `consumed_offsets` map is keyed by `(project_id, filename)` so identical
/// filenames across projects are tracked independently and never cross streams.
///
/// `rescan` returns the live project-id → store-root map and is called EVERY poll,
/// not just at startup, so a project created or switched-to after serve starts
/// (`POST /v1/projects/switch` or a CLI `project add`) gets a live event channel
/// without a serve restart (goal-multi-project #147 follow-up).
///
/// Seeding policy: projects present at startup are seeded at current EOF so only
/// rows appended after the watcher starts are streamed (the initial snapshot covers
/// pre-existing rows). A project that appears LATER is intentionally NOT EOF-seeded;
/// its offsets default to 0 so its freshly-created ledger streams from the first
/// byte, which makes a row appended right after registration deliverable with no
/// seed-vs-append race (a post-startup project is newly created, so its history is
/// empty/small and the full replay is cheap and deduped by id on the client).
#[allow(dead_code)]
pub fn start_sse_watcher(
    rescan: impl Fn() -> HashMap<String, PathBuf> + Send + 'static,
    manager: SseManager,
) -> std::io::Result<()> {
    start_scoped_sse_watcher(rescan, HashMap::new, manager)
}

/// Start the durable coordination watcher plus an independent Company Store
/// invalidation watcher. Company ledgers are observed only as change signals;
/// their rows are never copied into or decoded as Execution-Space truth.
pub fn start_scoped_sse_watcher(
    rescan: impl Fn() -> HashMap<String, PathBuf> + Send + 'static,
    company_rescan: impl Fn() -> HashMap<String, PathBuf> + Send + 'static,
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
        let mut execution_invalidations = HashMap::new();
        let mut company_invalidations = HashMap::new();
        // Seed offsets at current EOF for the projects known at startup so we only
        // stream rows appended after the watcher starts.
        for (project_id, store_root) in rescan() {
            seed_offsets_at_eof(&project_id, &store_root, &mut consumed_offsets);
            seed_invalidation_files(
                "execution_space",
                &project_id,
                &store_root,
                EXECUTION_INVALIDATION_FILES.iter().copied(),
                &mut execution_invalidations,
            );
            // Typed ledgers normally merge append rows directly. They still
            // need authoritative snapshot invalidation when an external writer
            // atomically replaces, truncates, or deletes the whole file.
            seed_invalidation_files(
                "execution_space",
                &project_id,
                &store_root,
                WATCHED_FILES.iter().copied(),
                &mut execution_invalidations,
            );
        }
        for (company_id, store_root) in company_rescan() {
            seed_invalidation_files(
                "company",
                &company_id,
                &store_root,
                company_ledger_names(&store_root),
                &mut company_invalidations,
            );
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
                poll_project(&project_id, &store_root, &mut consumed_offsets, &manager);
                poll_invalidation_files(
                    "execution_space",
                    &project_id,
                    &store_root,
                    EXECUTION_INVALIDATION_FILES.iter().copied(),
                    &mut execution_invalidations,
                    &manager,
                    true,
                );
                poll_invalidation_files(
                    "execution_space",
                    &project_id,
                    &store_root,
                    WATCHED_FILES.iter().copied(),
                    &mut execution_invalidations,
                    &manager,
                    false,
                );
            }
            for (company_id, store_root) in company_rescan() {
                let company_ledgers = company_ledger_names_with_known(
                    &store_root,
                    &company_id,
                    &company_invalidations,
                );
                poll_invalidation_files(
                    "company",
                    &company_id,
                    &store_root,
                    company_ledgers,
                    &mut company_invalidations,
                    &manager,
                    true,
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

/// The JSONL files tailed in every Execution Space or compatibility
/// coordination store.
const WATCHED_FILES: &[&str] = &[
    "messages.jsonl",
    "workflow_runs.jsonl",
    "workflow_steps.jsonl",
    "team_run_events.jsonl",
    "missions.jsonl",
    "waves.jsonl",
    "team_runs.jsonl",
    "member_runs.jsonl",
    "team_supervisor_leases.jsonl",
    "team_member_close_requests.jsonl",
    "member_actions.jsonl",
    "pending_interactions.jsonl",
];

/// Ledgers represented in the full Dashboard snapshot but not safely merged as
/// one typed row. Any complete external append invalidates the scoped snapshot.
const EXECUTION_INVALIDATION_FILES: &[&str] = &[
    "teams.jsonl",
    "provider_launch_profiles.jsonl",
    "durable_agent_provider_launch_profiles.jsonl",
    "provider_processes.jsonl",
    "evidence.jsonl",
    "provider_child_threads.jsonl",
    "workflow_patches.jsonl",
    "workflow_artifact_manifests.jsonl",
    "delegation_runs.jsonl",
    "work_operations.jsonl",
    "work_delivery_updates.jsonl",
    "host_attentions.jsonl",
    "agentfirm_trust_operations.jsonl",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct InvalidationFileState {
    exists: bool,
    identity: u128,
    modified_nanos: u128,
    observed_len: u64,
    consumed_len: u64,
}

fn company_ledger_names(store_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(store_root) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("company_os_") && name.ends_with(".jsonl"))
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn company_ledger_names_with_known(
    store_root: &Path,
    company_id: &str,
    states: &HashMap<InvalidationKey, InvalidationFileState>,
) -> Vec<String> {
    let mut names = company_ledger_names(store_root);
    names.extend(
        states
            .keys()
            .filter(|key| key.0 == "company" && key.1 == company_id)
            .map(|key| key.2.clone()),
    );
    names.sort();
    names.dedup();
    names
}

fn seed_invalidation_files<I, S>(
    scope: &str,
    scope_id: &str,
    store_root: &Path,
    filenames: I,
    states: &mut HashMap<(String, String, String), InvalidationFileState>,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for filename in filenames {
        let filename = filename.as_ref();
        let path = store_root.join(filename);
        let Ok(metadata) = fs::metadata(path) else {
            continue;
        };
        states.insert(
            (
                scope.to_string(),
                scope_id.to_string(),
                filename.to_string(),
            ),
            InvalidationFileState {
                exists: true,
                identity: file_identity(&metadata),
                modified_nanos: modified_nanos(&metadata),
                observed_len: metadata.len(),
                consumed_len: metadata.len(),
            },
        );
    }
}

fn poll_invalidation_files<I, S>(
    scope: &str,
    scope_id: &str,
    store_root: &Path,
    filenames: I,
    states: &mut HashMap<(String, String, String), InvalidationFileState>,
    manager: &SseManager,
    invalidate_appends: bool,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for filename in filenames {
        let filename = filename.as_ref();
        poll_invalidation_file(
            scope,
            scope_id,
            store_root,
            filename,
            states,
            manager,
            invalidate_appends,
        );
    }
}

fn poll_invalidation_file(
    scope: &str,
    scope_id: &str,
    store_root: &Path,
    filename: &str,
    states: &mut HashMap<(String, String, String), InvalidationFileState>,
    manager: &SseManager,
    invalidate_appends: bool,
) {
    let path = store_root.join(filename);
    let key = (
        scope.to_string(),
        scope_id.to_string(),
        filename.to_string(),
    );
    let Ok(metadata) = fs::metadata(&path) else {
        if states.get(&key).is_some_and(|state| state.exists) {
            states.insert(
                key,
                InvalidationFileState {
                    exists: false,
                    identity: 0,
                    modified_nanos: 0,
                    observed_len: 0,
                    consumed_len: 0,
                },
            );
            emit_invalidation(scope, scope_id, filename, "delete", manager);
        }
        return;
    };
    let identity = file_identity(&metadata);
    let modified_nanos = modified_nanos(&metadata);
    let observed_len = metadata.len();

    let Some(previous) = states.get(&key).cloned() else {
        let consumed_len = complete_prefix_len(&path, 0).unwrap_or(0);
        states.insert(
            key,
            InvalidationFileState {
                exists: true,
                identity,
                modified_nanos,
                observed_len,
                consumed_len,
            },
        );
        if consumed_len > 0 && invalidate_appends {
            emit_invalidation(scope, scope_id, filename, "append", manager);
        }
        return;
    };

    let replacement = !previous.exists
        || identity != previous.identity
        || (observed_len == previous.observed_len
            && modified_nanos != previous.modified_nanos
            && observed_len > 0);
    let truncated = !replacement
        && (observed_len < previous.observed_len || observed_len < previous.consumed_len);
    if replacement || truncated {
        let consumed_len = complete_prefix_len(&path, 0).unwrap_or(0);
        states.insert(
            key,
            InvalidationFileState {
                exists: true,
                identity,
                modified_nanos,
                observed_len,
                consumed_len,
            },
        );
        emit_invalidation(
            scope,
            scope_id,
            filename,
            if replacement { "replace" } else { "truncate" },
            manager,
        );
        return;
    }

    let consumed_len =
        complete_prefix_len(&path, previous.consumed_len).unwrap_or(previous.consumed_len);
    states.insert(
        key,
        InvalidationFileState {
            exists: true,
            identity,
            modified_nanos,
            observed_len,
            consumed_len,
        },
    );
    if invalidate_appends && consumed_len > previous.consumed_len {
        emit_invalidation(scope, scope_id, filename, "append", manager);
    }
}

fn complete_prefix_len(path: &Path, start: u64) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|last_newline| start + last_newline as u64 + 1)
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> u128 {
    use std::os::unix::fs::MetadataExt;
    ((metadata.dev() as u128) << 64) | metadata.ino() as u128
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> u128 {
    0
}

fn modified_nanos(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn emit_invalidation(
    scope: &str,
    scope_id: &str,
    ledger: &str,
    reason: &str,
    manager: &SseManager,
) {
    if scope == "company" {
        manager.invalidate_company(scope_id, ledger, reason);
    } else {
        manager.invalidate_execution_space(scope_id, ledger, reason);
    }
}

/// Keep the incremental SSE read model aligned with the snapshot projection:
/// legacy/manual reasoning actions are not product-visible durable state, even
/// if an old ledger row still contains them. Provider thinking belongs only in
/// the direct-only `LiveProviderActivity` stream.
fn member_action_frames(line: &str) -> Vec<SseEventFrame> {
    serde_json::from_str::<MemberAction>(line)
        .ok()
        .filter(|action| action.action_type != "thinking")
        .map(SseEventFrame::MemberAction)
        .into_iter()
        .collect()
}

/// Poll one project's ledgers once and broadcast any new rows to that project's
/// channel only.
fn poll_project(
    project_id: &str,
    store_root: &Path,
    consumed_offsets: &mut HashMap<(String, String), u64>,
    manager: &SseManager,
) {
    // Native Mission/Wave contract: these ledgers are the durable source for
    // the live console's incremental read model. They remain project-scoped by
    // the common `(project_id, filename)` offsets and manager subscription.
    check_and_broadcast_appends(
        project_id,
        store_root,
        "missions.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<Mission>(line)
                .ok()
                .map(SseEventFrame::Mission)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "waves.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<Wave>(line)
                .ok()
                .map(SseEventFrame::Wave)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "team_runs.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<AgentTeamRun>(line)
                .ok()
                .map(SseEventFrame::AgentTeamRun)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "member_runs.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<ProviderRuntimeProjection>(line)
                .ok()
                .map(|member| SseEventFrame::ProviderRuntimeProjection(Box::new(member)))
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "team_supervisor_leases.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<TeamSupervisorLease>(line)
                .ok()
                .map(SseEventFrame::TeamSupervisorLease)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "team_member_close_requests.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<TeamMemberCloseRequest>(line)
                .ok()
                .map(SseEventFrame::TeamMemberCloseRequest)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "member_actions.jsonl",
        consumed_offsets,
        member_action_frames,
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "pending_interactions.jsonl",
        consumed_offsets,
        |line| {
            serde_json::from_str::<PendingInteraction>(line)
                .ok()
                .map(SseEventFrame::PendingInteraction)
                .into_iter()
                .collect()
        },
        manager,
    );

    check_and_broadcast_appends(
        project_id,
        store_root,
        "messages.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(msg) = serde_json::from_str::<RegistryMessage>(line) {
                vec![SseEventFrame::RegistryMessage(msg)]
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

    // team_run_events.jsonl (Agent Team v0): the folded per-run event log; the
    // team console merges these incrementally over SSE.
    check_and_broadcast_appends(
        project_id,
        store_root,
        "team_run_events.jsonl",
        consumed_offsets,
        |line| {
            if let Ok(event) = serde_json::from_str::<TeamRunEvent>(line) {
                vec![SseEventFrame::TeamRunEvent(event)]
            } else {
                Vec::new()
            }
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
    let mut consumed = consumed_offsets.get(&key).copied().unwrap_or(0);

    // A store file can now SHRINK: `compact_supervisor_leases_unlocked` rewrites
    // team_supervisor_leases.jsonl in place (temp + rename) so heartbeat history
    // stops growing without bound. A grew-only watcher treats the smaller file
    // as "nothing new" and goes permanently silent until it regrows past the
    // pre-compaction size — which for a 23 MB lease file means never. Detect the
    // truncation and re-read the compacted file from the start; it is small by
    // construction, and every row in it is current state a client must see.
    if current_size < consumed {
        consumed = 0;
        consumed_offsets.insert(key.clone(), 0);
    }

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
    let mut observed_durable_row = false;
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
        if !parse_line(trimmed).is_empty() {
            observed_durable_row = true;
        }
    }
    if observed_durable_row {
        manager.invalidate_execution_space(project_id, filename, "append");
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
        MemberActionStatus, RegistryDeliveryStatus, RegistryMessage, RegistryMessageIntent,
        SenderKind, WorkflowRunStatus, WorkflowStepStatus,
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

    fn test_message(id: &str) -> RegistryMessage {
        RegistryMessage {
            id: id.into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: RegistryMessageIntent::Message,
            delivery_status: RegistryDeliveryStatus::Queued,
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
            project_binding_id: None,
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
            host_pid: None,
            dry_run: false,
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
            native_session: None,
            status: WorkflowStepStatus::Running,
            output_summary: None,
            result: None,
            started_at: "unix-ms:1".into(),
            ended_at: None,
            terminal_reason: None,
            partial: false,
        }
    }

    fn message_frame(line: &str) -> Vec<SseEventFrame> {
        serde_json::from_str::<RegistryMessage>(line)
            .ok()
            .map(SseEventFrame::RegistryMessage)
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

    fn test_member_action(id: &str) -> MemberAction {
        MemberAction {
            id: id.into(),
            seq: 1,
            team_run_id: "trun-1".into(),
            member_run_id: "mrun-1".into(),
            task_id: None,
            provider_call_id: None,
            action_type: "command_completed".into(),
            status: MemberActionStatus::Succeeded,
            provider_status: None,
            semantic_status: None,
            title: "Ran focused checks".into(),
            summary: "Focused checks passed".into(),
            evidence_refs: Vec::new(),
            started_at: "unix-ms:1".into(),
            completed_at: Some("unix-ms:2".into()),
        }
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
                SseEventFrame::ProjectionInvalidated(invalidation) => {
                    assert_eq!(invalidation.ledger, "messages.jsonl");
                    received.push(invalidation.revision)
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(
            received,
            vec![1, 2],
            "each completed append invalidates exactly once; torn fragments do not"
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
                SseEventFrame::ProjectionInvalidated(invalidation) => {
                    received.push(invalidation.ledger)
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(received, vec!["messages.jsonl".to_string()]);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// A store file that SHRINKS (lease compaction rewrites it in place) must not
    /// silence the watcher. Regression found by an independent reviewer on the
    /// lease-compaction change: the grew-only guard `current_size <= consumed`
    /// meant a 23 MB lease file compacted to a few hundred bytes would emit
    /// nothing until it regrew past 23 MB.
    #[test]
    fn compacted_file_is_rebroadcast_rather_than_silently_skipped() {
        let root = unique_dir("compaction-truncate");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("messages.jsonl");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        // Grow the file well past what the compacted version will occupy.
        let mut grown = String::new();
        // Stay under the bounded(100) subscriber channel: an overflowing
        // try_send drops the client from the manager and the test would then
        // measure nothing rather than the truncation behaviour.
        for index in 0..50 {
            let row =
                serde_json::to_string(&test_message(&format!("message-{index}"))).expect("ser");
            grown.push_str(&row);
            grown.push('\n');
        }
        std::fs::write(&path, &grown).expect("write grown rows");
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "messages.jsonl",
            &mut offsets,
            message_frame,
            &manager,
        );
        while rx.try_recv().is_ok() {}
        let consumed_before = offsets
            .get(&(TEST_PID.to_string(), "messages.jsonl".to_string()))
            .copied()
            .expect("offset recorded");
        assert!(consumed_before > 0);

        // Compaction: same file, far smaller, carrying current state.
        let compacted =
            serde_json::to_string(&test_message("message-after-compaction")).expect("ser");
        std::fs::write(&path, format!("{compacted}\n")).expect("write compacted");
        assert!(
            std::fs::metadata(&path).expect("meta").len() < consumed_before,
            "compacted file must be smaller than the consumed offset"
        );

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
                SseEventFrame::ProjectionInvalidated(invalidation) => {
                    received.push(invalidation.ledger)
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(
            received,
            vec!["messages.jsonl".to_string()],
            "post-compaction state must invalidate connected clients"
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

        let mut ledgers = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::ProjectionInvalidated(invalidation) => {
                    ledgers.push(invalidation.ledger)
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }

        assert_eq!(ledgers, ["workflow_runs.jsonl", "workflow_steps.jsonl"]);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Member actions are durable Agent Team execution records. They must take
    /// the same project-scoped tail path as the attempt/member/message rows so
    /// a background HTTP start updates an already-open console without polling.
    #[test]
    fn member_action_broadcasts_once_and_stays_project_scoped() {
        let root = unique_dir("member-action");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("member_actions.jsonl");
        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let other_project_rx = manager.subscribe("other-project");
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        let row = serde_json::to_string(&test_member_action("mact-1")).expect("serialize");
        let mut legacy_thinking = test_member_action("mact-thinking");
        legacy_thinking.action_type = "thinking".into();
        let thinking_row = serde_json::to_string(&legacy_thinking).expect("serialize thinking");
        std::fs::write(&path, format!("{row}\n{thinking_row}\n")).expect("write rows");

        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "member_actions.jsonl",
            &mut offsets,
            member_action_frames,
            &manager,
        );
        check_and_broadcast_appends(
            TEST_PID,
            &root,
            "member_actions.jsonl",
            &mut offsets,
            member_action_frames,
            &manager,
        );

        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(invalidation)) => {
                assert_eq!(invalidation.ledger, "member_actions.jsonl")
            }
            other => panic!("expected member action frame, got {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "action must broadcast exactly once and thinking rows must not emit"
        );
        assert!(
            other_project_rx.try_recv().is_err(),
            "member action must not cross project subscriptions"
        );
        assert!(offsets.contains_key(&(TEST_PID.to_string(), "member_actions.jsonl".to_string())));

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

        manager.broadcast(
            "proj-a",
            SseEventFrame::RegistryMessage(test_message("only-a")),
        );

        // A receives it.
        match rx_a.try_recv() {
            Ok(SseEventFrame::RegistryMessage(m)) => assert_eq!(m.id, "only-a"),
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

    /// Identical filenames across two coordination stores are tracked independently:
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
            Ok(SseEventFrame::ProjectionInvalidated(invalidation)) => {
                assert_eq!(invalidation.ledger, "messages.jsonl")
            }
            other => panic!("A should receive its row, got {other:?}"),
        }
        assert!(rx_b.try_recv().is_err(), "B must not see A's row");

        // A's offset advanced; B's is absent (no file to read).
        assert!(offsets.contains_key(&("proj-a".to_string(), "messages.jsonl".to_string())));
        assert!(!offsets.contains_key(&("proj-b".to_string(), "messages.jsonl".to_string())));

        std::fs::remove_dir_all(&root_a).expect("cleanup a");
        std::fs::remove_dir_all(&root_b).expect("cleanup b");
    }

    /// Native Mission/Wave and Agent Team ledgers are tail-able sources for the
    /// console read model. One project poll must parse each native record into
    /// its specific frame without requiring a full snapshot refresh.
    #[test]
    fn native_mission_wave_and_team_ledgers_emit_typed_frames() {
        let root = unique_dir("native-ledgers");
        std::fs::create_dir_all(&root).expect("create root");
        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let other_project_rx = manager.subscribe("other-project");
        let mut offsets: HashMap<(String, String), u64> = HashMap::new();

        let rows = [
            (
                "missions.jsonl",
                include_str!("../../../schemas/fixtures/mission/valid/basic.json"),
            ),
            (
                "waves.jsonl",
                include_str!("../../../schemas/fixtures/wave/valid/basic.json"),
            ),
            (
                "team_runs.jsonl",
                include_str!("../../../schemas/fixtures/agent-team-run/valid/basic.json"),
            ),
        ];
        for (filename, row) in rows {
            // Fixture files are pretty-printed JSON, whereas a JSONL ledger has
            // one compact record per physical line.
            let compact = serde_json::from_str::<serde_json::Value>(row)
                .expect("fixture JSON")
                .to_string();
            std::fs::write(root.join(filename), format!("{compact}\n")).expect("write row");
        }

        poll_project(TEST_PID, &root, &mut offsets, &manager);

        let mut ledgers = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            match frame {
                SseEventFrame::ProjectionInvalidated(invalidation) => {
                    ledgers.push(invalidation.ledger)
                }
                other => panic!("unexpected native-ledger frame {other:?}"),
            }
        }

        assert_eq!(
            ledgers,
            vec!["missions.jsonl", "waves.jsonl", "team_runs.jsonl"]
        );
        for (filename, _) in rows {
            assert!(
                offsets.contains_key(&(TEST_PID.to_string(), filename.to_string())),
                "native ledger {filename} must receive a project-scoped offset"
            );
        }
        assert!(
            other_project_rx.try_recv().is_err(),
            "native ledger frames must stay inside their subscribed project"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn scoped_invalidations_do_not_leak_between_spaces_or_companies() {
        let manager = SseManager::new();
        let a_x = manager.subscribe_scoped("space-a", Some("company-x"));
        let a_y = manager.subscribe_scoped("space-a", Some("company-y"));
        let b_x = manager.subscribe_scoped("space-b", Some("company-x"));
        let b_y = manager.subscribe_scoped("space-b", Some("company-y"));

        manager.invalidate_company("company-x", "company_os_milestones.jsonl", "append");
        for rx in [&a_x, &b_x] {
            match rx.try_recv() {
                Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                    assert_eq!(frame.scope, "company");
                    assert_eq!(frame.scope_id, "company-x");
                    assert_eq!(frame.revision, 1);
                    assert_eq!(frame.stream_epoch, manager.stream_epoch());
                }
                other => panic!("company-x subscriber missing invalidation: {other:?}"),
            }
        }
        assert!(
            a_y.try_recv().is_err(),
            "company X leaked to space A/company Y"
        );
        assert!(
            b_y.try_recv().is_err(),
            "company X leaked to space B/company Y"
        );

        manager.invalidate_execution_space("space-a", "work_operations.jsonl", "append");
        for rx in [&a_x, &a_y] {
            match rx.try_recv() {
                Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                    assert_eq!(frame.scope, "execution_space");
                    assert_eq!(frame.scope_id, "space-a");
                }
                other => panic!("space-a subscriber missing invalidation: {other:?}"),
            }
        }
        assert!(
            b_x.try_recv().is_err(),
            "space A leaked to space B/company X"
        );
        assert!(
            b_y.try_recv().is_err(),
            "space A leaked to space B/company Y"
        );
    }

    #[test]
    fn snapshot_only_execution_ledgers_have_an_invalidation_path() {
        for ledger in [
            "teams.jsonl",
            "provider_launch_profiles.jsonl",
            "durable_agent_provider_launch_profiles.jsonl",
            "provider_processes.jsonl",
            "evidence.jsonl",
            "provider_child_threads.jsonl",
            "workflow_patches.jsonl",
            "workflow_artifact_manifests.jsonl",
            "delegation_runs.jsonl",
            "work_operations.jsonl",
            "work_delivery_updates.jsonl",
        ] {
            assert!(
                EXECUTION_INVALIDATION_FILES.contains(&ledger),
                "snapshot-visible ledger {ledger} has neither a typed delta nor invalidation"
            );
        }
    }

    #[test]
    fn invalidation_watcher_handles_complete_appends_truncation_and_atomic_replace() {
        let root = unique_dir("projection-invalidation");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("work_operations.jsonl");
        std::fs::write(&path, "{\"v\":1}\n").expect("seed ledger");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut states = HashMap::new();
        seed_invalidation_files(
            "execution_space",
            TEST_PID,
            &root,
            ["work_operations.jsonl"],
            &mut states,
        );

        // A torn external write must not claim convergence until its newline is
        // durable; completing it emits exactly one append invalidation.
        let mut file = OpenOptions::new().append(true).open(&path).expect("append");
        file.write_all(b"{\"v\":2}").expect("write torn row");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "work_operations.jsonl",
            &mut states,
            &manager,
            true,
        );
        assert!(rx.try_recv().is_err(), "torn row must not invalidate yet");
        file.write_all(b"\n").expect("complete row");
        file.flush().expect("flush row");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "work_operations.jsonl",
            &mut states,
            &manager,
            true,
        );
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.reason, "append");
                assert_eq!(frame.revision, 1);
            }
            other => panic!("complete append missing invalidation: {other:?}"),
        }
        drop(file);

        // Atomic replacement with the same byte length changes inode, not
        // length. A length-only watcher would stay falsely healthy forever.
        let replacement = root.join("work_operations.jsonl.replace");
        let same_len = std::fs::metadata(&path).expect("metadata").len();
        let replacement_bytes = vec![b' '; same_len.saturating_sub(1) as usize];
        let mut replacement_content = replacement_bytes;
        replacement_content.push(b'\n');
        std::fs::write(&replacement, replacement_content).expect("write replacement");
        std::fs::rename(&replacement, &path).expect("atomic replace");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "work_operations.jsonl",
            &mut states,
            &manager,
            true,
        );
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.reason, "replace");
                assert_eq!(frame.revision, 2);
            }
            other => panic!("same-size replacement missing invalidation: {other:?}"),
        }

        std::fs::write(&path, "").expect("truncate ledger");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "work_operations.jsonl",
            &mut states,
            &manager,
            true,
        );
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.reason, "truncate");
                assert_eq!(frame.revision, 3);
            }
            other => panic!("truncation missing invalidation: {other:?}"),
        }

        std::fs::remove_file(&path).expect("delete ledger");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "work_operations.jsonl",
            &mut states,
            &manager,
            true,
        );
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.reason, "delete");
                assert_eq!(frame.revision, 4);
            }
            other => panic!("deletion missing invalidation: {other:?}"),
        }

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn typed_ledger_replace_delete_and_recreate_invalidate_without_append_noise() {
        let root = unique_dir("typed-projection-invalidation");
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("missions.jsonl");
        std::fs::write(&path, "{\"id\":\"before\"}\n").expect("seed typed ledger");

        let manager = SseManager::new();
        let rx = manager.subscribe(TEST_PID);
        let mut states = HashMap::new();
        seed_invalidation_files(
            "execution_space",
            TEST_PID,
            &root,
            ["missions.jsonl"],
            &mut states,
        );

        // Ordinary appends use typed frames and must not force a full refetch.
        let mut file = OpenOptions::new().append(true).open(&path).expect("append");
        file.write_all(b"{\"id\":\"append\"}\n")
            .expect("append row");
        file.flush().expect("flush append");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "missions.jsonl",
            &mut states,
            &manager,
            false,
        );
        assert!(
            rx.try_recv().is_err(),
            "typed append should stay incremental"
        );
        drop(file);

        let replacement = root.join("missions.jsonl.replace");
        let len = std::fs::metadata(&path).expect("metadata").len();
        let mut bytes = vec![b' '; len.saturating_sub(1) as usize];
        bytes.push(b'\n');
        std::fs::write(&replacement, bytes).expect("write same-size replacement");
        std::fs::rename(&replacement, &path).expect("atomic replace");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "missions.jsonl",
            &mut states,
            &manager,
            false,
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
                if reason == "replace"
        ));

        std::fs::remove_file(&path).expect("delete typed ledger");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "missions.jsonl",
            &mut states,
            &manager,
            false,
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
                if reason == "delete"
        ));

        std::fs::write(&path, "{\"id\":\"after!\"}\n").expect("recreate typed ledger");
        poll_invalidation_file(
            "execution_space",
            TEST_PID,
            &root,
            "missions.jsonl",
            &mut states,
            &manager,
            false,
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
                if reason == "replace"
        ));

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Transient provider activity is sent only to the current exact owner in
    /// its project. A same-project Host, sibling, anonymous subscriber, later
    /// owner subscriber, and other project all receive no replay or payload.
    #[test]
    fn live_provider_activity_is_direct_only_and_project_isolated() {
        let manager = SseManager::new();
        let owner = manager.subscribe_scoped_private("proj-a", None, Some("agent-owner"));
        let host = manager.subscribe_scoped_private("proj-a", None, Some("agent-host"));
        let anonymous = manager.subscribe("proj-a");
        let other_project = manager.subscribe_scoped_private("proj-b", None, Some("agent-owner"));
        let activity = serde_json::json!({
            "member_run_id": "mrun-a",
            "status": "working",
            "summary": "Reading the current implementation"
        });

        manager.broadcast_live_provider_activity("proj-a", "agent-owner", activity.clone());
        let late_owner = manager.subscribe_scoped_private("proj-a", None, Some("agent-owner"));

        match owner.try_recv() {
            Ok(SseEventFrame::LiveProviderActivity(value)) => assert_eq!(value, activity),
            other => panic!("exact owner should receive transient activity, got {other:?}"),
        }
        assert!(
            host.try_recv().is_err(),
            "Host must not see Member-private live activity"
        );
        assert!(
            anonymous.try_recv().is_err(),
            "anonymous stream must not see private activity"
        );
        assert!(
            other_project.try_recv().is_err(),
            "another project must not see activity"
        );
        assert!(
            late_owner.try_recv().is_err(),
            "a later owner subscriber receives no replay"
        );
        assert!(
            !WATCHED_FILES
                .iter()
                .any(|filename| filename.contains("activity")),
            "member activity must never be read from a JSONL watcher"
        );
    }
}
