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
    CliResult, HarnessStore, NativeSessionWakeUpdate, PreparedTeamRunStart, TeamRunDriveOutcome,
    TeamRunLedger, TeamSupervisorRegistration,
};

// ---------------------------------------------------------------------------
// Signal handling (portable Unix FFI)
// ---------------------------------------------------------------------------

mod control_protocol;
mod machine_authority;
pub(crate) mod recovery;
mod self_stop_events;
mod shutdown;
mod team_supervision;
use machine_authority::{
    daemon_control_generation_authorized, node_authority_refresh_interval, AuthorityReleaseReport,
};
pub(crate) use recovery::reconcile_team_run_start_postcondition;
use self_stop_events::MachineAuthorityLoss;

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
pub(crate) const CONTROL_TRANSIENT_READ_RETRIES: usize = 2;
pub(crate) const CONTROL_TRANSIENT_READ_BACKOFF: Duration = Duration::from_millis(25);
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
    /// Process-local display projection updated by the owning Supervisor.
    /// Keeping it beside the context lets `daemon status` stay Store-free.
    serving_status: Arc<Mutex<String>>,
    thread: Option<std::thread::JoinHandle<CliResult<TeamRunDriveOutcome>>>,
    started_at: Instant,
}

struct PendingControlConnection {
    stream: UnixStream,
    bytes: Vec<u8>,
    accepted_at: Instant,
}

#[derive(Clone)]
struct NativeSessionWakeEndpoint {
    authority: String,
    token: String,
    serve_instance_id: String,
}

/// How long Stop waits for already-accepted control mutations to converge.
const CONTROL_WORKER_DRAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// How long Stop then waits for the recovery scanner's current pass to finish.
const SCANNER_DRAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// How long Stop waits for managed Supervisor threads to converge before it
/// escalates to owned-process-group termination.
const SUPERVISOR_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// How long Stop then waits for those threads to observe the termination.
const FORCED_PROCESS_GROUP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// Documented upper bound a `stop` caller must outwait before the daemon can
/// answer truthfully. Stop replies with the drain result, never before it
/// (#584), so the client read timeout has to exceed this bound.
///
/// It covers every phase Stop actually waits on, in the order `serve_loop`
/// runs them. Deriving it from the Supervisor drain alone understated it by
/// the two unbounded joins that precede that drain, and a caller that timed
/// out was then told no daemon was running while this one was still draining.
pub(crate) const NODE_DAEMON_STOP_DRAIN_BOUND: Duration = Duration::from_secs(
    CONTROL_WORKER_DRAIN_TIMEOUT.as_secs()
        + SCANNER_DRAIN_TIMEOUT.as_secs()
        + SUPERVISOR_DRAIN_TIMEOUT.as_secs()
        + FORCED_PROCESS_GROUP_DRAIN_TIMEOUT.as_secs(),
);

/// The exact refusal `start_supervising` writes when this NodeDaemon already
/// drives `--max-concurrency` TeamRuns. Named once so the adoption classifier
/// and the at-capacity backoff cannot drift from the message.
const AT_CAPACITY_REFUSAL: &str = "NodeDaemon at capacity";

/// One TeamRun the discovery scan deferred because the daemon is already at
/// `--max-concurrency` (#836).
///
/// Retrying that adoption on every pass is not free: the start path decodes
/// the whole TeamRun and MemberRun ledgers and renews the machine lease —
/// two store write-lock acquisitions — before it ever reaches the capacity
/// check. A run that provably cannot start therefore competed with the
/// daemon's own Supervisor heartbeat for that lock, and three missed renewals
/// cost the daemon its machine authority.
#[derive(Debug, Clone)]
struct CapacityWait {
    /// Managed-run count observed when the refusal was recorded. Any change
    /// may have opened a slot, so it ends the deferral immediately.
    occupancy: usize,
    /// Earliest retry: one scan interval after the refusal.
    not_before: Instant,
    /// When this waiting episode started, for `daemon status`.
    since: Instant,
    /// The refusal itself, reported verbatim rather than re-derived.
    detail: String,
}

