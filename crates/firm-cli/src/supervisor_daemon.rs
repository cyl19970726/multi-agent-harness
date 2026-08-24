//! Machine-scoped NodeDaemon.
//!
//! One daemon owns the local execution-node lease and supervises every TeamRun
//! admitted from the node's registered Execution Spaces. TeamRun supervisors
//! are children of that daemon generation; they are not independently
//! discoverable or startable daemons.

use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    bind_team_runtime_supervisor, current_unix_ms_u64, drive_prepared_team_run,
    ensure_team_message_fabric, ensure_team_runtime_fabric, prepare_team_run_start_body, CliError,
    CliResult, HarnessStore, LiveProviderActivityUpdate, PreparedTeamRunStart, TeamRunLedger,
    TeamSupervisorRegistration,
};

// ---------------------------------------------------------------------------
// Signal handling (portable Unix FFI)
// ---------------------------------------------------------------------------

mod control_protocol;
mod recovery;
mod shutdown;
pub(crate) use recovery::reconcile_team_run_start_postcondition;

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
    // FIRM_HOME may reach the same directory through filesystem aliases (for
    // example macOS exposes /tmp through /private/tmp). The daemon socket is
    // machine-scoped authority, so derive both the direct path and long-path
    // hash from one canonical filesystem identity instead of the caller's raw
    // spelling. The home already exists in normal daemon flows; the
    // best-effort fallback preserves deterministic behavior during setup and
    // focused path tests.
    let firm_home = crate::project::canonicalize_best_effort(firm_home);
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
    daemon_generation: u64,
    supervisor_id: String,
    supervisor_generation: u64,
    heartbeat_valid: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<CliResult<()>>>,
    started_at: Instant,
}

struct PendingControlConnection {
    stream: UnixStream,
    bytes: Vec<u8>,
    accepted_at: Instant,
}

#[derive(Clone)]
struct LiveProviderActivityEndpoint {
    authority: String,
    token: String,
}

enum ControlReadState {
    Pending,
    Closed,
    Ready(String),
    Invalid(&'static str),
}

fn daemon_control_generation_authorized(
    lease: Option<&harness_core::NodeDaemonLease>,
    daemon_id: &str,
    instance_id: &str,
    expected_generation: u64,
    now_ms: u64,
) -> bool {
    lease.is_some_and(|lease| {
        lease.daemon_id == daemon_id
            && lease.instance_id == instance_id
            && lease.generation == expected_generation
            && lease.status == harness_core::NodeDaemonLeaseStatus::Active
            && lease.expires_unix_ms > now_ms
    })
}

fn node_authority_refresh_interval(scan_interval: Duration) -> Duration {
    scan_interval
        .min(Duration::from_secs(5))
        .max(Duration::from_secs(1))
}

/// The one machine-scoped NodeDaemon.
pub(crate) struct MultiTeamDaemon {
    firm_home: PathBuf,
    node_id: String,
    daemon_id: String,
    instance_id: String,
    contexts: Mutex<Vec<MultiTeamContext>>,
    /// Serializes only Supervisor admission. Recovery discovery and an
    /// explicit Start may target the same TeamRun concurrently; the separate
    /// gate closes that lease-acquisition window without holding `contexts`
    /// across Store/provider admission or blocking the reserved control lane.
    supervisor_start_gate: Mutex<()>,
    /// Machine-local provider handles keyed by canonical AgentSession id.
    /// Team membership is intentionally absent from this registry.
    session_runtimes: Mutex<HashMap<String, crate::provider_adapter::NodeSessionRuntime>>,
    /// Volatile callback registered by the current local `serve` process. It
    /// is never written to the Store and a daemon restart deliberately loses
    /// it. The bearer token is required on every loopback ingress request.
    live_provider_activity_endpoint: Arc<Mutex<Option<LiveProviderActivityEndpoint>>>,
    max_concurrency: usize,
    idle_timeout_secs: u64,
    scan_interval: Duration,
    /// Stops control acceptance and discovery, but deliberately does not stop
    /// authority renewal while already-accepted mutations are draining.
    stop_requested: Arc<AtomicBool>,
    /// Ends the NodeDaemon lease heartbeat only after accepted workers and
    /// managed supervisors have converged.
    authority_shutdown: Arc<AtomicBool>,
    /// Latches an accepted worker that panicked or returned without proving
    /// command completion. Such a generation may drain but never Release.
    control_worker_failed: AtomicBool,
    /// Volatile companion to the durable recovery MemberAction. It closes the
    /// rescan window even when the recovery projection itself cannot be
    /// written because the Execution Space is temporarily unavailable.
    recovery_blocked_runs: Mutex<HashSet<(String, String)>>,
    #[cfg(test)]
    lease_ttl_override_ms: Option<u64>,
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
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
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

