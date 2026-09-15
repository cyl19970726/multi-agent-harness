use super::adoption_tests::adoption_fixture;
use super::*;
use std::sync::Mutex;

#[test]
fn authority_renewal_failure_is_returned_by_the_team_run_events_reader() {
    let fixture = adoption_fixture("self-stop-event");
    let daemon = &fixture.daemon;
    fixture
        .store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: daemon.node_id().to_string(),
            display_name: "Self-stop test Node".to_string(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        })
        .expect("insert self-stop test Node");
    fixture
        .store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".to_string(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            },
            &fixture.execution_space_id,
        )
        .expect("register self-stop test project");
    let space = daemon
        .registered_spaces()
        .expect("list self-stop test Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.execution_space_id).then_some(space))
        .expect("self-stop test Space");
    let owned_lease = daemon
        .ensure_node_authority(&space, &fixture.store)
        .expect("acquire owned daemon lease");
    daemon.push_context(OwnedTestContext::new(TestContextConfig {
        execution_space_id: fixture.execution_space_id.clone(),
        project_binding_id: "unit-test-project".to_string(),
        run_id: fixture.run_id.clone(),
        daemon_generation: owned_lease.generation,
        supervisor_id: "self-stop-supervisor".to_string(),
        supervisor_generation: 1,
        heartbeat_valid: Arc::new(AtomicBool::new(true)),
        thread: None,
        started_at: Instant::now(),
        serving_status: Arc::new(Mutex::new("running".to_string())),
    }));

    daemon
        .supersede_node_authority_for_test(&fixture.store)
        .expect("replace lease with a successor that fences renewal");

    let error = daemon
        .refresh_held_node_authorities()
        .expect_err("heartbeat renewal must lose exact machine authority");
    assert!(error
        .to_string()
        .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));
    daemon.journal_machine_authority_loss_phase("process_groups_terminated", &[4242]);

    let events = fixture
        .store
        .current_team_run_events(&fixture.run_id)
        .expect("read TeamRun events after self-stop");
    let self_stop = events
        .iter()
        .filter(|event| {
            event.source_kind == harness_core::TeamRunEventSourceKind::Service
                && event.entity_type == "node_daemon"
                && event.operation == "self_stopped"
        })
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(&event.summary)
                .expect("self-stop summary is structured JSON")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        self_stop.len(),
        4,
        "renewal, lease-loss, cooperative-interrupt, and process-group phases"
    );
    assert_eq!(self_stop[0]["kind"], "node_daemon_self_stop");
    assert_eq!(self_stop[0]["reason"], "NODE_DAEMON_MACHINE_AUTHORITY_LOST");
    assert_eq!(self_stop[0]["phase"], "renewal_failed");
    assert_eq!(
        self_stop[0]["detail"],
        serde_json::Value::Null,
        "phases without extra evidence keep their exact summary shape"
    );
    assert_eq!(self_stop[1]["phase"], "lease_lost");
    assert_eq!(self_stop[1]["daemon_id"], daemon.daemon_id());
    assert_eq!(self_stop[1]["daemon_instance_id"], daemon.instance_id());
    assert_eq!(self_stop[1]["daemon_generation"], owned_lease.generation);
    assert!(self_stop[1]["error"]
        .as_str()
        .is_some_and(|error| error.contains("exact Node authority moved")));
    assert_eq!(
        self_stop[1]["terminated_provider_process_groups"],
        serde_json::json!([])
    );
    // The cooperative interrupt precedes the drain, so its phase is journalled
    // before any process group is terminated. This fixture serves no live
    // provider turn, so the honest count is zero rather than an absent phase.
    assert_eq!(self_stop[2]["phase"], "cooperative_interrupt_dispatched");
    assert_eq!(self_stop[2]["detail"]["turns_live"], 0);
    assert_eq!(self_stop[2]["detail"]["turns_interrupted"], 0);
    assert_eq!(self_stop[2]["detail"]["scope"], "process");
    assert_eq!(self_stop[3]["phase"], "process_groups_terminated");
    assert_eq!(
        self_stop[3]["terminated_provider_process_groups"],
        serde_json::json!([4242])
    );
}