/// A process-local adoption hold, used only when the durable marker could not
/// be written. It is deliberately liftable wherever possible: an unliftable
/// hold would strand a run for this daemon's whole lifetime, including legacy
/// runs with no Host MemberRun to project a marker onto.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VolatileAdoptionHold {
    /// The canonical state could not be read at all, so nothing can prove the
    /// run has since changed. Only an explicit start intent clears this.
    Unconditional,
    /// The durable write failed but the observed canonical state is known.
    /// Behaves exactly like the durable no-progress marker.
    AtCanonicalState(String),
}

/// One accepted `stop` whose truthful answer is still being determined.
struct DeferredStopResponse {
    stream: UnixStream,
    daemon_generation: u64,
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
    native_session_wake_endpoint: Arc<Mutex<HashMap<String, NativeSessionWakeEndpoint>>>,
    max_concurrency: usize,
    idle_timeout_secs: u64,
    scan_interval: Duration,
    /// Stops control acceptance and discovery, but deliberately does not stop
    /// authority renewal while already-accepted mutations are draining.
    stop_requested: Arc<AtomicBool>,
    /// Ends the NodeDaemon lease heartbeat only after accepted workers and
    /// managed supervisors have converged.
    authority_shutdown: Arc<AtomicBool>,
    /// Sticky process-local fence. Once any required Execution Space loses
    /// this instance's exact lease, no Space may admit another provider
    /// effect and this process may only drain/settle its predecessor bundle.
    authority_lost: AtomicBool,
    /// First machine-authority failure plus the TeamRuns served when it was
    /// latched. Shutdown drains `contexts`, so this snapshot keeps the
    /// Host-visible self-stop journal complete through the final phase.
    machine_authority_loss: Mutex<Option<MachineAuthorityLoss>>,
    /// Latches an accepted worker that panicked or returned without proving
    /// command completion. Such a generation may drain but never Release.
    control_worker_failed: AtomicBool,
    /// Volatile companion to the durable recovery MemberAction. It closes the
    /// rescan window even when the recovery projection itself cannot be
    /// written because the Execution Space is temporarily unavailable.
    recovery_blocked_runs: Mutex<HashMap<(String, String), VolatileAdoptionHold>>,
    /// Runs whose finished Supervisor outcome is being reconciled outside the
    /// `contexts` lock. An explicit Start must not adopt one of these: the
    /// dead generation is still deciding what durable marker to write, and
    /// that marker would land on the live successor.
    settling_runs: Mutex<HashSet<(String, String)>>,
    /// TeamRuns deferred at `--max-concurrency`, keyed by (Execution Space,
    /// TeamRun). Recorded once per waiting episode, retried no earlier than
    /// the next scan interval or a change in managed-run count, and surfaced
    /// by `daemon status` as `waiting_for_capacity` (#836).
    capacity_waits: Mutex<HashMap<(String, String), CapacityWait>>,
    /// Accepted `stop` client sockets held open until the drain result is
    /// known. Stop is answered from that result rather than from acceptance,
    /// so a caller can never read `ok:true` while this process still spins.
    deferred_stop_responses: Mutex<Vec<DeferredStopResponse>>,
    #[cfg(test)]
    lease_ttl_override_ms: Option<u64>,
    /// Bounded (cooperative, forced) drain deadlines in milliseconds. Tests
    /// use it to exercise the honest Stop answer without waiting the full
    /// production bound.
    #[cfg(test)]
    drain_timeout_override_ms: Option<(u64, u64)>,
}

