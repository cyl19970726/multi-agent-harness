use super::*;

#[test]
fn start_reconciliation_requires_one_exact_live_daemon_and_supervisor_generation() {
    let daemon = harness_core::NodeDaemonLease {
        node_id: "node".into(),
        daemon_id: "node-daemon:node".into(),
        generation: 7,
        instance_id: "instance".into(),
        status: harness_core::NodeDaemonLeaseStatus::Active,
        acquired_unix_ms: 10,
        renewed_unix_ms: 20,
        expires_unix_ms: 100,
        released_unix_ms: None,
    };
    let supervisor = harness_core::TeamSupervisorLease {
        team_run_id: "run".into(),
        node_id: "node".into(),
        node_daemon_id: daemon.daemon_id.clone(),
        node_daemon_generation: daemon.generation,
        execution_space_id: "space".into(),
        project_binding_id: "project".into(),
        supervisor_id: "supervisor".into(),
        generation: 3,
        owner_process_id: 42,
        owner_locator: "test://supervisor".into(),
        status: harness_core::TeamSupervisorLeaseStatus::Active,
        acquired_unix_ms: 10,
        heartbeat_unix_ms: 20,
        expires_unix_ms: 100,
        released_unix_ms: None,
    };
    assert!(start_postcondition_matches(
        &daemon,
        &supervisor,
        "node",
        "instance",
        7,
        "space",
        "project",
        "run",
        "supervisor",
        3,
        42,
        50,
    ));
    assert!(!start_postcondition_matches(
        &daemon,
        &supervisor,
        "node",
        "instance",
        8,
        "space",
        "project",
        "run",
        "supervisor",
        3,
        42,
        50,
    ));
    assert!(!start_postcondition_matches(
        &daemon,
        &supervisor,
        "node",
        "instance",
        7,
        "space",
        "project",
        "run",
        "supervisor",
        3,
        43,
        50,
    ));
}

fn valid_status(node_id: &str, runs: serde_json::Value) -> String {
    serde_json::json!({
        "ok": true,
        "node_id": node_id,
        "runs": runs,
    })
    .to_string()
}

fn run_observation(
    budget: Duration,
    interval: Duration,
    mut steps: std::collections::VecDeque<FetchStep>,
    mut prove: impl FnMut(usize) -> Option<CliResult<serde_json::Value>>,
) -> ObservationOutcome {
    let current = std::rc::Rc::new(std::cell::RefCell::new(std::time::Instant::now()));
    let fetch_budgets = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sleeps = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let fetch_count = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let mut fetch_status = {
        let current = current.clone();
        let fetch_budgets = fetch_budgets.clone();
        let fetch_count = fetch_count.clone();
        move |io_budget: Duration| {
            fetch_budgets.borrow_mut().push(io_budget);
            fetch_count.set(fetch_count.get() + 1);
            match steps
                .pop_front()
                .unwrap_or(FetchStep::Immediate(Some(valid_status(
                    "node",
                    serde_json::json!([]),
                )))) {
                FetchStep::Immediate(status) => status,
                FetchStep::StallThenAnswer(status) => {
                    *current.borrow_mut() += io_budget;
                    status
                }
            }
        }
    };
    let mut prove_postcondition = {
        let fetch_count = fetch_count.clone();
        move |_status: &str| prove(fetch_count.get())
    };
    let mut now = {
        let current = current.clone();
        move || *current.borrow()
    };
    let mut sleep = {
        let current = current.clone();
        let sleeps = sleeps.clone();
        move |duration: Duration| {
            *current.borrow_mut() += duration;
            sleeps.borrow_mut().push(duration);
        }
    };
    let result = reconcile_team_run_start_with_observation(
        "node",
        budget,
        interval,
        &mut fetch_status,
        &mut prove_postcondition,
        &mut now,
        &mut sleep,
    );
    ObservationOutcome {
        result,
        fetch_budgets: fetch_budgets.take(),
        sleeps: sleeps.take(),
        fetches: fetch_count.get(),
    }
}

#[test]
fn start_observation_proves_a_late_postcondition_within_budget() {
    let outcome = run_observation(
        Duration::from_secs(60),
        Duration::from_secs(1),
        std::collections::VecDeque::new(),
        |fetch| (fetch == 3).then(|| Ok(serde_json::json!({"daemon_response": {"ok": true}}))),
    );
    assert_eq!(outcome.fetches, 3, "proof arrived on the third poll");
    assert_eq!(
        outcome.sleeps,
        vec![Duration::from_secs(1), Duration::from_secs(1)]
    );
    assert_eq!(
        outcome.fetch_budgets,
        vec![
            Duration::from_secs(60),
            Duration::from_secs(59),
            Duration::from_secs(58),
        ],
        "every status I/O is bounded by the remaining observation budget"
    );
    let result = outcome.result.expect("late proof must resolve");
    assert!(result.is_ok(), "late proof: {result:?}");
}