        let daemon = Arc::new(MultiTeamDaemon {
            firm_home,
            node_id,
            daemon_id,
            instance_id,
            contexts: Mutex::new(Vec::new()),
            supervisor_start_gate: Mutex::new(()),
            session_runtimes: Mutex::new(HashMap::new()),
            live_provider_activity_endpoint: Arc::new(Mutex::new(None)),
            max_concurrency,
            idle_timeout_secs,
            scan_interval: Duration::from_secs(scan_interval_secs),
            stop_requested: shutdown_sig,
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            control_worker_failed: AtomicBool::new(false),
            recovery_blocked_runs: Mutex::new(HashSet::new()),
            #[cfg(test)]
            lease_ttl_override_ms: None,
        });

        // `serve_loop` owns the two-phase shutdown: it stops accepting new
        // control work, keeps authority alive while accepted work and managed
        // supervisors converge, and only then drains/releases the generation.
        let serve_result = daemon.serve_loop(&listener);
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
        eprintln!("[node-daemon] shutdown complete");
        serve_result
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
        #[cfg(test)]
        if let Some(ttl_ms) = self.lease_ttl_override_ms {
            return ttl_ms;
        }
        self.scan_interval
            .as_millis()
            .min(u64::MAX as u128)
            .try_into()
            .unwrap_or(u64::MAX)
            .saturating_mul(4)
            .max(15_000)
    }