impl MultiTeamDaemon {
    fn install_native_session_wake_endpoint(
        &self,
        authority: &str,
        token: &str,
        agent_member_id: &str,
        expected_daemon_instance_id: &str,
        serve_instance_id: &str,
    ) -> bool {
        let loopback = authority
            .parse::<std::net::SocketAddr>()
            .ok()
            .is_some_and(|address| address.ip().is_loopback());
        if !loopback
            || token.len() < 32
            || token.len() > 256
            || serve_instance_id.len() < 32
            || serve_instance_id.len() > 256
            || expected_daemon_instance_id != self.instance_id
            || agent_member_id.trim().is_empty()
        {
            return false;
        }
        self.native_session_wake_endpoint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                agent_member_id.to_string(),
                NativeSessionWakeEndpoint {
                    authority: authority.to_string(),
                    token: token.to_string(),
                    serve_instance_id: serve_instance_id.to_string(),
                },
            );
        true
    }

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
            native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
            max_concurrency,
            idle_timeout_secs,
            scan_interval: Duration::from_secs(scan_interval_secs),
            stop_requested: shutdown_sig,
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            authority_lost: AtomicBool::new(false),
            machine_authority_loss: Mutex::new(None),
            control_worker_failed: AtomicBool::new(false),
            recovery_blocked_runs: Mutex::new(HashMap::new()),
            settling_runs: Mutex::new(HashSet::new()),
            capacity_waits: Mutex::new(HashMap::new()),
            deferred_stop_responses: Mutex::new(Vec::new()),
            #[cfg(test)]
            lease_ttl_override_ms: None,
            #[cfg(test)]
            drain_timeout_override_ms: None,
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

            // Known limit: control ingress stops here, so `daemon status` gets
            // no answer for the rest of the drain. The drain is bounded by
            // NODE_DAEMON_STOP_DRAIN_BOUND and the in-flight `stop` caller is
            // answered with the real result, so the window is documented
            // rather than papered over with a stale "running" reply. Serving a
            // `draining` status would need the socket poll moved onto its own
            // thread; that is a separate change to the ingress model.

            // Every accepted mutation owns a response socket and may already
            // have crossed the durable RuntimeCommand prepare boundary. Do
            // not abandon those effects when Stop wins: stop accepting new
            // work, then join every bounded control worker before releasing
            // this daemon generation.
            //
            // The *wait* is bounded even though the join is not. Stop answers
            // from what actually drained, so an unbounded wait here would make
            // the answer arrive after the caller's read timeout and report "no
            // daemon is running" about a daemon that is still draining (#584).
            // Overrunning either bound is itself a drain-incomplete result:
            // authority is retained and the threads are still joined before
            // this generation returns.
            let control_drained = wait_for(self.control_worker_drain_timeout(), || {
                control_workers.iter().all(|worker| worker.is_finished())
            });
            let scanner_drained = wait_for(self.scanner_drain_timeout(), || scanner.is_finished());
            // Answer Stop before the unbounded joins when either phase
            // overran. The joins still happen — no accepted effect is
            // abandoned — but the operator learns the truth inside the bound
            // instead of timing out and being told no daemon is running.
            if !control_drained || !scanner_drained {
                let phase = if !control_drained {
                    "control_workers"
                } else {
                    "recovery_scanner"
                };
                // Release has not run at this point, and the gate below can
                // no longer reach it, so `false` here is an observation rather
                // than the prediction this message used to make.
                self.answer_deferred_stop_responses(
                    Some(&StopDrainFailure {
                        phase,
                        error: CliError::Usage(format!(
                            "NODE_DAEMON_DRAIN_INCOMPLETE: {phase} did not converge within the documented stop bound; authority release is deferred until the joins complete"
                        )),
                    }),
                    false,
                    &AuthorityReleaseReport::default(),
                );
            }

            for worker in control_workers {
                self.observe_control_worker_result(worker.join(), "while draining");
            }
            let control_result = if !control_drained {
                Err(CliError::Usage(
                    "NODE_DAEMON_DRAIN_INCOMPLETE: accepted control commands did not converge within the documented stop bound"
                        .into(),
                ))
            } else if self.control_worker_failed.load(Ordering::SeqCst) {
                Err(CliError::Usage(
                    "NODE_DAEMON_CONTROL_DRAIN_INCOMPLETE: an accepted control command did not prove completion"
                        .into(),
                ))
            } else {
                Ok(())
            };

            let scan_result = match scanner.join() {
                Ok(result) if !scanner_drained => result.and(Err(CliError::Usage(
                    "NODE_DAEMON_DRAIN_INCOMPLETE: the recovery scanner did not converge within the documented stop bound"
                        .into(),
                ))),
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
            let settlement_result = if supervisor_result.is_ok() {
                self.settle_node_authorities_for_shutdown()
            } else {
                Ok(())
            };
            let drain_result = if supervisor_result.is_ok() && settlement_result.is_ok() {
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
            // The scanner belongs in this gate. It is the phase that proves
            // every registered Execution Space is still readable under this
            // exact generation; releasing machine authority after it failed
            // would both silently drop the fence and make the stop receipt
            // contradict itself (DEV-149-REVIEW-02).
            let release_attempted = control_result.is_ok()
                && scan_result.is_ok()
                && supervisor_result.is_ok()
                && settlement_result.is_ok()
                && drain_result.is_ok()
                && heartbeat_result.is_ok();
            let (release_result, release_report) = if release_attempted {
                self.release_node_authorities()
            } else {
                (Ok(()), AuthorityReleaseReport::default())
            };
            // Observed, never predicted: authority is wholly released only when
            // the release actually ran and every registered Execution Space
            // lease actually came back Released. A partial release reports
            // false here and names the Spaces in the receipt.
            let authority_released = release_attempted && release_result.is_ok();
            // Every accepted `stop` is answered from what the drain actually
            // proved. Authority release is part of that answer: a caller must
            // never see `ok:true` while this process still owns runtimes, and
            // the failing phase is named so an operator knows what to inspect.
            let failure = [
                ("control_workers", control_result.as_ref().err()),
                ("recovery_scanner", scan_result.as_ref().err()),
                ("supervisor_drain", supervisor_result.as_ref().err()),
                ("authority_settlement", settlement_result.as_ref().err()),
                ("authority_drain", drain_result.as_ref().err()),
                ("authority_heartbeat", heartbeat_result.as_ref().err()),
                ("authority_release", release_result.as_ref().err()),
            ]
            .into_iter()
            .find_map(|(phase, error)| {
                error.map(|error| StopDrainFailure {
                    phase,
                    error: CliError::Usage(error.to_string()),
                })
            });
            self.answer_deferred_stop_responses(
                failure.as_ref(),
                authority_released,
                &release_report,
            );

            scan_result
                .and(control_result)
                .and(supervisor_result)
                .and(settlement_result)
                .and(drain_result)
                .and(heartbeat_result)
                .and(release_result)
        })
    }

    /// Deliver the truthful Stop answer to every caller still waiting on it.
    fn answer_deferred_stop_responses(
        &self,
        drain_failure: Option<&StopDrainFailure>,
        authority_released: bool,
        release_report: &AuthorityReleaseReport,
    ) {
        let deferred = {
            let mut guard = self
                .deferred_stop_responses
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *guard)
        };
        for mut pending in deferred {
            let response = match drain_failure {
                None => serde_json::json!({
                    "ok": true,
                    "daemon_generation": pending.daemon_generation,
                    "drained": true,
                    "authority_released": authority_released,
                    "released_execution_space_ids": release_report.released_space_ids,
                    "release_failed_execution_space_ids": release_report.failed_space_ids,
                }),
                // `authority_released: false` means "not wholly released".
                // Release continues past a per-Space failure, so the two lists
                // are the only truthful account of what happened.
                Some(failure) => serde_json::json!({
                    "ok": false,
                    "daemon_generation": pending.daemon_generation,
                    "drained": false,
                    "authority_released": authority_released,
                    "released_execution_space_ids": release_report.released_space_ids,
                    "release_failed_execution_space_ids": release_report.failed_space_ids,
                    "failed_phase": failure.phase,
                    "error": failure.error.to_string(),
                }),
            };
            if let Err(error) = Self::write_control_response(&mut pending.stream, &response) {
                eprintln!("[node-daemon] could not deliver the stop drain result: {error}");
            }
        }
    }

    fn control_worker_drain_timeout(&self) -> Duration {
        #[cfg(test)]
        if let Some((cooperative_ms, _)) = self.drain_timeout_override_ms {
            return Duration::from_millis(cooperative_ms);
        }
        CONTROL_WORKER_DRAIN_TIMEOUT
    }

    fn scanner_drain_timeout(&self) -> Duration {
        #[cfg(test)]
        if let Some((cooperative_ms, _)) = self.drain_timeout_override_ms {
            return Duration::from_millis(cooperative_ms);
        }
        SCANNER_DRAIN_TIMEOUT
    }
}

