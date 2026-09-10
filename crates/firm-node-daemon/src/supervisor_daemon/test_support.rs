//! Default-disabled fixtures that call the same private production daemon.
//! No registry, lock guard or Deref escapes this module.
use super::*;

pub struct TestContextConfig {
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub run_id: String,
    pub daemon_generation: u64,
    pub supervisor_id: String,
    pub supervisor_generation: u64,
    pub heartbeat_valid: Arc<AtomicBool>,
    /// Process-local display projection updated by the owning Supervisor.
    /// Keeping it beside the context lets `daemon status` stay Store-free.
    pub serving_status: Arc<Mutex<String>>,
    pub thread: Option<std::thread::JoinHandle<CliResult<TeamRunDriveOutcome>>>,
    pub started_at: Instant,
}
pub struct OwnedTestContext {
    inner: MultiTeamContext,
}
impl OwnedTestContext {
    pub fn new(config: TestContextConfig) -> Self {
        Self {
            inner: MultiTeamContext {
                execution_space_id: config.execution_space_id,
                project_binding_id: config.project_binding_id,
                run_id: config.run_id,
                daemon_generation: config.daemon_generation,
                supervisor_id: config.supervisor_id,
                supervisor_generation: config.supervisor_generation,
                heartbeat_valid: config.heartbeat_valid,
                serving_status: config.serving_status,
                thread: config.thread,
                started_at: config.started_at,
            },
        }
    }
}
pub struct TestDaemonConfig {
    pub firm_home: PathBuf,
    pub node_id: String,
    pub daemon_id: String,
    pub instance_id: String,
    pub contexts: Vec<OwnedTestContext>,
    pub application: Arc<dyn DaemonApplicationPort>,
    pub max_concurrency: usize,
    pub input_acceptance_secs: u64,
    pub scan_interval: Duration,
    pub stop_requested: Arc<AtomicBool>,
    pub authority_shutdown: Arc<AtomicBool>,
    pub lease_ttl_override_ms: Option<u64>,
    pub drain_timeout_override_ms: Option<(u64, u64)>,
}
pub struct TestDaemon {
    inner: Arc<MultiTeamDaemon>,
}
pub struct TestAuthorityReleaseReport {
    pub released_space_ids: Vec<String>,
    pub failed_space_ids: Vec<String>,
}
pub struct CapacityWaitSnapshot {
    pub occupancy: usize,
    pub detail: String,
}
impl TestDaemon {
    pub fn new(config: TestDaemonConfig) -> Self {
        Self {
            inner: Arc::new(MultiTeamDaemon {
                firm_home: config.firm_home,
                node_id: config.node_id,
                daemon_id: config.daemon_id,
                instance_id: config.instance_id,
                contexts: Mutex::new(config.contexts.into_iter().map(|ctx| ctx.inner).collect()),
                application: config.application,
                max_concurrency: config.max_concurrency,
                input_acceptance_secs: config.input_acceptance_secs,
                scan_interval: config.scan_interval,
                stop_requested: config.stop_requested,
                authority_shutdown: config.authority_shutdown,
                lease_ttl_override_ms: config.lease_ttl_override_ms,
                drain_timeout_override_ms: config.drain_timeout_override_ms,
                supervisor_start_gate: Mutex::new(()),
                session_runtimes: Mutex::new(HashMap::new()),
                native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
                authority_lost: AtomicBool::new(false),
                machine_authority_loss: Mutex::new(None),
                confirmed_node_leases: Mutex::new(HashMap::new()),
                control_worker_failed: AtomicBool::new(false),
                recovery_blocked_runs: Mutex::new(HashMap::new()),
                settling_runs: Mutex::new(HashSet::new()),
                capacity_waits: Mutex::new(HashMap::new()),
                deferred_stop_responses: Mutex::new(Vec::new()),
            }),
        }
    }
    pub fn successor(self, instance_id: String) -> Self {
        let mut daemon = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("fixture daemon must have one owner"));
        daemon.authority_lost = AtomicBool::new(false);
        daemon.machine_authority_loss = Mutex::new(None);
        daemon.confirmed_node_leases = Mutex::new(HashMap::new());
        daemon.stop_requested = Arc::new(AtomicBool::new(false));
        daemon.instance_id = instance_id;
        Self {
            inner: Arc::new(daemon),
        }
    }
    pub fn set_node_identity(&mut self, node_id: String) {
        let daemon = Arc::get_mut(&mut self.inner).expect("unique fixture owner");
        daemon.node_id = node_id;
        daemon.daemon_id = format!("node-daemon:{}", daemon.node_id);
    }
    pub fn node_id(&self) -> &str {
        &self.inner.node_id
    }
    pub fn daemon_id(&self) -> &str {
        &self.inner.daemon_id
    }
    pub fn instance_id(&self) -> &str {
        &self.inner.instance_id
    }
    pub fn firm_home(&self) -> &Path {
        &self.inner.firm_home
    }
    pub fn set_scan_interval(&mut self, value: Duration) {
        Arc::get_mut(&mut self.inner)
            .expect("unique fixture owner")
            .scan_interval = value;
    }
    pub fn set_lease_ttl_override(&mut self, value: Option<u64>) {
        Arc::get_mut(&mut self.inner)
            .expect("unique fixture owner")
            .lease_ttl_override_ms = value;
    }
    pub fn authority_lost(&self) -> bool {
        self.inner.authority_lost.load(Ordering::SeqCst)
    }
    pub fn control_worker_failed(&self) -> bool {
        self.inner.control_worker_failed.load(Ordering::SeqCst)
    }
    pub fn stop_requested_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.stop_requested)
    }
    pub fn authority_shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.authority_shutdown)
    }
    pub fn push_context(&self, context: OwnedTestContext) {
        self.inner
            .contexts
            .lock()
            .expect("lock managed contexts")
            .push(context.inner);
    }
    pub fn clear_contexts(&self) {
        self.inner
            .contexts
            .lock()
            .expect("lock managed contexts")
            .clear();
    }
    pub fn context_count(&self) -> usize {
        self.inner
            .contexts
            .lock()
            .expect("lock managed contexts")
            .len()
    }
    pub fn context_thread_finished(&self, index: usize) -> bool {
        self.inner.contexts.lock().unwrap()[index]
            .thread
            .as_ref()
            .unwrap()
            .is_finished()
    }
    pub fn wake_endpoint_count(&self) -> usize {
        self.inner
            .native_session_wake_endpoint
            .lock()
            .expect("live sink registry")
            .len()
    }
    pub fn wake_endpoint(&self, member: &str) -> Option<NativeSessionWakeEndpoint> {
        self.inner
            .native_session_wake_endpoint
            .lock()
            .expect("live sink registry")
            .get(member)
            .cloned()
    }
    pub fn capacity_wait_count(&self) -> usize {
        self.inner
            .capacity_waits
            .lock()
            .expect("lock capacity waits")
            .len()
    }
    pub fn capacity_wait(&self, space: &str, run: &str) -> Option<CapacityWaitSnapshot> {
        self.inner
            .capacity_waits
            .lock()
            .expect("lock capacity waits")
            .get(&(space.into(), run.into()))
            .map(|wait| CapacityWaitSnapshot {
                occupancy: wait.occupancy,
                detail: wait.detail.clone(),
            })
    }
    pub fn insert_volatile_hold(&self, key: (String, String), fingerprint: Option<String>) {
        self.inner
            .recovery_blocked_runs
            .lock()
            .expect("volatile hold registry")
            .insert(
                key,
                fingerprint
                    .map(VolatileAdoptionHold::AtCanonicalState)
                    .unwrap_or(VolatileAdoptionHold::Unconditional),
            );
    }
    pub fn insert_settling_run(&self, key: (String, String)) {
        self.inner
            .settling_runs
            .lock()
            .expect("settling registry")
            .insert(key);
    }
    pub fn remove_settling_run(&self, key: &(String, String)) {
        self.inner
            .settling_runs
            .lock()
            .expect("settling registry")
            .remove(key);
    }
    pub fn scan_and_adopt(&self) -> CliResult<()> {
        self.inner.scan_and_adopt()
    }
    pub fn reap_finished(&self) -> CliResult<()> {
        self.inner.reap_finished()
    }
    pub fn settle_finished_supervisor(&self, ctx: &OwnedTestContext, outcome: TeamRunDriveOutcome) {
        self.inner.settle_finished_supervisor(&ctx.inner, outcome)
    }
    pub fn block_finished_supervisor_failure(&self, ctx: &OwnedTestContext, error: &CliError) {
        self.inner
            .block_finished_supervisor_failure(&ctx.inner, error)
    }
    pub fn serve_loop(self: &Arc<Self>, listener: &UnixListener) -> CliResult<()> {
        self.inner.serve_loop(listener)
    }
    pub fn handle_control_command(&self, stream: &mut UnixStream, cmd_line: &str) -> CliResult<()> {
        self.inner.handle_control_command(stream, cmd_line)
    }
    pub fn ensure_node_authority_bundle(&self) -> CliResult<HashSet<String>> {
        self.inner.ensure_node_authority_bundle()
    }
    pub fn run_held_node_authorities(&self) -> CliResult<()> {
        self.inner.run_held_node_authorities()
    }
    pub fn refresh_held_node_authorities(&self) -> CliResult<()> {
        self.inner.refresh_held_node_authorities()
    }
    pub fn remember_node_lease(
        &self,
        space: &str,
        store: &HarnessStore,
        lease: &harness_core::NodeDaemonLease,
    ) {
        self.inner.remember_node_lease(space, store, lease)
    }
    pub fn ensure_node_authority(
        &self,
        space: &harness_core::ExecutionSpace,
        store: &HarnessStore,
    ) -> CliResult<harness_core::NodeDaemonLease> {
        self.inner.ensure_node_authority(space, store)
    }
    pub fn registered_spaces(
        &self,
    ) -> CliResult<Vec<(harness_core::ExecutionSpace, HarnessStore)>> {
        self.inner.registered_spaces()
    }
    pub fn release_node_authorities(&self) -> (CliResult<()>, TestAuthorityReleaseReport) {
        let (result, report) = self.inner.release_node_authorities();
        (
            result,
            TestAuthorityReleaseReport {
                released_space_ids: report.released_space_ids,
                failed_space_ids: report.failed_space_ids,
            },
        )
    }
    pub fn settle_node_authorities_for_shutdown(&self) -> CliResult<()> {
        self.inner.settle_node_authorities_for_shutdown()
    }
    pub fn install_native_session_wake_endpoint(
        &self,
        authority: &str,
        token: &str,
        agent_member_id: &str,
        expected_daemon_instance_id: &str,
        serve_instance_id: &str,
    ) -> bool {
        self.inner.install_native_session_wake_endpoint(
            authority,
            token,
            agent_member_id,
            expected_daemon_instance_id,
            serve_instance_id,
        )
    }
    pub fn block_start_failure_if_unresolved(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        error: &CliError,
    ) {
        self.inner
            .block_start_failure_if_unresolved(execution_space_id, store, run_id, error)
    }
    pub fn clear_team_run_supervisor_recovery(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
    ) -> CliResult<()> {
        self.inner.clear_team_run_supervisor_recovery(
            execution_space_id,
            store,
            run_id,
            supervisor_id,
            supervisor_generation,
        )
    }
    pub fn hold_adoption_without_progress(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
        detail: &str,
        observed_state: Option<&str>,
    ) {
        self.inner.hold_adoption_without_progress(
            execution_space_id,
            store,
            run_id,
            detail,
            observed_state,
        )
    }
    pub fn team_run_adoption_is_held(
        &self,
        execution_space_id: &str,
        store: &HarnessStore,
        run_id: &str,
    ) -> CliResult<bool> {
        self.inner
            .team_run_adoption_is_held(execution_space_id, store, run_id)
    }
    pub fn adoption_defers_for_capacity(&self, execution_space_id: &str, run_id: &str) -> bool {
        self.inner
            .adoption_defers_for_capacity(execution_space_id, run_id)
    }
    pub fn next_node_authority_refresh_delay(&self) -> Duration {
        self.inner.next_node_authority_refresh_delay()
    }
    pub fn graceful_shutdown_with_deadline(&self, cooperative_timeout: Duration) -> CliResult<()> {
        self.inner
            .graceful_shutdown_with_deadline(cooperative_timeout)
    }
    pub fn supersede_node_authority_for_test(&self, store: &HarnessStore) -> CliResult<()> {
        self.inner.supersede_node_authority_for_test(store)
    }
    pub fn write_control_response(
        stream: &mut UnixStream,
        response: &serde_json::Value,
    ) -> CliResult<()> {
        MultiTeamDaemon::write_control_response(stream, response)
    }
    pub fn graceful_shutdown_with_deadlines(
        &self,
        cooperative_timeout: Duration,
        forced_timeout: Duration,
    ) -> CliResult<()> {
        self.inner
            .graceful_shutdown_with_deadlines(cooperative_timeout, forced_timeout)
    }
    pub fn journal_machine_authority_loss_phase(
        &self,
        phase: &str,
        terminated_provider_process_groups: &[u32],
    ) {
        self.inner
            .journal_machine_authority_loss_phase(phase, terminated_provider_process_groups)
    }
}
pub fn adoption_start_attempts() -> u64 {
    team_supervision::ADOPTION_START_ATTEMPTS.load(Ordering::Relaxed)
}
pub fn node_authority_refresh_interval(scan_interval: Duration) -> Duration {
    machine_authority::node_authority_refresh_interval(scan_interval)
}
pub fn daemon_control_generation_authorized(
    lease: Option<&harness_core::NodeDaemonLease>,
    daemon_id: &str,
    instance_id: &str,
    generation: u64,
    now_ms: u64,
) -> bool {
    machine_authority::daemon_control_generation_authorized(
        lease,
        daemon_id,
        instance_id,
        generation,
        now_ms,
    )
}