/// ADR 0074: losing machine authority hands every live provider turn exactly
/// one cooperative interrupt through the adapter's own interrupt path, before
/// the drain's cooperative wait and its SIGKILL backstop. The self-stop
/// journal carries the count and the per-turn outcome. No RuntimeCommand is
/// prepared or settled on this path — admission is already closed.
#[test]
fn machine_authority_loss_hands_each_live_turn_one_cooperative_interrupt() {
    // This latch is Process-scoped, so it reaches every live turn in this test
    // binary. CI is --test-threads=1; a local `cargo test` is not.
    let _serialized = crate::live_turn_serialization::serialized_live_provider_turns();
    const INTERRUPTED_MEMBER_RUN_ID: &str = "member-run-cooperative-interrupt";
    let fixture = adoption_fixture("self-stop-cooperative-interrupt");
    let daemon = &fixture.daemon;
    fixture
        .store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: daemon.node_id().to_string(),
            display_name: "Cooperative interrupt Node".to_string(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        })
        .expect("insert cooperative-interrupt Node");
    fixture
        .store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".to_string(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            },
            &fixture.execution_space_id,
        )
        .expect("register cooperative-interrupt project");
    let space = daemon
        .registered_spaces()
        .expect("list cooperative-interrupt Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.execution_space_id).then_some(space))
        .expect("cooperative-interrupt Space");
    let owned_lease = daemon
        .ensure_node_authority(&space, &fixture.store)
        .expect("acquire owned daemon lease");
    daemon.push_context(OwnedTestContext::new(TestContextConfig {
        execution_space_id: fixture.execution_space_id.clone(),
        project_binding_id: "unit-test-project".to_string(),
        run_id: fixture.run_id.clone(),
        daemon_generation: owned_lease.generation,
        supervisor_id: "cooperative-interrupt-supervisor".to_string(),
        supervisor_generation: 1,
        heartbeat_valid: Arc::new(AtomicBool::new(true)),
        thread: None,
        started_at: Instant::now(),
        serving_status: Arc::new(Mutex::new("running".to_string())),
    }));

    // One live provider turn, polling control on its own supervisor thread
    // exactly as `run_team_member_with_adapter` drives a real cycle.
    let registered = Arc::new(AtomicBool::new(false));
    let interrupted = Arc::new(Mutex::new(None::<String>));
    let thread_registered = Arc::clone(&registered);
    let thread_interrupted = Arc::clone(&interrupted);
    let thread_run_id = fixture.run_id.clone();
    let driver = std::thread::spawn(move || {
        let turn = harness_runtime_host::register_live_provider_turn(
            "kimi",
            &thread_run_id,
            INTERRUPTED_MEMBER_RUN_ID,
        );
        thread_registered.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(reason) = turn.take_authority_loss_interrupt() {
                *thread_interrupted
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(reason);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    while !registered.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }

    daemon
        .supersede_node_authority_for_test(&fixture.store)
        .expect("replace lease with a successor that fences renewal");
    let error = daemon
        .refresh_held_node_authorities()
        .expect_err("heartbeat renewal must lose exact machine authority");
    assert!(error
        .to_string()
        .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));
    driver.join().expect("live turn driver thread");

    let reason = interrupted
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .expect("the live turn received its cooperative interrupt");
    assert!(
        reason.contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "interrupt reason names the authority loss: {reason}"
    );

    let dispatched = fixture
        .store
        .current_team_run_events(&fixture.run_id)
        .expect("read TeamRun events after self-stop")
        .iter()
        .filter(|event| event.entity_type == "node_daemon" && event.operation == "self_stopped")
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(&event.summary)
                .expect("self-stop summary is structured JSON")
        })
        .find(|summary| summary["phase"] == "cooperative_interrupt_dispatched")
        .expect("the cooperative interrupt is journalled as its own self-stop phase");
    assert_eq!(dispatched["detail"]["scope"], "process");
    assert!(
        dispatched["detail"]["turns_interrupted"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "journal counts the interrupted turns: {dispatched}"
    );
    let turn = dispatched["detail"]["turns"]
        .as_array()
        .expect("per-turn outcomes")
        .iter()
        .find(|turn| turn["member_run_id"] == INTERRUPTED_MEMBER_RUN_ID)
        .expect("the interrupted member run is named in the journalled evidence");
    assert_eq!(turn["provider"], "kimi");
    assert_eq!(turn["outcome"], "dispatched");
}