/// Which drain phase denied a clean Stop, and why.
struct StopDrainFailure {
    phase: &'static str,
    error: CliError,
}

/// Poll `ready` until it holds or `timeout` elapses. Returns whether it held.
fn wait_for(timeout: Duration, ready: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
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
    read_control_response_line(
        &mut reader,
        &mut buf,
        CONTROL_TRANSIENT_READ_RETRIES,
        CONTROL_TRANSIENT_READ_BACKOFF,
    )
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

#[derive(Debug)]
enum NativeSessionWakePostError {
    Unavailable(std::io::Error),
    Rejected(String),
}

impl NativeSessionWakePostError {
    fn clears_registered_endpoint(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl std::fmt::Display for NativeSessionWakePostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "serve callback unavailable: {error}"),
            Self::Rejected(status) => {
                write!(formatter, "serve rejected exact live scope: {status}")
            }
        }
    }
}

impl From<std::io::Error> for NativeSessionWakePostError {
    fn from(error: std::io::Error) -> Self {
        Self::Unavailable(error)
    }
}

fn post_native_session_wake(
    endpoint: &NativeSessionWakeEndpoint,
    execution_space_id: &str,
    update: &NativeSessionWakeUpdate,
) -> Result<(), NativeSessionWakePostError> {
    let mut stream = std::net::TcpStream::connect(&endpoint.authority)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let body = serde_json::to_vec(update).map_err(std::io::Error::other)?;
    let encoded_space = execution_space_id.replace('%', "%25").replace(' ', "%20");
    write!(
        stream,
        "POST /v1/live/native-session-wake?space={encoded_space} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nX-AgentFirm-Native-Session-Wake-Token: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.authority,
        endpoint.token,
        body.len(),
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    let mut status_line = String::new();
    std::io::BufReader::new(&mut stream).read_line(&mut status_line)?;
    if !status_line.contains(" 202 ") {
        return Err(NativeSessionWakePostError::Rejected(
            status_line.trim().to_string(),
        ));
    }
    Ok(())
}

/// Register the current `serve` process as the volatile live-activity sink.
/// A missing daemon is not an error: serve remains usable and a later restart
/// registers again. The endpoint is loopback-only and never durable.
pub(crate) struct NativeSessionWakeRegistration<'a> {
    pub authority: &'a str,
    pub token: &'a str,
    pub agent_member_id: &'a str,
    pub expected_daemon_instance_id: &'a str,
    pub serve_instance_id: &'a str,
}

