//! Machine-scoped NodeDaemon.
//!
//! One daemon owns the local execution-node lease and supervises every TeamRun
//! admitted from the node's registered Execution Spaces. TeamRun supervisors
//! are children of that daemon generation; they are not independently
//! discoverable or startable daemons.

use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
#[cfg(test)]
use std::io::BufRead;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::daemon_application::DaemonApplication;
use crate::daemon_application_port::DaemonApplicationPort;
use crate::daemon_application_port::TeamRunDriveOutcome;
use crate::daemon_error::{DaemonError as CliError, DaemonResult as CliResult};
use crate::daemon_protocol::NativeSessionWakeUpdate;
use crate::daemon_support::current_unix_ms_u64;
use harness_store::HarnessStore;

// ---------------------------------------------------------------------------
// Signal handling (portable Unix FFI)
// ---------------------------------------------------------------------------

mod control_protocol;
#[cfg(test)]
mod lease_renewal_tests;
mod machine_authority;
mod recovery;
mod self_stop_events;
mod shutdown;
mod team_supervision;
#[cfg(test)]
use crate::daemon_client::*;
#[cfg(test)]
use machine_authority::node_authority_refresh_interval;
use machine_authority::{daemon_control_generation_authorized, AuthorityReleaseReport};
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
    let firm_home = crate::daemon_support::canonicalize_best_effort(firm_home);
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

use crate::daemon_protocol::NativeSessionWakeEndpoint;

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
pub(crate) use crate::daemon_protocol::AT_CAPACITY_REFUSAL;

/// One TeamRun the discovery scan deferred because the daemon is already at
/// `--max-concurrency` (#836).
///
/// Retrying that adoption on every pass is not free: the start path decodes
/// the whole TeamRun and MemberRun ledgers before it reaches the capacity
/// check. A run that provably cannot start must not repeatedly consume that
/// work. Held machine leases are renewed separately from discovery.
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
    application: Arc<dyn DaemonApplicationPort>,
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
    session_runtimes:
        Mutex<HashMap<String, Box<dyn crate::daemon_application_port::NodeSessionHandle>>>,
    /// Volatile callback registered by the current local `serve` process. It
    /// is never written to the Store and a daemon restart deliberately loses
    /// it. The bearer token is required on every loopback ingress request.
    native_session_wake_endpoint: Arc<Mutex<HashMap<String, NativeSessionWakeEndpoint>>>,
    max_concurrency: usize,
    /// The provider input-acceptance boundary in seconds (delivery boundary:
    /// input written -> the provider's exact acceptance receipt), never a
    /// silence or wall-clock limit on a running cycle.
    input_acceptance_secs: u64,
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
    // Last successful leases bound retry time only; Store fences own all drives.
    confirmed_node_leases: Mutex<HashMap<String, (HarnessStore, harness_core::NodeDaemonLease)>>,
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
        input_acceptance_secs: u64,
        scan_interval_secs: u64,
        application: Arc<dyn DaemonApplicationPort>,
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
                    Self::ensure_stale_socket_reclaimable(&firm_home, &node_id, &*application)?;
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
            application,
            native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
            max_concurrency,
            input_acceptance_secs,
            scan_interval: Duration::from_secs(scan_interval_secs),
            stop_requested: shutdown_sig,
            authority_shutdown: Arc::new(AtomicBool::new(false)),
            authority_lost: AtomicBool::new(false),
            machine_authority_loss: Mutex::new(None),
            confirmed_node_leases: Mutex::new(HashMap::new()),
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
                while !self.authority_shutdown.load(Ordering::SeqCst) {
                    self.refresh_held_node_authorities()?;
                    let next_refresh = Instant::now() + self.next_node_authority_refresh_delay();
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
mod recover_blocked_lane_blocker_tests;
#[cfg(test)]
mod recover_lost_execution_tests;
#[cfg(test)]
mod stop_drain_tests;
#[cfg(test)]
mod tests;
