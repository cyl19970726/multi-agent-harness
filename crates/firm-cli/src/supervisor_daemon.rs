//! Machine-scoped NodeDaemon.
//!
//! One daemon owns the local execution-node lease and supervises every TeamRun
//! admitted from the node's registered Execution Spaces. TeamRun supervisors
//! are children of that daemon generation; they are not independently
//! discoverable or startable daemons.

use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    current_unix_ms_u64, drive_prepared_team_run, prepare_team_run_start_body, CliError, CliResult,
    HarnessStore, PreparedTeamRunStart, TeamRunLedger, TeamSupervisorRegistration,
};

// ---------------------------------------------------------------------------
// Signal handling (portable Unix FFI)
// ---------------------------------------------------------------------------

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
type SigHandler = extern "C" fn(i32);
extern "C" {
    fn signal(signum: i32, handler: SigHandler) -> usize;
}

// ---------------------------------------------------------------------------
// Machine-scoped NodeDaemon (#429)
// ---------------------------------------------------------------------------
// One NodeDaemon manages every local TeamRun across every registered Execution
// Space. A Team never crosses Nodes; a control request therefore names the
// exact Execution Space and TeamRun and is rejected when placement differs.

/// Socket path for the one NodeDaemon that owns a stable local Node identity.
/// Uses a hash-based fallback under /tmp when the FIRM_HOME path exceeds
/// the macOS AF_UNIX 104-byte limit.
pub(crate) fn node_daemon_socket_path(firm_home: &Path, node_id: &str) -> PathBuf {
    let direct = firm_home.join("nodes").join(node_id).join("daemon.sock");
    let direct_str = direct.to_string_lossy();
    if direct_str.len() < 100 {
        return direct;
    }
    // Hash-based fallback for long paths (macOS AF_UNIX 104-byte limit). Node
    // identity remains part of the hash so two local profiles cannot collide.
    let mut hasher = DefaultHasher::new();
    firm_home.to_string_lossy().hash(&mut hasher);
    node_id.hash(&mut hasher);
    let hash = hasher.finish();
    std::path::Path::new("/tmp").join(format!("firm-node-daemon-{hash:x}.sock"))
}

/// A managed TeamRun context inside the NodeDaemon.
struct MultiTeamContext {
    execution_space_id: String,
    project_binding_id: String,
    run_id: String,
    heartbeat_valid: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<CliResult<()>>>,
    started_at: Instant,
}

struct PendingControlConnection {
    stream: UnixStream,
    bytes: Vec<u8>,
    accepted_at: Instant,
}

enum ControlReadState {
    Pending,
    Closed,
    Ready(String),
    Invalid(&'static str),
}

/// The one machine-scoped NodeDaemon.
pub(crate) struct MultiTeamDaemon {
    firm_home: PathBuf,
    node_id: String,
    daemon_id: String,
    instance_id: String,
    contexts: Mutex<Vec<MultiTeamContext>>,
    max_concurrency: usize,
    idle_timeout_secs: u64,
    scan_interval: Duration,
    shutdown: Arc<AtomicBool>,
}

impl MultiTeamDaemon {
    /// Run the multi-team daemon in the foreground. Blocks until SIGTERM/SIGINT
    /// or until the control socket receives a "stop" command.
    pub(crate) fn run(
        firm_home: PathBuf,
        node_id: String,
        max_concurrency: usize,
        idle_timeout_secs: u64,
        scan_interval_secs: u64,
    ) -> CliResult<()> {
        let shutdown = Arc::new(AtomicBool::new(false));

        // Signal handling: use a self-contained pattern where the handler
        // sets an AtomicBool — no static raw pointer (fixes P0-8).
        let shutdown_sig = Arc::clone(&shutdown);
        install_signal_handlers_mt(Arc::clone(&shutdown_sig));

        let socket_path = node_daemon_socket_path(&firm_home, &node_id);
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if socket_path.exists() {
            match UnixStream::connect(&socket_path) {
                Ok(_) => {
                    return Err(CliError::Usage(format!(
                        "NODE_DAEMON_ALREADY_RUNNING: Node {node_id} is already served at {}",
                        socket_path.display()
                    )))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    Self::ensure_stale_socket_reclaimable(&firm_home, &node_id)?;
                    std::fs::remove_file(&socket_path).map_err(|remove_error| {
                        CliError::Usage(format!(
                            "cannot remove stale supervisor socket at {}: {remove_error}",
                            socket_path.display()
                        ))
                    })?;
                }
                Err(error) => {
                    return Err(CliError::Usage(format!(
                        "cannot verify supervisor socket at {}: {error}",
                        socket_path.display()
                    )))
                }
            }
        }

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            CliError::Usage(format!(
                "cannot bind supervisor socket at {}: {e}",
                socket_path.display()
            ))
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|e| CliError::Usage(format!("cannot set socket non-blocking: {e}")))?;

