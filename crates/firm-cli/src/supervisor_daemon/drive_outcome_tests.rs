//! Drive-outcome classification and reap routing, against real Store rows.
//!
//! These are the tests that would have caught the fingerprint self-refutation:
//! a start that writes no canonical row must produce an identical fingerprint,
//! and therefore a hold, while a run that gained a Work operation must not.

use super::adoption_tests::{adoption_fixture, AdoptionFixture};
use super::*;

impl AdoptionFixture {
    fn context(&self) -> MultiTeamContext {
        MultiTeamContext {
            execution_space_id: self.execution_space_id.clone(),
            project_binding_id: "unit-test-project".into(),
            run_id: self.run_id.clone(),
            daemon_generation: 1,
            supervisor_id: "settle-supervisor".into(),
            supervisor_generation: 1,
            heartbeat_valid: Arc::new(AtomicBool::new(false)),
            serving_status: Arc::new(Mutex::new("running".into())),
            thread: None,
            started_at: Instant::now(),
        }
    }

    fn no_progress_markers(&self) -> usize {
        self.store
            .member_actions()
            .expect("read member actions")
            .into_iter()
            .filter(|action| {
                action.team_run_id == self.run_id
                    && action.action_type == "team_supervisor_no_progress"
            })
            .count()
    }

    /// One real Work operation for this run: the canonical progress that must
    /// always re-enable adoption.
    fn add_work_operation(&self, id: &str) {
        let run = crate::latest_team_run(&self.store, &self.run_id).expect("read TeamRun");
        let host_actor = run.host_actor.clone().expect("exact fixture Host");
        self.store
            .insert_work(
                harness_core::CurrentWorkDraft::new(
                    id.into(),
                    self.run_id.clone(),
                    run.agent_team_id.clone(),
                    "new responsibility".into(),
                    "the Host added Work after the last adoption".into(),
                    "member completes it".into(),
                    harness_core::WorkClaimMode::TeamClaim,
                    harness_core::WorkPriority::Normal,
                    host_actor.clone(),
                    "unix-ms:3".into(),
                )
                .into_work(),
                harness_core::WorkCommandContext {
                    event_id: format!("{id}-create"),
                    performed_by_actor: host_actor,
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("{id}-create"),
                    created_at: "unix-ms:3".into(),
                    duplicate_ok: false,
                },
            )
            .expect("seed a canonical Work operation");
    }
}

#[test]
fn a_start_that_only_moved_a_clock_stamp_classifies_as_no_progress_and_holds() {
    let fixture = adoption_fixture("drive-superseded");
    let entry = fixture.canonical_state();

    // Perform the exact durable write a failing provider start leaves behind.
    // `claim_member_provider_start` stamps `last_event_at = now()` before the
    // transport is attempted, and `prepare_team_run_start_body` rewrites the
    // MemberRun row for a refreshed provider profile. Neither is a
    // coordination fact, but while `last_event_at` was in the fingerprint
    // every failing start looked like canonical progress and earned another
    // Supervisor generation (#671).
    let mut member = crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("read canonical MemberRuns")
        .into_iter()
        .find(|member| member.team_run_id == fixture.run_id && member.role != "host")
        .expect("exact fixture MemberRun");
    let expected = member.clone();
    member.last_event_at = Some(crate::now_string());
    fixture
        .store
        .compare_and_append_member_run(&expected, &member)
        .expect("stamp last_event_at the way a failing provider start does");

    let stamped = crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("read canonical MemberRuns again")
        .into_iter()
        .find(|candidate| candidate.id == member.id)
        .expect("the stamped MemberRun row");
    assert_ne!(
        stamped.last_event_at, expected.last_event_at,
        "the fixture must actually have moved the clock stamp"
    );

    let exit = fixture.canonical_state();
    assert_eq!(
        entry, exit,
        "an adoption that moved only a clock stamp must observe an identical fingerprint"
    );

    let outcome = crate::classify_team_run_drive_outcome(
        harness_core::TeamRunStatus::Running,
        &entry,
        &exit,
        1,
    );
    let TeamRunDriveOutcome::NoProgress {
        ref canonical_state,
        ..
    } = outcome
    else {
        panic!("an unchanged Running TeamRun is a no-progress outcome: {outcome:?}");
    };
    assert_eq!(canonical_state, &exit);

    fixture
        .daemon
        .settle_finished_supervisor(&fixture.context(), outcome);
    assert_eq!(
        fixture.no_progress_markers(),
        1,
        "the reap must route a no-progress outcome to a durable hold"
    );
    assert!(
        fixture.adoption_is_held(),
        "the next scan must not re-adopt the identical canonical state"
    );
}