pub(crate) fn register_native_session_wake_via_socket(
    firm_home: &Path,
    node_id: &str,
    registration: NativeSessionWakeRegistration<'_>,
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
        "cmd": "register_native_session_wake",
        "authority": registration.authority,
        "token": registration.token,
        "agent_member_id": registration.agent_member_id,
        "expected_daemon_instance_id": registration.expected_daemon_instance_id,
        "serve_instance_id": registration.serve_instance_id,
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
    runtime_command_via_socket_with_policy(
        firm_home,
        node_id,
        envelope,
        ControlSocketReadPolicy {
            timeout: Duration::from_secs(10),
            transient_retries: CONTROL_TRANSIENT_READ_RETRIES,
            retry_backoff: CONTROL_TRANSIENT_READ_BACKOFF,
        },
    )
}

#[derive(Clone, Copy)]
struct ControlSocketReadPolicy {
    timeout: Duration,
    transient_retries: usize,
    retry_backoff: Duration,
}

fn runtime_command_via_socket_with_policy(
    firm_home: &Path,
    node_id: &str,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
    read_policy: ControlSocketReadPolicy,
) -> Result<serde_json::Value, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(read_policy.timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let command = serde_json::json!({"cmd": "runtime", "envelope": envelope});
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&command).map_err(std::io::Error::other)?
    )?;
    stream.flush()?;
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    read_control_response_line(
        &mut reader,
        &mut line,
        read_policy.transient_retries,
        read_policy.retry_backoff,
    )?;
    if line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "NodeDaemon returned an empty runtime response",
        ));
    }
    serde_json::from_str(line.trim()).map_err(std::io::Error::other)
}