/// ADR 0074, machine path. Closing provider-effect admission stops turns that
/// have not reached their admission check, and the fan-out reaches turns
/// already registered — but a turn that passed admission and registers *after*
/// the fan-out is covered by neither. `latch_machine_authority_loss` writes
/// only NodeDaemon lease rows and process-wide flags, and
/// `TeamRunLedger::require_supervisor_lease` reads neither, so the latch must
/// invalidate the served Supervisor scopes itself or that turn drives on under
/// authority that is already gone.
///
/// The durable TeamSupervisorLease row is deliberately left untouched here, so
/// the process-local flip is the only thing that can refuse.
#[test]
fn machine_authority_loss_quiesces_every_served_supervisor_scope() {
    let _serialized = crate::live_turn_serialization::serialized_live_provider_turns();
    let fixture = adoption_fixture("self-stop-quiesce-scope");
    let daemon = &fixture.daemon;
    fixture
        .store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: daemon.node_id().to_string(),
            display_name: "Quiesce scope Node".to_string(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        })
        .expect("insert quiesce-scope Node");
    fixture
        .store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".to_string(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            },
            &fixture.execution_space_id,
        )
        .expect("register quiesce-scope project");
    let space = daemon
        .registered_spaces()
        .expect("list quiesce-scope Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.execution_space_id).then_some(space))
        .expect("quiesce-scope Space");
    let owned_lease = daemon
        .ensure_node_authority(&space, &fixture.store)
        .expect("acquire owned daemon lease");

    // This is the exact `Arc` the owning TeamRunLedger reads as
    // `supervisor_valid` (`daemon_application.rs:104-110`).
    let supervisor_valid = Arc::new(AtomicBool::new(true));
    daemon.push_context(OwnedTestContext::new(TestContextConfig {
        execution_space_id: fixture.execution_space_id.clone(),
        project_binding_id: "unit-test-project".to_string(),
        run_id: fixture.run_id.clone(),
        daemon_generation: owned_lease.generation,
        supervisor_id: "quiesce-scope-supervisor".to_string(),
        supervisor_generation: 1,
        heartbeat_valid: Arc::clone(&supervisor_valid),
        thread: None,
        started_at: Instant::now(),
        serving_status: Arc::new(Mutex::new("running".to_string())),
    }));
    assert!(
        supervisor_valid.load(Ordering::Acquire),
        "the served scope starts valid"
    );

    daemon
        .supersede_node_authority_for_test(&fixture.store)
        .expect("replace lease with a successor that fences renewal");
    let error = daemon
        .refresh_held_node_authorities()
        .expect_err("heartbeat renewal must lose exact machine authority");
    assert!(error
        .to_string()
        .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"));

    assert!(
        !supervisor_valid.load(Ordering::Acquire),
        "machine authority loss must invalidate every served Supervisor scope, \
         or a turn registering after the fan-out still passes \
         acquire_prepared_cycle_turn and drives"
    );

    // Why the flip and not the registry: the registry deliberately forgets a
    // latched scope, so a turn registered now carries no request. Refusing it
    // is the quiesced scope's job.
    let late = harness_runtime_host::register_live_provider_turn(
        "kimi",
        &fixture.run_id,
        "member-run-after-the-fan-out",
    );
    assert_eq!(
        late.take_authority_loss_interrupt(),
        None,
        "the registry is not sticky; the quiesced Supervisor scope is the cover"
    );

    let dispatched = fixture
        .store
        .current_team_run_events(&fixture.run_id)
        .expect("read TeamRun events after self-stop")
        .iter()
        .filter(|event| event.entity_type == "node_daemon" && event.operation == "self_stopped")
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(&event.summary)
                .expect("self-stop summary is structured JSON")
        })
        .find(|summary| summary["phase"] == "cooperative_interrupt_dispatched")
        .expect("the cooperative interrupt is journalled as its own self-stop phase");
    assert_eq!(dispatched["detail"]["supervisor_scopes_quiesced"], 1);
}