#[test]
fn a_run_that_gained_a_work_operation_classifies_as_progressed_and_never_holds() {
    let fixture = adoption_fixture("drive-progressed");
    let entry = fixture.canonical_state();
    fixture.add_work_operation("work-canonical-progress");
    let exit = fixture.canonical_state();
    assert_ne!(
        entry, exit,
        "a new Work operation is canonical progress the fingerprint must see"
    );

    let outcome = crate::classify_team_run_drive_outcome(
        harness_core::TeamRunStatus::Running,
        &entry,
        &exit,
        1,
    );
    assert_eq!(
        outcome,
        TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Running,
        }
    );

    fixture
        .daemon
        .settle_finished_supervisor(&fixture.context(), outcome);
    assert_eq!(
        fixture.no_progress_markers(),
        0,
        "a generation that made canonical progress writes no hold"
    );
    assert!(
        !fixture.adoption_is_held(),
        "a run that gained Work stays adoptable"
    );
}

#[test]
fn a_team_run_that_left_running_is_settled_not_held() {
    let fixture = adoption_fixture("drive-settled");
    let entry = fixture.canonical_state();
    let outcome = crate::classify_team_run_drive_outcome(
        harness_core::TeamRunStatus::Completed,
        &entry,
        &entry,
        1,
    );
    assert_eq!(
        outcome,
        TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        },
        "a TeamRun that left Running is settled, never a no-progress observation"
    );
    fixture
        .daemon
        .settle_finished_supervisor(&fixture.context(), outcome);
    assert_eq!(fixture.no_progress_markers(), 0);
}

#[test]
fn a_volatile_hold_keyed_to_canonical_state_is_lifted_by_canonical_change() {
    let fixture = adoption_fixture("volatile-hold");
    let observed = fixture.canonical_state();
    // Model the durable-write failure path — for example a legacy run with no
    // Host MemberRun to project a marker onto.
    fixture
        .daemon
        .recovery_blocked_runs
        .lock()
        .expect("volatile hold registry")
        .insert(
            (fixture.execution_space_id.clone(), fixture.run_id.clone()),
            VolatileAdoptionHold::AtCanonicalState(observed),
        );
    assert!(fixture.adoption_is_held());

    fixture.add_work_operation("work-lifts-volatile-hold");
    assert!(
        !fixture.adoption_is_held(),
        "a state-keyed volatile hold must not strand a run for the daemon's whole lifetime"
    );

    fixture
        .daemon
        .recovery_blocked_runs
        .lock()
        .expect("volatile hold registry")
        .insert(
            (fixture.execution_space_id.clone(), fixture.run_id.clone()),
            VolatileAdoptionHold::Unconditional,
        );
    assert!(
        fixture.adoption_is_held(),
        "an unreadable Store leaves nothing to prove change against, so that hold stays"
    );
}

#[test]
fn a_settling_run_is_not_adopted_while_its_dead_generation_writes_its_outcome() {
    let fixture = adoption_fixture("settling-window");
    let key = (fixture.execution_space_id.clone(), fixture.run_id.clone());
    assert!(!fixture.adoption_is_held());
    fixture
        .daemon
        .settling_runs
        .lock()
        .expect("settling registry")
        .insert(key.clone());
    assert!(
        fixture.adoption_is_held(),
        "a marker still being written must not land on a live successor generation"
    );
    fixture
        .daemon
        .settling_runs
        .lock()
        .expect("settling registry")
        .remove(&key);
    assert!(!fixture.adoption_is_held());
}

