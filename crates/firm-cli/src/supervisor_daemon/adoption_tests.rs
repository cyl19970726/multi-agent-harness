//! Regressions for the durable, canonical-state-keyed adoption outcome
//! (#704, #671) and for keeping the control lane answerable while a finished
//! Supervisor is reaped (#671).

use super::team_supervision::ADOPTION_START_ATTEMPTS;
use super::tests::TestTree;
use super::*;

pub(super) struct AdoptionFixture {
    _tree: TestTree,
    pub(super) execution_space_id: String,
    pub(super) store: HarnessStore,
    pub(super) run_id: String,
    pub(super) daemon: MultiTeamDaemon,
}

/// The unit-test AgentTeam fixture that `create_team_run` bootstraps binds
/// its canonical AgentMembers to this Execution Space id.
const ADOPTION_SPACE_ID: &str = "unit-test-space";

pub(super) fn adoption_fixture(label: &str) -> AdoptionFixture {
    let tree = TestTree::new(label);
    let firm_home = tree.0.join("home");
    // Register the Space the unit-test AgentTeam fixture binds to, so the reap
    // path's `store_for_space` resolves this exact Store.
    let space = crate::execution_space::register_and_activate(
        &firm_home,
        ADOPTION_SPACE_ID,
        "Adoption Space",
        Some("unit-test-project".to_string()),
        None,
        "unix-ms:1",
    )
    .expect("register adoption Execution Space");
    let store = HarnessStore::new(space.store_root.clone());
    store.init().expect("initialize adoption Store");

    let member = |agent_member_id: &str, name: &str, role: &str| crate::TeamMemberSpec {
        agent_member_id: agent_member_id.into(),
        name: name.into(),
        role: role.into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: None,
        initial_work: None,
    };
    let created = crate::create_team_run(
        &store,
        None,
        None,
        None,
        "Hold adoption without canonical progress",
        None,
        "test",
        None,
        harness_core::HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            member("agent-builder-a", "BuilderA", "module_a"),
            member("host", "Host", "host"),
        ],
    )
    .expect("create adoption TeamRun");

    let daemon = MultiTeamDaemon {
        firm_home,
        node_id: "11111111-1111-4111-8111-111111111119".into(),
        daemon_id: "node-daemon:11111111-1111-4111-8111-111111111119".into(),
        instance_id: "adoption-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        machine_authority_loss: Mutex::new(None),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        capacity_waits: Mutex::new(HashMap::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };

    AdoptionFixture {
        _tree: tree,
        execution_space_id: ADOPTION_SPACE_ID.to_string(),
        store,
        run_id: created.team_run.id,
        daemon,
    }
}

impl AdoptionFixture {
    pub(super) fn adoption_is_held(&self) -> bool {
        self.daemon
            .team_run_adoption_is_held(&self.execution_space_id, &self.store, &self.run_id)
            .expect("read adoption hold")
    }

    pub(super) fn canonical_state(&self) -> String {
        crate::team_run_canonical_state_fingerprint(
            &self.store,
            Some(&self.execution_space_id),
            &self.run_id,
        )
        .expect("fingerprint canonical state")
    }

    /// One real canonical change: the TeamRun lifecycle transition a Host or
    /// Supervisor performs. Nothing about the durable marker is touched.
    fn advance_team_run_status(&self, next: harness_core::TeamRunStatus) {
        let current = crate::latest_team_run(&self.store, &self.run_id).expect("read TeamRun");
        let mut advanced = current.clone();
        advanced.status = next;
        advanced.updated_at = crate::now_string();
        self.store
            .compare_and_append_team_run_lifecycle(&current, &advanced)
            .expect("advance TeamRun status");
    }
}