pub(crate) fn read_control_response_line(
    reader: &mut impl BufRead,
    line: &mut String,
    max_transient_retries: usize,
    retry_backoff: Duration,
) -> Result<(), std::io::Error> {
    // A read timeout is post-send and the daemon may still be applying the
    // command. Keep waiting on this connection; reconnecting or rewriting the
    // frame would turn a transport delay into an ambiguous duplicate effect.
    let mut transient_retries = 0;
    loop {
        match reader.read_line(line) {
            Ok(_) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && transient_retries < max_transient_retries =>
            {
                transient_retries += 1;
                std::thread::sleep(retry_backoff);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Read one bounded provider-owned Session page through the exact local
/// NodeDaemon. The response never contains a filesystem path and the daemon
/// revalidates placement, lease generation, viewer scope and native identity.
pub(crate) fn native_session_read_via_socket(
    firm_home: &Path,
    node_id: &str,
    request: &crate::provider_event_api::PersistedSessionReadRequest,
) -> Result<crate::provider_event_api::PersistedSessionReadResponse, std::io::Error> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let command = serde_json::json!({"cmd": "read_native_session", "request": request});
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
            "NodeDaemon returned an empty persisted Session response",
        ));
    }
    let envelope: serde_json::Value =
        serde_json::from_str(line.trim()).map_err(std::io::Error::other)?;
    if envelope["ok"] != true {
        return Err(std::io::Error::other(
            envelope["error"]
                .as_str()
                .unwrap_or("NodeDaemon rejected persisted Session read"),
        ));
    }
    serde_json::from_value(envelope["response"].clone()).map_err(std::io::Error::other)
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
    let (log_path, stdout, stderr) =
        crate::daemon_cli::node_daemon_log_streams(firm_home, node_id)?;
    let executable = std::env::current_exe().map_err(|error| {
        CliError::Usage(format!(
            "cannot resolve NodeDaemon executable (log: {}): {error}",
            log_path.display()
        ))
    })?;
    let mut command = std::process::Command::new(executable);
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
        .stdout(stdout)
        .stderr(stderr);
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| {
        CliError::Usage(format!(
            "cannot start NodeDaemon (log: {}): {error}",
            log_path.display()
        ))
    })?;
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
                    return crate::daemon_cli::daemon_status_with_log_path(&status, &log_path);
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            return Err(CliError::Usage(
                "SUPERVISOR_GENERATION_FENCED: another NodeDaemon generation won startup".into(),
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Err(crate::daemon_cli::daemon_start_failure(
                child.id(),
                &format!("exited before acquiring generation ({status})"),
                &log_path,
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::daemon_cli::daemon_start_failure(
                child.id(),
                "did not become ready within 60s and was stopped",
                &log_path,
            ));
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
    daemon_stop_via_socket_with_policy(
        firm_home,
        node_id,
        execution_space_id,
        daemon_generation,
        ControlSocketReadPolicy {
            timeout: NODE_DAEMON_STOP_DRAIN_BOUND.saturating_add(Duration::from_secs(25)),
            transient_retries: CONTROL_TRANSIENT_READ_RETRIES,
            retry_backoff: CONTROL_TRANSIENT_READ_BACKOFF,
        },
    )
}

fn daemon_stop_via_socket_with_policy(
    firm_home: &Path,
    node_id: &str,
    execution_space_id: &str,
    daemon_generation: u64,
    read_policy: ControlSocketReadPolicy,
) -> Option<String> {
    let socket_path = node_daemon_socket_path(firm_home, node_id);
    let mut stream = UnixStream::connect(&socket_path).ok()?;
    // Stop answers with the drain result, not with its acceptance, so the
    // client must outwait the documented drain bound plus the bounded
    // settle/release writes that follow it (#584).
    stream.set_read_timeout(Some(read_policy.timeout)).ok()?;

    let cmd = serde_json::json!({
        "cmd": "stop",
        "execution_space_id": execution_space_id,
        "daemon_generation": daemon_generation,
    });
    writeln!(stream, "{cmd}").ok()?;
    stream.flush().ok()?;

    let mut buf = String::new();
    let mut reader = std::io::BufReader::new(&mut stream);
    read_control_response_line(
        &mut reader,
        &mut buf,
        read_policy.transient_retries,
        read_policy.retry_backoff,
    )
    .ok()?;
    Some(buf.trim().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod adoption_tests;
#[cfg(test)]
mod drain_blocked_member_tests;
#[cfg(test)]
mod drain_inflight_work_tests;
#[cfg(test)]
mod drain_recovery_tests;
#[cfg(test)]
mod drive_outcome_tests;
#[cfg(test)]
mod stop_drain_tests;
#[cfg(test)]
mod tests;