        let daemon_id = format!("node-daemon:{node_id}");
        let instance_id = format!(
            "{}:{}:{}",
            std::process::id(),
            current_unix_ms_u64(),
            daemon_id
        );
        eprintln!(
            "[node-daemon] Node {node_id} listening on {}",
            socket_path.display()
        );

        let daemon = MultiTeamDaemon {
            firm_home,
            node_id,
            daemon_id,
            instance_id,
            contexts: Mutex::new(Vec::new()),
            max_concurrency,
            idle_timeout_secs,
            scan_interval: Duration::from_secs(scan_interval_secs),
            shutdown: shutdown_sig,
        };

        // Always remove the socket and drain managed supervisors, even when a
        // store or control-loop error stops the foreground service.
        let serve_result = daemon
            .recover_orphaned_runs()
            .and_then(|()| daemon.serve_loop(&listener));
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
        let shutdown_result = daemon.graceful_shutdown();

        let release_result = daemon.release_node_authorities();
        eprintln!("[node-daemon] shutdown complete");
        serve_result.and(shutdown_result).and(release_result)
    }

    /// A dead socket is not sufficient evidence that the previous daemon
    /// generation lost authority. Every registered Execution Space must
    /// either have no active lease for this Node or have an expired one before
    /// the filesystem rendezvous may be reclaimed. An unreadable Store is
    /// fail-closed because it may contain the live lease we are trying not to
    /// steal.
    fn ensure_stale_socket_reclaimable(firm_home: &Path, node_id: &str) -> CliResult<()> {
        let spaces = crate::execution_space::list_spaces(firm_home).map_err(|error| {
            CliError::Usage(format!(
                "NODE_DAEMON_SOCKET_RECLAIM_UNSAFE: cannot list Execution Spaces: {error}"
            ))
        })?;
        let now_ms = current_unix_ms_u64();
        for space in spaces {
            let store = HarnessStore::new(space.store_root.clone());
            let lease = store.latest_node_daemon_lease(node_id).map_err(|error| {
                CliError::Usage(format!(
                    "NODE_DAEMON_SOCKET_RECLAIM_UNSAFE: cannot verify Node {node_id} authority in Execution Space {}: {error}",
                    space.id
                ))
            })?;
            if let Some(lease) = lease {
                if lease.status == harness_core::NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > now_ms
                {
                    return Err(CliError::Usage(format!(
                        "NODE_DAEMON_LEASE_HELD: refusing to remove stale socket for Node {node_id}; Execution Space {} is held by {} generation {} until {}",
                        space.id,
                        lease.daemon_id,
                        lease.generation,
                        lease.expires_unix_ms
                    )));
                }
            }
        }
        Ok(())
    }

    /// Enumerate non-terminal team-runs and adopt runs whose supervisor lease
    /// is expired (no live supervisor elsewhere).
    fn recover_orphaned_runs(&self) -> CliResult<()> {
        self.scan_and_adopt()
    }

    fn registered_spaces(&self) -> CliResult<Vec<(harness_core::ExecutionSpace, HarnessStore)>> {
        let spaces = crate::execution_space::list_spaces(&self.firm_home).map_err(|error| {
            CliError::Usage(format!(
                "cannot list Execution Spaces for NodeDaemon: {error}"
            ))
        })?;
        Ok(spaces
            .into_iter()
            .map(|space| {
                let store = HarnessStore::new(space.store_root.clone());
                (space, store)
            })
            .collect())
    }

    fn node_lease_ttl_ms(&self) -> u64 {
        self.scan_interval
            .as_millis()
            .min(u64::MAX as u128)
            .try_into()
            .unwrap_or(u64::MAX)
            .saturating_mul(4)
            .max(15_000)
    }