#[test]
fn start_observation_stops_at_the_deadline_without_a_post_deadline_poll() {
    let outcome = run_observation(
        Duration::from_secs(5),
        Duration::from_secs(2),
        std::collections::VecDeque::new(),
        |_fetch| None,
    );
    assert!(outcome.result.is_none(), "unproved outcome stays UNKNOWN");
    assert_eq!(outcome.fetches, 3, "no poll launches after the deadline");
    assert_eq!(
        outcome.fetch_budgets,
        vec![
            Duration::from_secs(5),
            Duration::from_secs(3),
            Duration::from_secs(1),
        ]
    );
    assert_eq!(
        outcome.sleeps,
        vec![
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(1),
        ],
        "the final sleep is clamped to the remaining budget"
    );
}

#[test]
fn start_observation_fails_promptly_when_status_is_unreachable() {
    let outcome = run_observation(
        Duration::from_secs(60),
        Duration::from_secs(1),
        std::collections::VecDeque::from([FetchStep::Immediate(None)]),
        |_fetch| None,
    );
    assert!(outcome.result.is_none());
    assert_eq!(outcome.fetches, 1, "unreachable status is not adoption");
    assert!(outcome.sleeps.is_empty());
}

#[test]
fn start_observation_fails_promptly_on_malformed_or_untrusted_status() {
    for status in [
        "not json".to_string(),
        serde_json::json!({"ok": false, "node_id": "node"}).to_string(),
        serde_json::json!({"ok": true, "node_id": "other-node"}).to_string(),
    ] {
        let outcome = run_observation(
            Duration::from_secs(60),
            Duration::from_secs(1),
            std::collections::VecDeque::from([FetchStep::Immediate(Some(status))]),
            |_fetch| None,
        );
        assert!(outcome.result.is_none());
        assert_eq!(
            outcome.fetches, 1,
            "malformed or untrusted status must fail promptly, not read as still adopting"
        );
        assert!(outcome.sleeps.is_empty());
    }
}

#[test]
fn start_observation_returns_authoritative_errors_without_retrying() {
    let outcome = run_observation(
        Duration::from_secs(60),
        Duration::from_secs(1),
        std::collections::VecDeque::new(),
        |_fetch| Some(Err(CliError::Usage("authoritative store rejection".into()))),
    );
    assert_eq!(outcome.fetches, 1);
    assert!(outcome.sleeps.is_empty());
    let result = outcome.result.expect("authoritative error must surface");
    let error = result.expect_err("authoritative error must surface verbatim");
    assert!(error.to_string().contains("authoritative store rejection"));
}

#[test]
fn start_observation_never_fabricates_success_for_changed_authority() {
    // The run is listed, but the exact postcondition never proves (a
    // changed daemon/Supervisor authority): observation must exhaust and
    // preserve UNKNOWN rather than claim success from presence alone.
    let listed = valid_status(
        "node",
        serde_json::json!([{
            "execution_space_id": "space",
            "run_id": "run",
            "status": "running",
        }]),
    );
    let outcome = run_observation(
        Duration::from_secs(3),
        Duration::from_secs(1),
        std::collections::VecDeque::from([
            FetchStep::Immediate(Some(listed.clone())),
            FetchStep::Immediate(Some(listed.clone())),
            FetchStep::Immediate(Some(listed)),
        ]),
        |_fetch| None,
    );
    assert!(outcome.result.is_none());
    assert_eq!(outcome.fetches, 3);
}

#[test]
fn start_observation_bounds_stalled_status_io_and_never_polls_past_deadline() {
    let outcome = run_observation(
        Duration::from_secs(10),
        Duration::from_secs(1),
        std::collections::VecDeque::from([FetchStep::StallThenAnswer(Some(valid_status(
            "node",
            serde_json::json!([]),
        )))]),
        |_fetch| None,
    );
    assert!(outcome.result.is_none());
    assert_eq!(
        outcome.fetches, 1,
        "a status read that consumes the whole remaining budget leaves no poll after it"
    );
    assert!(outcome.sleeps.is_empty());
}
enum FetchStep {
    Immediate(Option<String>),
    /// Consume the entire offered I/O budget, then answer: a status read
    /// bounded by the remaining observation budget.
    StallThenAnswer(Option<String>),
}

struct ObservationOutcome {
    result: Option<CliResult<serde_json::Value>>,
    fetch_budgets: Vec<Duration>,
    sleeps: Vec<Duration>,
    fetches: usize,
}