#[test]
fn unchanged_team_run_is_adopted_at_most_once_per_canonical_state() {
    let fixture = adoption_fixture("adoption-hold");
    assert!(
        !fixture.adoption_is_held(),
        "a run with no durable adoption outcome is adoptable"
    );
    let first_state = fixture.canonical_state();

    fixture.daemon.hold_adoption_without_progress(
        &fixture.execution_space_id,
        &fixture.store,
        &fixture.run_id,
        "member supervisor stopped with team run still running and no canonical change",
        Some(&first_state),
    );
    assert!(
        fixture.adoption_is_held(),
        "an adoption that produced no canonical progress must not be repeated at the same state"
    );

    // Writing the same outcome again is idempotent: the daemon may observe the
    // identical state many times and must not grow the journal or change the
    // hold.
    fixture.daemon.hold_adoption_without_progress(
        &fixture.execution_space_id,
        &fixture.store,
        &fixture.run_id,
        "second identical observation",
        Some(&first_state),
    );
    let no_progress_markers = fixture
        .store
        .member_actions()
        .expect("read member actions")
        .into_iter()
        .filter(|action| {
            action.team_run_id == fixture.run_id
                && action.action_type == "team_supervisor_no_progress"
        })
        .count();
    assert_eq!(
        no_progress_markers, 1,
        "the same canonical state is recorded exactly once"
    );

    // A canonical change is the only thing needed to make the run adoptable
    // again — no operator action at all.
    fixture.advance_team_run_status(harness_core::TeamRunStatus::Running);
    assert_ne!(
        fixture.canonical_state(),
        first_state,
        "a TeamRun lifecycle transition is a canonical change"
    );
    assert!(
        !fixture.adoption_is_held(),
        "a canonical change re-enables automatic adoption"
    );

    // The new state may hold in its own right, and an explicit recovery or
    // start intent clears it.
    let second_state = fixture.canonical_state();
    fixture.daemon.hold_adoption_without_progress(
        &fixture.execution_space_id,
        &fixture.store,
        &fixture.run_id,
        "no canonical progress at the new state either",
        Some(&second_state),
    );
    assert!(fixture.adoption_is_held());
    fixture
        .daemon
        .clear_team_run_supervisor_recovery(
            &fixture.execution_space_id,
            &fixture.store,
            &fixture.run_id,
            "explicit-supervisor",
            9,
        )
        .expect("explicit start settles the hold");
    assert!(
        !fixture.adoption_is_held(),
        "an explicit operator recovery or Host start intent clears the hold"
    );
}

#[test]
fn structurally_dead_start_failure_is_not_retried_on_every_scan() {
    let fixture = adoption_fixture("adoption-start-failure");
    assert!(!fixture.adoption_is_held());

    // No RuntimeCommand exists — this start never reached one. Before the fix
    // nothing durable was written and the next scan adopted the same run
    // again, advancing a Supervisor generation each time (#671).
    fixture.daemon.block_start_failure_if_unresolved(
        &fixture.execution_space_id,
        &fixture.store,
        &fixture.run_id,
        &CliError::Usage(format!(
            "team run {} is pinned to unavailable Project Binding gone",
            fixture.run_id
        )),
    );
    assert!(
        fixture.adoption_is_held(),
        "a structural start failure must leave a durable, diagnosed adoption hold"
    );
    assert!(
        fixture
            .store
            .member_actions()
            .expect("read member actions")
            .into_iter()
            .any(|action| action.team_run_id == fixture.run_id
                && action.action_type == "team_supervisor_no_progress"
                && action
                    .summary
                    .contains("TEAM_SUPERVISOR_START_FAILED_BEFORE_RUNTIME_COMMAND")),
        "the hold names why the start could not proceed"
    );

    // The hold is still only canonical-state deep: real progress re-enables it.
    fixture.advance_team_run_status(harness_core::TeamRunStatus::Running);
    assert!(!fixture.adoption_is_held());
}

#[test]
fn a_transient_start_failure_never_holds_adoption() {
    let fixture = adoption_fixture("adoption-transient-failure");
    fixture.daemon.block_start_failure_if_unresolved(
        &fixture.execution_space_id,
        &fixture.store,
        &fixture.run_id,
        &CliError::Usage("NodeDaemon at capacity (1/1 runs); cannot start space/run".into()),
    );
    assert!(
        !fixture.adoption_is_held(),
        "capacity is a property of this daemon generation, not of the TeamRun"
    );
    assert!(
        fixture
            .store
            .member_actions()
            .expect("read member actions")
            .into_iter()
            .all(|action| action.action_type != "team_supervisor_no_progress"),
        "a transient start failure writes no durable adoption outcome"
    );
}