    /// Acquire or renew this process' parent authority in one registered
    /// Execution Space. A malformed/broken Space is isolated by the caller.
    fn ensure_node_authority(
        &self,
        space: &harness_core::ExecutionSpace,
        store: &HarnessStore,
    ) -> CliResult<harness_core::NodeDaemonLease> {
        let node = store
            .latest_execution_nodes()?
            .into_iter()
            .find(|node| node.id == self.node_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "NODE_NOT_ENROLLED: Node {} is absent from Execution Space {}",
                    self.node_id, space.id
                ))
            })?;
        if node.status == harness_core::ExecutionNodeStatus::Retired {
            return Err(CliError::Usage(format!(
                "NODE_NOT_ACTIVE: Node {} is retired in Execution Space {}",
                self.node_id, space.id
            )));
        }
        let registered =
            store
                .latest_node_project_registrations()?
                .into_iter()
                .any(|registration| {
                    registration.node_id == self.node_id
                        && registration.execution_space_id == space.id
                        && registration.status
                            == harness_core::NodeProjectRegistrationStatus::Active
                });
        if !registered {
            return Err(CliError::Usage(format!(
                "NODE_HAS_NO_REGISTERED_PROJECT: Node {} has no active project in Execution Space {}",
                self.node_id, space.id
            )));
        }
        let now_ms = current_unix_ms_u64();
        let ttl_ms = self.node_lease_ttl_ms();
        let lease = store
            .acquire_node_daemon_lease(
                &self.node_id,
                &self.daemon_id,
                &self.instance_id,
                now_ms,
                ttl_ms,
            )
            .map_err(CliError::Store)?;
        store
            .renew_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                now_ms,
                ttl_ms,
            )
            .map_err(CliError::Store)
    }

    fn release_node_authorities(&self) -> CliResult<()> {
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    eprintln!(
                        "[node-daemon] isolating Execution Space {} during Node authority release: {error}",
                        space.id
                    );
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id || lease.instance_id != self.instance_id {
                continue;
            }
            if let Err(error) = store.release_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
            ) {
                eprintln!(
                    "[node-daemon] failed to release Node authority in {}: {error}",
                    space.id
                );
            }
        }
        Ok(())
    }

    /// Main loop: scan for new runs, reap finished contexts, poll control socket.
    fn serve_loop(&self, listener: &UnixListener) -> CliResult<()> {
        const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
        let mut pending = Vec::new();
        let mut next_scan = Instant::now();

        while !self.shutdown.load(Ordering::SeqCst) {
            if Instant::now() >= next_scan {
                self.scan_and_adopt()?;
                self.reap_finished()?;
                next_scan = Instant::now() + self.scan_interval;
            }

            self.poll_control_socket(listener, &mut pending);
            std::thread::sleep(CONTROL_POLL_INTERVAL);
        }
        Ok(())
    }

    /// Scan every registered Execution Space. One broken Store is logged and
    /// isolated; it cannot stop healthy local Teams from progressing.
    fn scan_and_adopt(&self) -> CliResult<()> {
        let mut managed_ids: HashSet<(String, String)> = {
            let ctx = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            ctx.iter()
                .map(|context| (context.execution_space_id.clone(), context.run_id.clone()))
                .collect()
        };

        for (space, store) in self.registered_spaces()? {
            if let Err(error) = self.ensure_node_authority(&space, &store) {
                eprintln!(
                    "[node-daemon] isolating Execution Space {} during authority refresh: {error}",
                    space.id
                );
                continue;
            }
            let runs = match crate::latest_team_runs_in_append_order(&store) {
                Ok(runs) => runs,
                Err(error) => {
                    eprintln!(
                        "[node-daemon] isolating Execution Space {} after Store read failure: {error}",
                        space.id
                    );
                    continue;
                }
            };
            for run in runs {
                if run.execution_node_id != self.node_id
                    || !matches!(run.status, harness_core::TeamRunStatus::Running)
                    || managed_ids.contains(&(space.id.clone(), run.id.clone()))
                {
                    continue;
                }
                let now_ms = current_unix_ms_u64();
                let should_start = match store.latest_team_supervisor_lease(&run.id) {
                    Ok(None) => true,
                    Ok(Some(lease)) => {
                        lease.status != harness_core::TeamSupervisorLeaseStatus::Active
                            || lease.expires_unix_ms <= now_ms
                    }
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect TeamRun {} in {}: {error}",
                            run.id, space.id
                        );
                        false
                    }
                };
                if should_start {
                    eprintln!(
                        "[node-daemon] adopting {}/{} on Node {}",
                        space.id, run.id, self.node_id
                    );
                    match self.start_supervising(space.clone(), store.clone(), &run.id) {
                        Ok(()) => {
                            managed_ids.insert((space.id.clone(), run.id.clone()));
                        }
                        Err(error) => {
                            eprintln!(
                                "[node-daemon] failed to adopt {}/{}: {error}",
                                space.id, run.id
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Spawn one Team supervisor under this exact NodeDaemon generation.
    fn start_supervising(
        &self,
        space: harness_core::ExecutionSpace,
        store: HarnessStore,
        run_id: &str,
    ) -> CliResult<()> {
        self.ensure_node_authority(&space, &store)?;
        // P0-2 fix: enforce concurrent team-run limit.
        {
            let contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            if contexts
                .iter()
                .any(|context| context.execution_space_id == space.id && context.run_id == run_id)
            {
                return Err(CliError::Usage(format!(
                    "NodeDaemon already manages {}/{run_id}",
                    space.id
                )));
            }
            if contexts.len() >= self.max_concurrency {
                return Err(CliError::Usage(format!(
                    "NodeDaemon at capacity ({}/{} runs); cannot start {}/{run_id}",
                    contexts.len(),
                    self.max_concurrency,
                    space.id,
                )));
            }
        }

        let run_id = run_id.to_string();
        let max_concurrency = self.max_concurrency;
        let idle_timeout_secs = self.idle_timeout_secs;

        // Validate and create registration outside the context lock (fixes P0-7).
        let body = prepare_team_run_start_body(&store, &run_id, max_concurrency)?;
        if body.run.execution_node_id != self.node_id {
            return Err(CliError::Usage(format!(
                "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun {run_id} belongs to Node {}, local Node is {}",
                body.run.execution_node_id, self.node_id
            )));
        }
        let project_binding_id = body.run.project_binding_id.clone();
        let registration = TeamSupervisorRegistration::start(&store, &run_id, Some(&space.id))?;
        let heartbeat_valid = Arc::clone(&registration.heartbeat_valid);

        // Transition Planning→Running when the child supervisor is admitted.
        use crate::{now_string, store_conflict_as_usage};
        use harness_core::TeamRunStatus;

        let running = if body.run.status == TeamRunStatus::Planning {
            let mut running = body.run.clone();
            running.status = TeamRunStatus::Running;
            running.updated_at = now_string();
            store_conflict_as_usage(
                store.compare_and_append_team_run_lifecycle(&body.run, &running),
            )?;
            running
        } else {
            body.run.clone()
        };

        let ledger = Arc::new(TeamRunLedger::new(
            &store,
            &run_id,
            &registration.supervisor_id,
            registration.generation,
            Arc::clone(&registration.heartbeat_valid),
        ));

        ledger.fold_event(
            harness_core::TeamRunEventSourceKind::Host,
            None,
            "team_run",
            &run_id,
            "updated",
            &format!(
                "member supervisor {} generation {} {} ({} unclosed member(s), max-concurrency {max_concurrency})",
                registration.supervisor_id,
                registration.generation,
                if body.run.status == TeamRunStatus::Planning {
                    "started"
                } else {
                    "reattached"
                },
                body.members.len(),
            ),
        )?;

        let prepared = PreparedTeamRunStart {
            run_id: body.run_id,
            objective: body.objective,
            running,
            members: body.members,
            ledger,
            supervisor_registration: registration,
        };

        eprintln!(
            "[node-daemon] {}/{}: serving (pid {}, gen {})",
            space.id,
            run_id,
            std::process::id(),
            prepared.supervisor_registration.generation,
        );

        let execution_space_id = space.id.clone();
        let thread = std::thread::spawn(move || {
            drive_prepared_team_run(
                prepared,
                Some(space),
                None,
                max_concurrency,
                Duration::from_secs(idle_timeout_secs),
                None,
            )
        });

        // P0-7 fix: lock only for the insert, not around store I/O.
        {
            let mut contexts = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            contexts.push(MultiTeamContext {
                execution_space_id,
                project_binding_id,
                run_id,
                heartbeat_valid,
                thread: Some(thread),
                started_at: Instant::now(),
            });
        }
        Ok(())
    }

    /// Reap finished supervisor threads and remove them from the context registry.
    fn reap_finished(&self) -> CliResult<()> {
        let mut contexts = self
            .contexts
            .lock()
            .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;

        let mut finished = Vec::new();
        let mut still_running = Vec::new();

        for ctx in contexts.drain(..) {
            let is_done = ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
            if is_done {
                finished.push(ctx);
            } else {
                still_running.push(ctx);
            }
        }

        *contexts = still_running;

        for ctx in finished {
            if let Some(thread) = ctx.thread {
                match thread.join() {
                    Ok(Ok(())) => {
                        eprintln!(
                            "[node-daemon] {}/{} completed",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                    Ok(Err(e)) => {
                        eprintln!(
                            "[node-daemon] {}/{} error: {e}",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                    Err(_) => {
                        eprintln!(
                            "[node-daemon] {}/{} panicked",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Accept new control connections and advance every partial command once.
    /// Per-client framing and I/O failures never escape into the daemon loop.
    fn poll_control_socket(
        &self,
        listener: &UnixListener,
        pending: &mut Vec<PendingControlConnection>,
    ) {
        loop {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(error) = stream.set_nonblocking(true) {
                        eprintln!("[node-daemon] cannot configure client socket: {error}");
                        continue;
                    }
                    pending.push(PendingControlConnection {
                        stream,
                        bytes: Vec::new(),
                        accepted_at: Instant::now(),
                    });
                }
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    eprintln!("[node-daemon] socket accept error: {error}");
                    break;
                }
            }
        }

        let mut index = 0;
        while index < pending.len() {
            let state = Self::read_control_command(&mut pending[index]);
            match state {
                ControlReadState::Pending => index += 1,
                ControlReadState::Closed => {
                    pending.swap_remove(index);
                }
                ControlReadState::Ready(command) => {
                    let mut connection = pending.swap_remove(index);
                    if let Err(error) =
                        self.handle_control_command(&mut connection.stream, command.trim())
                    {
                        eprintln!("[node-daemon] control client error: {error}");
                    }
                }
                ControlReadState::Invalid(error) => {
                    let mut connection = pending.swap_remove(index);
                    let response = serde_json::json!({"ok": false, "error": error});
                    if let Err(write_error) =
                        Self::write_control_response(&mut connection.stream, &response)
                    {
                        eprintln!("[node-daemon] control client error: {write_error}");
                    }
                }
            }
        }
    }

    fn read_control_command(connection: &mut PendingControlConnection) -> ControlReadState {
        const MAX_CONTROL_BYTES: usize = 64 * 1024;
        let mut chunk = [0_u8; 4096];
        loop {
            match connection.stream.read(&mut chunk) {
                Ok(0) if connection.bytes.is_empty() => return ControlReadState::Closed,
                Ok(0) => {
                    return ControlReadState::Invalid(
                        "control command must be one newline-terminated JSON object",
                    )
                }
                Ok(count) => {
                    connection.bytes.extend_from_slice(&chunk[..count]);
                    if connection.bytes.len() > MAX_CONTROL_BYTES {
                        return ControlReadState::Invalid("control command exceeds 64 KiB");
                    }
                    if let Some(newline) = connection.bytes.iter().position(|byte| *byte == b'\n') {
                        return match String::from_utf8(connection.bytes[..newline].to_vec()) {
                            Ok(command) => ControlReadState::Ready(command),
                            Err(_) => {
                                ControlReadState::Invalid("control command is not valid UTF-8")
                            }
                        };
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if connection.accepted_at.elapsed() >= Duration::from_secs(1) {
                        return ControlReadState::Invalid(
                            "control command must be one newline-terminated JSON object",
                        );
                    }
                    return ControlReadState::Pending;
                }
                Err(_) => return ControlReadState::Closed,
            }
        }
    }

    fn write_control_response(
        stream: &mut UnixStream,
        response: &serde_json::Value,
    ) -> CliResult<()> {
        writeln!(stream, "{response}").map_err(CliError::Io)?;
        stream.flush().map_err(CliError::Io)
    }

    /// Handle a single control socket command.
    fn handle_control_command(&self, stream: &mut UnixStream, cmd_line: &str) -> CliResult<()> {
        let cmd: serde_json::Value = match serde_json::from_str(cmd_line) {
            Ok(v) => v,
            Err(e) => {
                let response = serde_json::json!({
                    "ok": false,
                    "error": format!("invalid json: {e}"),
                });
                Self::write_control_response(stream, &response)?;
                return Ok(());
            }
        };

        let cmd_name = cmd["cmd"].as_str().unwrap_or("");
        match cmd_name {
            "start" => {
                let run_id = cmd["run_id"].as_str().unwrap_or("");
                let execution_space_id = cmd["execution_space_id"].as_str().unwrap_or("");
                if run_id.is_empty() || execution_space_id.is_empty() {
                    let response = serde_json::json!({
                        "ok": false,
                        "error": "execution_space_id and run_id are required"
                    });
                    Self::write_control_response(stream, &response)?;
                    return Ok(());
                }
                let already_managed = self
                    .contexts
                    .lock()
                    .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?
                    .iter()
                    .any(|context| {
                        context.execution_space_id == execution_space_id && context.run_id == run_id
                    });
                if already_managed {
                    let response = serde_json::json!({
                        "ok": true,
                        "execution_space_id": execution_space_id,
                        "run_id": run_id,
                        "reused": true
                    });
                    Self::write_control_response(stream, &response)?;
                    return Ok(());
                }
                let space =
                    crate::execution_space::context_for_id(&self.firm_home, execution_space_id)
                        .map_err(|error| {
                            CliError::Usage(format!(
                                "cannot resolve Execution Space {execution_space_id}: {error}"
                            ))
                        })?
                        .ok_or_else(|| {
                            CliError::Usage(format!(
                                "Execution Space not found: {execution_space_id}"
                            ))
                        })?;
                let store = HarnessStore::new(space.store_root.clone());
                match self.start_supervising(space, store, run_id) {
                    Ok(()) => {
                        let response = serde_json::json!({
                            "ok": true,
                            "execution_space_id": execution_space_id,
                            "run_id": run_id,
                            "reused": false,
                        });
                        Self::write_control_response(stream, &response)?;
                    }
                    Err(e) => {
                        let response = serde_json::json!({
                            "ok": false,
                            "execution_space_id": execution_space_id,
                            "run_id": run_id,
                            "error": e.to_string(),
                        });
                        Self::write_control_response(stream, &response)?;
                    }
                }
            }
            "status" => {
                let runs: Vec<serde_json::Value> = {
                    let contexts = self
                        .contexts
                        .lock()
                        .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
                    contexts
                        .iter()
                        .map(|ctx| {
                            let is_finished =
                                ctx.thread.as_ref().map(|t| t.is_finished()).unwrap_or(true);
                            serde_json::json!({
                                "execution_space_id": ctx.execution_space_id,
                                "project_binding_id": ctx.project_binding_id,
                                "run_id": ctx.run_id,
                                "status": if is_finished { "finished" } else { "running" },
                                "elapsed_secs": ctx.started_at.elapsed().as_secs(),
                            })
                        })
                        .collect()
                };
                let resp = serde_json::json!({
                    "ok": true,
                    "node_id": self.node_id,
                    "daemon_id": self.daemon_id,
                    "instance_id": self.instance_id,
                    "process_id": std::process::id(),
                    "runs": runs
                });
                Self::write_control_response(stream, &resp)?;
            }
            "stop" => {
                // P0-1 fix: actually set shutdown, not just reply ok.
                self.shutdown.store(true, Ordering::SeqCst);
                Self::write_control_response(stream, &serde_json::json!({"ok": true}))?;
            }
            _ => {
                let response = serde_json::json!({
                    "ok": false,
                    "error": format!("unknown command: {cmd_name}"),
                });
                Self::write_control_response(stream, &response)?;
            }
        }
        Ok(())
    }

    /// Graceful shutdown: signal all managed contexts to stop, drain them,
    /// and join threads with a deadline.
    fn graceful_shutdown(&self) -> CliResult<()> {
        eprintln!("[node-daemon] graceful shutdown initiated");

        // Drain contexts from the registry.
        let contexts: Vec<MultiTeamContext> = {
            let mut guard = self
                .contexts
                .lock()
                .map_err(|e| CliError::Usage(format!("context lock poisoned: {e}")))?;
            std::mem::take(&mut *guard)
        };

        if contexts.is_empty() {
            return Ok(());
        }

        eprintln!(
            "[node-daemon] waiting for {} run(s) to finish...",
            contexts.len()
        );

        // P0-1 fix: signal every managed context to stop by invalidating
        // their heartbeat.  This causes drive_prepared_team_run to detect
        // lease loss and exit its main loop promptly.
        for ctx in &contexts {
            ctx.heartbeat_valid.store(false, Ordering::Release);
        }

        // P0-3 fix: join threads with a deadline.  Since std::thread::JoinHandle
        // lacks join_timeout, we poll is_finished() with a sleep loop.
        const JOIN_DEADLINE_SECS: u64 = 30;
        let deadline = Instant::now() + Duration::from_secs(JOIN_DEADLINE_SECS);

        for ctx in contexts {
            let Some(thread) = ctx.thread else { continue };
            loop {
                if thread.is_finished() {
                    let _ = thread.join();
                    break;
                }
                if Instant::now() >= deadline {
                    eprintln!(
                        "[node-daemon] shutdown deadline exceeded for {}/{}",
                        ctx.execution_space_id, ctx.run_id
                    );
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Multi-team daemon signal handling (channel-based, no static raw pointer)
// ---------------------------------------------------------------------------

fn install_signal_handlers_mt(shutdown: Arc<AtomicBool>) {
    // P0-8 fix: leak the Arc to get a 'static reference for the signal
    // handler. The leaked memory is reclaimed at process exit. This avoids
    // dangling raw pointers while still
    // being async-signal-safe.
    let leaked: &'static AtomicBool = Box::leak(Box::new(shutdown));

    extern "C" fn handle(sig: i32) {
        let _ = sig;
        // SAFETY: MT_SIGNAL_FLAG is set before signal handlers are installed
        // and lives for the process lifetime. The store is async-signal-safe.
        // We access the static mut via raw pointer to avoid the static_mut_refs
        // warning in Rust 2024 edition.
        unsafe {
            let ptr: *const Option<&'static AtomicBool> = &raw const MT_SIGNAL_FLAG;
            if let Some(flag) = &*ptr {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    unsafe {
        MT_SIGNAL_FLAG = Some(leaked);
        signal(SIGTERM, handle as SigHandler);
        signal(SIGINT, handle as SigHandler);
    }
}

static mut MT_SIGNAL_FLAG: Option<&'static AtomicBool> = None;

// ---------------------------------------------------------------------------
// CLI integration: delegate TeamRun start to the machine NodeDaemon
// ---------------------------------------------------------------------------

/// Send an exact Execution Space + TeamRun start request to the local Node.
/// Returns the response line on success.
pub(crate) fn try_delegate_to_node_daemon(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Result<String, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let cmd = serde_json::json!({
        "cmd": "start",
        "execution_space_id": execution_space_id,
        "run_id": run_id
    });
    let cmd_str = serde_json::to_string(&cmd).map_err(std::io::Error::other)?;
    writeln!(stream, "{cmd_str}")?;
    stream.flush()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

/// Send a status request to the machine NodeDaemon.
pub(crate) fn daemon_status_via_socket(firm_home: &Path, node_id: &str) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .ok()?;

    let cmd = r#"{"cmd":"status"}"#;
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).ok()?;
    let response = buf.trim().to_string();
    if response.is_empty() {
        return None;
    }
    Some(response)
}

/// Send a stop command to the multi-team daemon.
pub(crate) fn daemon_stop_via_socket(firm_home: &Path, node_id: &str) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let cmd = r#"{"cmd":"stop"}"#;
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader.read_line(&mut buf).ok()?;
    Some(buf.trim().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_daemon_socket_path_short_home() {
        let root = std::path::Path::new("/tmp/firm-test");
        let path = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
        assert_eq!(
            path,
            root.join("nodes")
                .join("00000000-0000-4000-8000-000000000001")
                .join("daemon.sock")
        );
    }

    #[test]
    fn node_daemon_socket_path_long_home_fallback() {
        let long = "/tmp/very-long-directory-name-that-makes-the-path-exceed-the-af-unix-limit-on-macos-which-is-104-bytes".repeat(2);
        let root = std::path::Path::new(&long);
        let path = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
        assert!(path.to_string_lossy().starts_with("/tmp/firm-node-daemon-"));
        assert!(path.to_string_lossy().len() < 104);
    }

    #[test]
    fn node_daemon_socket_path_is_stable_per_node() {
        let root = std::path::Path::new("/some/store/root");
        let p1 = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
        let p2 = node_daemon_socket_path(root, "00000000-0000-4000-8000-000000000001");
        assert_eq!(p1, p2);
    }
}