/// Every Store `Conflict` reachable from `start_supervising` must arrive at
/// `start_failure_is_transient` typed, and must therefore leave no durable
/// adoption hold. Three sites on that path can produce one; this drives the
/// real production function that owns each mapping — not the store call under
/// it and not a hand-built error — so reverting any one of them to
/// `store_conflict_as_usage` fails this test (DEV-149-REVIEW-03/04).
#[test]
fn every_start_path_store_conflict_is_typed_and_records_no_hold() {
    let fixture = adoption_fixture("start-path-conflicts");
    let run = crate::latest_team_run(&fixture.store, &fixture.run_id).expect("read TeamRun");

    // Site 1 — the MemberRun write-back `prepare_team_run_start_body` performs
    // for a refreshed provider profile, driven through the named production
    // function that owns the mapping.
    let mut member = crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("read canonical MemberRuns")
        .into_iter()
        .find(|member| member.team_run_id == fixture.run_id && member.role != "host")
        .expect("exact fixture MemberRun");
    let stale_member = member.clone();
    member.last_event_at = Some(crate::now_string());
    fixture
        .store
        .compare_and_append_member_run(&stale_member, &member)
        .expect("the concurrent Host write lands first");
    let mut losing = stale_member.clone();
    losing.provider_cwd_hint = Some("/tmp/losing-adoption".into());
    let member_conflict =
        crate::persist_refreshed_member_profile(&fixture.store, &stale_member, &losing)
            .expect_err("the adoption's CAS must lose against the newer row");
    assert!(
        matches!(
            member_conflict,
            CliError::Store(harness_store::StoreError::Conflict(_))
        ),
        "the MemberRun write-back must surface a typed Store conflict: {member_conflict}"
    );

    // Site 2 — the Supervisor lease, driven through the real
    // `TeamSupervisorRegistration::start`. A live incumbent lease makes the
    // acquisition lose before any thread is spawned.
    let node_lease = fixture
        .store
        .acquire_node_daemon_lease(
            &run.execution_node_id,
            &format!("node-daemon:{}", run.execution_node_id),
            "conflict-test-instance",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire the parent NodeDaemon lease");
    fixture
        .store
        .acquire_team_supervisor_under_node_lease(
            &fixture.run_id,
            &run.execution_node_id,
            &node_lease.daemon_id,
            node_lease.generation,
            &fixture.execution_space_id,
            &run.project_binding_id,
            "incumbent-supervisor",
            std::process::id(),
            "test://incumbent-supervisor",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("the incumbent Supervisor holds the lease");
    let lease_conflict = crate::TeamSupervisorRegistration::start(
        &fixture.store,
        &fixture.run_id,
        Some(&fixture.execution_space_id),
    )
    .err()
    .expect("a live incumbent lease must deny a second Supervisor registration");
    assert!(
        matches!(
            lease_conflict,
            CliError::Store(harness_store::StoreError::Conflict(_))
        ),
        "TeamSupervisorRegistration::start must surface a typed Store conflict: {lease_conflict}"
    );

    // Site 3 — TeamRun scope resolution, driven through the real
    // `prepare_team_run_start_body`. Declaring a MemberRun id that has no
    // canonical projection makes the resolver report incomplete
    // materialization, which is the same resolver Conflict a concurrent Host
    // append produces as `TEAM_RUN_CHANGED`. It is exercised last because the
    // appended row deliberately leaves the Store unresolvable.
    let mut partial = run.clone();
    partial
        .member_run_ids
        .push("member-run-never-materialized".to_string());
    partial.updated_at = crate::now_string();
    let mut team_runs = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(fixture.store.root().join("team_runs.jsonl"))
        .expect("open TeamRun ledger");
    writeln!(
        team_runs,
        "{}",
        serde_json::to_string(&partial).expect("serialize partial TeamRun")
    )
    .expect("append the partially materialized TeamRun row");
    drop(team_runs);

    let scope_conflict = crate::prepare_team_run_start_body(&fixture.store, &fixture.run_id, 1)
        .err()
        .expect("start preparation must reject an unresolvable TeamRun scope");
    assert!(
        matches!(
            scope_conflict,
            CliError::Store(harness_store::StoreError::Conflict(_))
        ),
        "prepare_team_run_start_body must propagate the resolver's typed Store conflict: {scope_conflict}"
    );

    // The property under test: none of the three records a durable hold.
    for conflict in [&scope_conflict, &member_conflict, &lease_conflict] {
        fixture.daemon.block_start_failure_if_unresolved(
            &fixture.execution_space_id,
            &fixture.store,
            &fixture.run_id,
            conflict,
        );
    }
    assert_eq!(
        fixture.no_progress_markers(),
        0,
        "a lost race on the start path is this attempt's problem, never a durable property of the TeamRun"
    );
    assert!(
        !fixture.adoption_is_held(),
        "a healthy run must stay adoptable after any start-path Store conflict"
    );
}