#[test]
fn status_remains_responsive_while_reap_joins_a_finished_supervisor() {
    use std::ffi::CString;
    use std::fs::OpenOptions;
    use std::io::BufReader;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let tree = TestTree::new("reap-control");
    let firm_home = tree.0.join("home");
    let registry_path = crate::execution_space::registry_path(&firm_home);
    std::fs::create_dir_all(
        registry_path
            .parent()
            .expect("Execution Space registry has a parent"),
    )
    .expect("create Execution Space registry directory");
    let fifo_path = CString::new(registry_path.as_os_str().as_bytes())
        .expect("registry test path has no interior NUL");
    // SAFETY: `fifo_path` is a live, NUL-terminated path and the mode contains
    // only filesystem permission bits.
    let mkfifo_result = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
    assert_eq!(
        mkfifo_result,
        0,
        "create blocking registry FIFO: {}",
        std::io::Error::last_os_error()
    );

    // One already-finished Supervisor. Reaping it resolves its durable outcome
    // through `store_for_space`, which reads the Execution Space registry —
    // the FIFO makes that read block for as long as the test needs.
    let finished = std::thread::spawn(|| -> CliResult<TeamRunDriveOutcome> {
        Ok(TeamRunDriveOutcome::NoProgress {
            canonical_state: "sha256:unchanged".into(),
            detail: "test supervisor produced no canonical progress".into(),
        })
    });
    while !finished.is_finished() {
        std::thread::yield_now();
    }

    let daemon = MultiTeamDaemon {
        firm_home,
        node_id: "reap-node".into(),
        daemon_id: "node-daemon:reap-node".into(),
        instance_id: "reap-instance".into(),
        contexts: Mutex::new(vec![MultiTeamContext {
            execution_space_id: "reap-space".into(),
            project_binding_id: "reap-project".into(),
            run_id: "reap-run".into(),
            daemon_generation: 1,
            supervisor_id: "reap-supervisor".into(),
            supervisor_generation: 1,
            heartbeat_valid: Arc::new(AtomicBool::new(false)),
            serving_status: Arc::new(Mutex::new("running".into())),
            thread: Some(finished),
            started_at: Instant::now(),
        }]),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(60),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        machine_authority_loss: Mutex::new(None),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        capacity_waits: Mutex::new(HashMap::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };

    std::thread::scope(|scope| {
        let reaper = scope.spawn(|| daemon.reap_finished());

        // Opening the FIFO writer nonblocking succeeds only once the reaper is
        // waiting inside its blocking registry read, which is exactly the
        // window that used to be covered by the `contexts` lock.
        let deadline = Instant::now() + Duration::from_secs(5);
        let registry_writer = loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(file) => break file,
                Err(error)
                    if error.raw_os_error() == Some(libc::ENXIO) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("wait for reaper to open registry FIFO: {error}"),
            }
        };

        let status_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (mut server, mut client) = UnixStream::pair().expect("create control socket pair");
            server
                .set_nonblocking(true)
                .expect("model an accepted nonblocking daemon socket");
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("bound status response wait");
            daemon
                .handle_control_command(&mut server, "{\"cmd\":\"status\"}")
                .expect("status answers while reap is blocked on a Store scan");
            let mut response = String::new();
            BufReader::new(&mut client)
                .read_line(&mut response)
                .expect("status returns while the reap Store scan is still blocked");
            let response: serde_json::Value =
                serde_json::from_str(response.trim()).expect("status is complete JSON");
            assert_eq!(response["ok"], true);
            assert_eq!(response["node_id"], "reap-node");
            assert_eq!(
                response["runs"].as_array().map(Vec::len),
                Some(0),
                "the reaped context left the registry before the Store scan started"
            );
        }));

        // Release the blocked reader on every path so a failed assertion does
        // not turn into a hung test process.
        drop(registry_writer);
        let release_deadline = Instant::now() + Duration::from_secs(5);
        while !reaper.is_finished() && Instant::now() < release_deadline {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&registry_path)
            {
                Ok(mut writer) => {
                    if let Err(error) = writer.write_all(b"\n") {
                        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("release late registry FIFO reader: {error}"),
            }
        }
        if let Err(panic) = status_result {
            std::panic::resume_unwind(panic);
        }
        reaper.join().expect("reaper thread").expect("reap result");
    });
}

