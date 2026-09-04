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
fn a_start_that_wrote_no_canonical_row_classifies_as_no_progress_and_holds() {
    let fixture = adoption_fixture("drive-superseded");
    let entry = fixture.canonical_state();

    // A provider start superseded by lifecycle control returns before its
    // claim CAS, so no canonical row moves. `claim_member_provider_start`
    // nonetheless stamps `last_event_at = now()` on the paths that do reach
    // the CAS, which is why the fingerprint must exclude clock stamps: with
    // one included, every failing start looked like canonical progress and
    // earned another Supervisor generation (#671).
    let before = crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("read canonical MemberRuns");
    let exit = fixture.canonical_state();
    let after = crate::latest_member_runs_in_append_order(&fixture.store)
        .expect("read canonical MemberRuns again");
    assert_eq!(
        before, after,
        "the fixture wrote no canonical MemberRun row"
    );
    assert_eq!(
        entry, exit,
        "an adoption that wrote no canonical row must observe an identical fingerprint"
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