    /// Renew only authority already owned by this exact daemon instance.
    /// Discovery remains responsible for first acquisition; this heartbeat is
    /// deliberately unable to steal or create authority in an unscanned Space.
    fn refresh_held_node_authorities(&self) -> CliResult<()> {
        let now_ms = current_unix_ms_u64();
        let ttl_ms = self.node_lease_ttl_ms();
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    eprintln!(
                        "[node-daemon] cannot refresh Node authority in {}: {error}",
                        space.id
                    );
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id
                || lease.instance_id != self.instance_id
                || lease.status != harness_core::NodeDaemonLeaseStatus::Active
            {
                continue;
            }
            if let Err(error) = store.renew_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                now_ms,
                ttl_ms,
            ) {
                eprintln!(
                    "[node-daemon] cannot refresh Node authority in {}: {error}",
                    space.id
                );
            }
        }
        Ok(())
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
        let mut failures = Vec::new();
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    failures.push(format!("{}: {error}", space.id));
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
                failures.push(format!("{}: {error}", space.id));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_RELEASE_INCOMPLETE: {}",
                failures.join("; ")
            )))
        }
    }

    fn drain_node_authorities(&self) -> CliResult<()> {
        const DRAIN_TTL_MS: u64 = 60_000;
        let mut failures = Vec::new();
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    failures.push(format!("{}: {error}", space.id));
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id || lease.instance_id != self.instance_id {
                continue;
            }
            if let Err(error) = store.drain_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
                DRAIN_TTL_MS,
            ) {
                failures.push(format!("{}: {error}", space.id));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_DRAIN_INCOMPLETE: {}",
                failures.join("; ")
            )))
        }
    }

    /// Keep the machine control plane responsive while durable discovery and
    /// provider recovery scan every registered Execution Space. Store reads,
    /// stale-run validation and native-session recovery can take many seconds;
    /// none of them may head-of-line block status/start/runtime control.
    fn serve_loop(self: &Arc<Self>, listener: &UnixListener) -> CliResult<()> {
        const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(20);
        let mut pending = Vec::new();
        let mut control_workers = Vec::new();

        std::thread::scope(|scope| {
            let scanner = scope.spawn(|| -> CliResult<()> {
                while !self.stop_requested.load(Ordering::SeqCst) {
                    self.scan_and_adopt()?;
                    self.reap_finished()?;
                    let next_scan = Instant::now() + self.scan_interval;
                    while !self.stop_requested.load(Ordering::SeqCst) && Instant::now() < next_scan
                    {
                        std::thread::sleep(CONTROL_POLL_INTERVAL);
                    }
                }
                Ok(())
            });

            let authority_heartbeat = scope.spawn(|| -> CliResult<()> {
                // Discovery may spend longer than one lease TTL inspecting
                // unrelated historical Spaces. Keep already-acquired machine
                // authority alive on an independent cadence so a slow scan
                // cannot fence the AgentSessions currently being supervised.
                let interval = node_authority_refresh_interval(self.scan_interval);
                while !self.authority_shutdown.load(Ordering::SeqCst) {
                    self.refresh_held_node_authorities()?;
                    let next_refresh = Instant::now() + interval;
                    while !self.authority_shutdown.load(Ordering::SeqCst)
                        && Instant::now() < next_refresh
                    {
                        std::thread::sleep(CONTROL_POLL_INTERVAL);
                    }
                }
                Ok(())
            });

            while !self.stop_requested.load(Ordering::SeqCst)
                && !scanner.is_finished()
                && !authority_heartbeat.is_finished()
            {
                self.poll_control_socket(listener, &mut pending, &mut control_workers);
                std::thread::sleep(CONTROL_POLL_INTERVAL);
            }

            // A failure in either background responsibility ends the exact
            // daemon generation and lets the normal drain/release path run.
            self.stop_requested.store(true, Ordering::SeqCst);

            // Every accepted mutation owns a response socket and may already
            // have crossed the durable RuntimeCommand prepare boundary. Do
            // not abandon those effects when Stop wins: stop accepting new
            // work, then join every bounded control worker before releasing
            // this daemon generation.
            for worker in control_workers {
                self.observe_control_worker_result(worker.join(), "while draining");
            }
            let control_result = if self.control_worker_failed.load(Ordering::SeqCst) {
                Err(CliError::Usage(
                    "NODE_DAEMON_CONTROL_DRAIN_INCOMPLETE: an accepted control command did not prove completion"
                        .into(),
                ))
            } else {
                Ok(())
            };

            let scan_result = match scanner.join() {
                Ok(result) => result,
                Err(_) => Err(CliError::Usage(
                    "NODE_DAEMON_SCAN_PANICKED: recovery scanner terminated unexpectedly".into(),
                )),
            };
            // The machine generation remains renewed while every managed
            // Supervisor/provider handle converges. Only after that
            // postcondition may Draining fence the generation and the
            // heartbeat stop. This prevents a successor generation from
            // overlapping an accepted mutation that already crossed prepare.
            let supervisor_result = self.graceful_shutdown();
            let drain_result = if supervisor_result.is_ok() {
                self.drain_node_authorities()
            } else {
                Ok(())
            };
            self.authority_shutdown.store(true, Ordering::SeqCst);
            let heartbeat_result = match authority_heartbeat.join() {
                Ok(result) => result,
                Err(_) => Err(CliError::Usage(
                    "NODE_DAEMON_HEARTBEAT_PANICKED: authority heartbeat terminated unexpectedly"
                        .into(),
                )),
            };
            let release_result = if control_result.is_ok()
                && supervisor_result.is_ok()
                && drain_result.is_ok()
                && heartbeat_result.is_ok()
            {
                self.release_node_authorities()
            } else {
                Ok(())
            };
            scan_result
                .and(control_result)
                .and(supervisor_result)
                .and(drain_result)
                .and(heartbeat_result)
                .and(release_result)
        })
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
                match self.team_run_recovery_is_blocking(&space.id, &store, &run.id) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect Supervisor recovery marker for {}/{}: {error}",
                            space.id, run.id
                        );
                        continue;
                    }
                }
                // Close freezes a MemberRun without completing its TeamRun.
                // A Running TeamRun with no Active coordination member is
                // therefore dormant, not orphaned runtime work. Re-adopting
                // it would create an unbounded Supervisor-generation loop;
                // Reopen makes the same row Active again and the next scan (or
                // explicit daemon start request) becomes eligible.
                match team_run_has_active_member(&store, &run.id) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        eprintln!(
                            "[node-daemon] cannot inspect TeamRun {} members in {}: {error}",
                            run.id, space.id
                        );
                        continue;
                    }
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
                            self.block_start_failure_if_unresolved(
                                &space.id, &store, &run.id, &error,
                            );
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
        let _start_guard = self
            .supervisor_start_gate
            .lock()
            .map_err(|error| CliError::Usage(format!("supervisor start gate poisoned: {error}")))?;
        // Provider admissions are scoped to the exact Project Binding and
        // physical Execution Space.  The machine daemon opens stores from the
        // space registry rather than through CLI resolution, so recover that
        // scope from the canonical TeamRun before provider preflight.  Without
        // this, an admission written through the public CLI is invisible to
        // the daemon and every review-required provider is rejected even
        // though the exact tuple was admitted.
        let run_scope = crate::latest_team_run(&store, run_id)?;
        if !team_run_has_active_member(&store, run_id)? {
            return Err(CliError::Usage(format!(
                "TEAM_RUN_DORMANT: TeamRun {run_id} has no Active MemberRun; Reopen a member before runtime adoption"
            )));
        }
        let store = store.with_provider_compatibility_scope(
            run_scope.project_binding_id,
            format!("execution-space:{}", space.id),
        );
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
        let live_provider_activity_endpoint = Arc::clone(&self.live_provider_activity_endpoint);

        // Validate and create registration outside the context lock (fixes P0-7).
        let body = prepare_team_run_start_body(&store, &run_id, max_concurrency)?;
        if body.run.execution_node_id != self.node_id {
            return Err(CliError::Usage(format!(
                "REMOTE_TEAM_RUN_NOT_ADOPTED: TeamRun {run_id} belongs to Node {}, local Node is {}",
                body.run.execution_node_id, self.node_id
            )));
        }
        let project_binding_id = body.run.project_binding_id.clone();
        let daemon_generation = store
            .latest_node_daemon_lease(&self.node_id)?
            .filter(|lease| {
                lease.daemon_id == self.daemon_id && lease.instance_id == self.instance_id
            })
            .ok_or_else(|| {
                CliError::Usage("NODE_DAEMON_GENERATION_FENCED: current lease is missing".into())
            })?
            .generation;
        ensure_team_runtime_fabric(&store, &body, &space.id, &self.daemon_id, daemon_generation)?;
        let registration = TeamSupervisorRegistration::start(&store, &run_id, Some(&space.id))?;
        let supervisor_id = registration.supervisor_id.clone();
        let supervisor_generation = registration.generation;
        bind_team_runtime_supervisor(
            &store,
            &body,
            &space.id,
            &self.daemon_id,
            &registration.supervisor_id,
            registration.generation,
        )?;
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
        let callback_space_id = execution_space_id.clone();
        let thread = std::thread::spawn(move || {
            let live_sink = Arc::new(move |update: LiveProviderActivityUpdate| {
                let endpoint = live_provider_activity_endpoint
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if let Some(endpoint) = endpoint {
                    if let Err(error) =
                        post_live_provider_activity(&endpoint, &callback_space_id, &update)
                    {
                        *live_provider_activity_endpoint
                            .lock()
                            .unwrap_or_else(|lock_error| lock_error.into_inner()) = None;
                        eprintln!(
                            "[node-daemon] live provider activity callback unavailable: {error}"
                        );
                    }
                }
            });
            drive_prepared_team_run(
                prepared,
                Some(space),
                None,
                max_concurrency,
                Duration::from_secs(idle_timeout_secs),
                Some(live_sink),
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
                daemon_generation,
                supervisor_id,
                supervisor_generation,
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

        for mut ctx in finished {
            if let Some(thread) = ctx.thread.take() {
                match thread.join() {
                    Ok(Ok(())) => {
                        self.block_finished_supervisor_if_unresolved(&ctx);
                        eprintln!(
                            "[node-daemon] {}/{} completed",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                    Ok(Err(e)) => {
                        self.block_finished_supervisor_failure(&ctx, &e);
                        eprintln!(
                            "[node-daemon] {}/{} error: {e}",
                            ctx.execution_space_id, ctx.run_id
                        );
                    }
                    Err(_) => {
                        self.block_finished_supervisor_failure(
                            &ctx,
                            &CliError::Usage("TEAM_SUPERVISOR_PANICKED".into()),
                        );
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
}

fn team_run_has_active_member(store: &HarnessStore, run_id: &str) -> CliResult<bool> {
    Ok(crate::latest_member_runs_in_append_order(store)?
        .into_iter()
        .any(|member| {
            member.team_run_id == run_id
                && member.coordination_status == harness_core::MemberCoordinationStatus::Active
        }))
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
#[derive(Debug)]
pub(crate) struct NodeDaemonStartRequestError {
    source: std::io::Error,
    request_may_have_been_accepted: bool,
}

impl NodeDaemonStartRequestError {
    fn before_send(source: std::io::Error) -> Self {
        Self {
            source,
            request_may_have_been_accepted: false,
        }
    }

    fn after_send(source: std::io::Error) -> Self {
        Self {
            source,
            request_may_have_been_accepted: true,
        }
    }

    pub(crate) fn request_may_have_been_accepted(&self) -> bool {
        self.request_may_have_been_accepted
    }
}

impl std::fmt::Display for NodeDaemonStartRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

pub(crate) fn try_delegate_to_node_daemon(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    run_id: &str,
) -> Result<String, NodeDaemonStartRequestError> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream =
        UnixStream::connect(&socket_path).map_err(NodeDaemonStartRequestError::before_send)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(NodeDaemonStartRequestError::before_send)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(NodeDaemonStartRequestError::before_send)?;

    let cmd = serde_json::json!({
        "cmd": "start",
        "execution_space_id": execution_space_id,
        "run_id": run_id
    });
    let cmd_str = serde_json::to_string(&cmd)
        .map_err(std::io::Error::other)
        .map_err(NodeDaemonStartRequestError::before_send)?;
    // From the first write attempt onward, the daemon may have accepted the
    // complete newline-delimited request even when the client observes only a
    // later transport error. Every such result must be reconciled, never
    // classified as NotApplied or blindly retried.
    writeln!(stream, "{cmd_str}").map_err(NodeDaemonStartRequestError::after_send)?;
    stream
        .flush()
        .map_err(NodeDaemonStartRequestError::after_send)?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    reader
        .read_line(&mut buf)
        .map_err(NodeDaemonStartRequestError::after_send)?;
    if buf.trim().is_empty() || !buf.ends_with('\n') {
        return Err(NodeDaemonStartRequestError::after_send(
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "NodeDaemon closed before returning a start result",
            ),
        ));
    }
    serde_json::from_str::<serde_json::Value>(buf.trim()).map_err(|error| {
        NodeDaemonStartRequestError::after_send(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("NodeDaemon returned invalid start JSON: {error}"),
        ))
    })?;
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

fn post_live_provider_activity(
    endpoint: &LiveProviderActivityEndpoint,
    execution_space_id: &str,
    update: &LiveProviderActivityUpdate,
) -> Result<(), std::io::Error> {
    let mut stream = std::net::TcpStream::connect(&endpoint.authority)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let body = serde_json::to_vec(update).map_err(std::io::Error::other)?;
    let encoded_space = execution_space_id.replace('%', "%25").replace(' ', "%20");
    write!(
        stream,
        "POST /v1/live/provider-activity?space={encoded_space} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nX-AgentFirm-Live-Token: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.authority,
        endpoint.token,
        body.len(),
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    let mut status_line = String::new();
    std::io::BufReader::new(&mut stream).read_line(&mut status_line)?;
    if !status_line.contains(" 202 ") {
        return Err(std::io::Error::other(format!(
            "serve rejected live provider activity: {}",
            status_line.trim()
        )));
    }
    Ok(())
}

/// Register the current `serve` process as the volatile live-activity sink.
/// A missing daemon is not an error: serve remains usable and a later restart
/// registers again. The endpoint is loopback-only and never durable.
pub(crate) fn register_live_provider_activity_via_socket(
    firm_home: &Path,
    node_id: &str,
    authority: &str,
    token: &str,
) -> Option<Result<String, std::io::Error>> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = match UnixStream::connect(&socket_path) {
        Ok(stream) => stream,
        Err(_) => return None,
    };
    if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        return Some(Err(error));
    }
    if let Err(error) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        return Some(Err(error));
    }
    let command = serde_json::json!({
        "cmd": "register_live_provider_activity",
        "authority": authority,
        "token": token,
    });
    if let Err(error) = writeln!(stream, "{command}") {
        return Some(Err(error));
    }
    if let Err(error) = stream.flush() {
        return Some(Err(error));
    }
    let mut response = String::new();
    if let Err(error) = std::io::BufReader::new(&mut stream).read_line(&mut response) {
        return Some(Err(error));
    }
    Some(Ok(response.trim().to_string()))
}

/// Send an authenticated runtime command to the one local NodeDaemon. The
/// caller receives only the daemon's fenced result; it never mutates provider
/// or session ledgers directly.
pub(crate) fn runtime_command_via_socket(
    firm_home: &Path,
    node_id: &str,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
) -> Result<serde_json::Value, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let command = serde_json::json!({"cmd": "runtime", "envelope": envelope});
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&command).map_err(std::io::Error::other)?
    )?;
    stream.flush()?;
    let mut line = String::new();
    std::io::BufReader::new(&mut stream).read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "NodeDaemon returned an empty runtime response",
        ));
    }
    serde_json::from_str(line.trim()).map_err(std::io::Error::other)
}