/// The NodeDaemon must not retry an adoption it already proved impossible.
///
/// At `--max-concurrency` the start path still decodes the whole TeamRun and
/// MemberRun ledgers and renews the machine lease — two store write-lock
/// acquisitions — before it reaches the capacity check. Retrying that on every
/// discovery pass is what produced the observed "failed to adopt ... at
/// capacity (2/2 runs)" storm and helped starve the daemon's own Supervisor
/// heartbeat off the write lock until it lost machine authority and
/// self-stopped (#836).
#[test]
fn at_capacity_adoption_is_attempted_once_per_scan_tick_not_once_per_pass() {
    const PASSES: usize = 6;

    let mut fixture = adoption_fixture("adoption-at-capacity");
    // The unit-test AgentTeam fixture places its run on this canonical Node,
    // already enrolled and project-registered in the adoption Store.
    fixture.daemon.node_id = "00000000-0000-4000-8000-000000000001".to_string();
    fixture.daemon.daemon_id = format!("node-daemon:{}", fixture.daemon.node_id);
    // Long enough that every pass below falls inside one scan tick.
    fixture.daemon.scan_interval = Duration::from_secs(600);
    fixture.advance_team_run_status(harness_core::TeamRunStatus::Running);

    // Fill the daemon's one concurrency slot with an unrelated managed run.
    fixture
        .daemon
        .contexts
        .lock()
        .expect("lock managed contexts")
        .push(MultiTeamContext {
            execution_space_id: fixture.execution_space_id.clone(),
            project_binding_id: "unit-test-project".to_string(),
            run_id: "team-run-occupying-the-only-slot".to_string(),
            daemon_generation: 1,
            supervisor_id: "occupying-supervisor".to_string(),
            supervisor_generation: 1,
            heartbeat_valid: Arc::new(AtomicBool::new(true)),
            serving_status: Arc::new(Mutex::new("running".to_string())),
            thread: None,
            started_at: Instant::now(),
        });

    let before = ADOPTION_START_ATTEMPTS.load(Ordering::Relaxed);
    for pass in 0..PASSES {
        fixture
            .daemon
            .scan_and_adopt()
            .unwrap_or_else(|error| panic!("discovery pass {pass} failed: {error}"));
    }
    assert_eq!(
        ADOPTION_START_ATTEMPTS.load(Ordering::Relaxed) - before,
        1,
        "{PASSES} passes inside one scan tick must produce one adoption attempt, not one per pass"
    );

    // The refusal is recorded exactly once and is what `daemon status` reports
    // as `waiting_for_capacity`.
    let waits = fixture
        .daemon
        .capacity_waits
        .lock()
        .expect("lock capacity waits");
    let wait = waits
        .get(&(fixture.execution_space_id.clone(), fixture.run_id.clone()))
        .expect("the deferred run is recorded as waiting for capacity");
    assert_eq!(waits.len(), 1);
    assert_eq!(wait.occupancy, 1);
    assert!(
        wait.detail.starts_with(AT_CAPACITY_REFUSAL),
        "the record carries the refusal verbatim: {}",
        wait.detail
    );
    drop(waits);
    assert!(
        fixture
            .daemon
            .adoption_defers_for_capacity(&fixture.execution_space_id, &fixture.run_id),
        "the deferral holds while the daemon stays at capacity"
    );

    // A freed slot ends the deferral immediately, long before the scan
    // interval elapses. The retry itself is left to the daemon's own loop:
    // adopting here would start a real provider runtime.
    fixture
        .daemon
        .contexts
        .lock()
        .expect("lock managed contexts")
        .clear();
    assert!(
        !fixture
            .daemon
            .adoption_defers_for_capacity(&fixture.execution_space_id, &fixture.run_id),
        "a capacity change re-enables adoption without waiting out the scan interval"
    );
}