/// The other half of the same contract, end to end: after the machine latch,
/// the turn that registered too late actually refuses at
/// `acquire_prepared_cycle_turn` and never drives.
///
/// The ledger here shares its `supervisor_valid` `Arc` with the served context
/// exactly as `daemon_application.rs:104-110` does in production, and its
/// durable `TeamSupervisorLease` row is left Active — so the process-local
/// flip is the only thing that can refuse. Without it this is green and the
/// turn drives on under machine authority that is already gone.
#[test]
fn a_turn_admitted_before_the_machine_latch_cannot_drive_after_it() {
    let _serialized = crate::live_turn_serialization::serialized_live_provider_turns();
    let (store, root) = crate::tests::temp_store("machine-latch-refuses-late-turn");
    let (ledger, member) = crate::tests::persisted_native_test_member(
        &store,
        "codex",
        "codex_app_server",
        "thread-machine-latch",
    );
    let effect = crate::runtime_composition::prepare_provider_effect(
        &ledger,
        &member,
        "machine-latch-cycle",
        "must not drive",
        1,
    )
    .expect("prepare the effect before the latch");
    ledger
        .require_supervisor_lease()
        .expect("the durable Supervisor lease is Active before the latch");

    let fixture = adoption_fixture("machine-latch-refuses-late-turn");
    let daemon = &fixture.daemon;
    fixture
        .store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: daemon.node_id().to_string(),
            display_name: "Late turn Node".to_string(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        })
        .expect("insert late-turn Node");
    fixture
        .store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: daemon.node_id().to_string(),
                execution_space_id: fixture.execution_space_id.clone(),
                project_binding_id: "unit-test-project".to_string(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".to_string(),
                updated_at: "unix-ms:1".to_string(),
            },
            &fixture.execution_space_id,
        )
        .expect("register late-turn project");
    let space = daemon
        .registered_spaces()
        .expect("list late-turn Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.execution_space_id).then_some(space))
        .expect("late-turn Space");
    let owned_lease = daemon
        .ensure_node_authority(&space, &fixture.store)
        .expect("acquire owned daemon lease");
    // The served context carries the ledger's own scope Arc.
    daemon.push_context(OwnedTestContext::new(TestContextConfig {
        execution_space_id: fixture.execution_space_id.clone(),
        project_binding_id: "unit-test-project".to_string(),
        run_id: fixture.run_id.clone(),
        daemon_generation: owned_lease.generation,
        supervisor_id: "late-turn-supervisor".to_string(),
        supervisor_generation: 1,
        heartbeat_valid: Arc::clone(&ledger.supervisor_valid),
        thread: None,
        started_at: Instant::now(),
        serving_status: Arc::new(Mutex::new("running".to_string())),
    }));

    daemon
        .supersede_node_authority_for_test(&fixture.store)
        .expect("replace lease with a successor that fences renewal");
    daemon
        .refresh_held_node_authorities()
        .expect_err("heartbeat renewal must lose exact machine authority");

    // A turn that passed admission before the latch and registers only now.
    let late =
        harness_runtime_host::register_live_provider_turn("codex", &member.team_run_id, &member.id);
    assert_eq!(
        late.take_authority_loss_interrupt(),
        None,
        "the registry is not sticky; the quiesced scope is what must refuse"
    );

    let pool = Arc::new(crate::ActiveTurnLeasePool::new(1));
    let mut drive_calls = 0usize;
    let refusal = crate::runtime_adapter::acquire_prepared_cycle_turn(&ledger, &effect, &pool, &[]);
    if refusal.is_ok() {
        drive_calls += 1;
    }

    assert_eq!(drive_calls, 0, "a late turn must never enter the drive");
    let error = refusal.err().expect("acquire must refuse").to_string();
    assert!(
        error.contains("lost its durable Supervisor lease"),
        "refusal must be the lease-lost error: {error}"
    );
    std::fs::remove_dir_all(root).ok();
}