/// Start a NodeDaemon for an exact observed predecessor generation. Unlike the
/// convenience CLI helper, this never treats an already-running or concurrently
/// winning daemon as the requested external effect. The child PID in the
/// server-owned status response must be the process spawned by this request.
pub(crate) fn start_daemon_process_fenced(
    firm_home: &Path,
    node_id: &str,
    max_concurrency: usize,
    execution_space_id: &str,
    observed_generation: u64,
) -> CliResult<String> {
    if max_concurrency == 0 {
        return Err(CliError::Usage(
            "daemon max_concurrency must be greater than zero".into(),
        ));
    }
    if daemon_status_via_socket(firm_home, node_id).is_some() {
        return Err(CliError::Usage(
            "SUPERVISOR_GENERATION_FENCED: a NodeDaemon is already live".into(),
        ));
    }
    let space = crate::execution_space::context_for_id(firm_home, execution_space_id)
        .map_err(|error| CliError::Usage(error.to_string()))?
        .ok_or_else(|| {
            CliError::Usage(format!("Execution Space not found: {execution_space_id}"))
        })?;
    let store = HarnessStore::new(space.store_root);
    let current_generation = store
        .latest_node_daemon_lease(node_id)?
        .map(|lease| lease.generation)
        .unwrap_or(0);
    if current_generation != observed_generation {
        return Err(CliError::Usage(format!(
            "SUPERVISOR_GENERATION_FENCED: observed generation {observed_generation}, current generation {current_generation}"
        )));
    }
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("daemon")
        .arg("serve")
        .arg("--max-concurrency")
        .arg(max_concurrency.to_string())
        .arg("--idle-timeout-secs")
        .arg("300")
        .arg("--scan-interval-secs")
        .arg("5")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn()?;
    // Recovery/adoption can legitimately take longer than a trivial socket
    // bind. A failed start must not leave a child that later acquires the
    // NodeDaemon generation behind the caller's back.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = daemon_status_via_socket(firm_home, node_id) {
            let status_value = serde_json::from_str::<serde_json::Value>(&status).ok();
            let process_id = status_value
                .as_ref()
                .and_then(|value| value["process_id"].as_u64());
            if process_id == Some(u64::from(child.id())) {
                let instance_id = status_value
                    .as_ref()
                    .and_then(|value| value["instance_id"].as_str());
                let lease = store.latest_node_daemon_lease(node_id)?;
                if lease.as_ref().is_some_and(|lease| {
                    lease.daemon_id == format!("node-daemon:{node_id}")
                        && Some(lease.instance_id.as_str()) == instance_id
                        && lease.generation > observed_generation
                        && lease.status == harness_core::NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > current_unix_ms_u64()
                }) {
                    return Ok(status);
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            return Err(CliError::Usage(
                "SUPERVISOR_GENERATION_FENCED: another NodeDaemon generation won startup".into(),
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Err(CliError::Usage(format!(
                "NodeDaemon pid {} exited before acquiring generation: {status}",
                child.id()
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CliError::Usage(format!(
                "NodeDaemon pid {} did not become ready within 60s and was stopped",
                child.id()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Send a stop command to the multi-team daemon.
pub(crate) fn daemon_stop_via_socket(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    daemon_generation: u64,
) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let cmd = serde_json::json!({
        "cmd": "stop",
        "execution_space_id": execution_space_id,
        "daemon_generation": daemon_generation,
    });
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
mod tests;
