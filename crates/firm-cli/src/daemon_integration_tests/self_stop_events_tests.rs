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
        3,
        "renewal, lease-loss, and process-group phases"
    );
    assert_eq!(self_stop[0]["kind"], "node_daemon_self_stop");
    assert_eq!(self_stop[0]["reason"], "NODE_DAEMON_MACHINE_AUTHORITY_LOST");
    assert_eq!(self_stop[0]["phase"], "renewal_failed");
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
    assert_eq!(self_stop[2]["phase"], "process_groups_terminated");
    assert_eq!(
        self_stop[2]["terminated_provider_process_groups"],
        serde_json::json!([4242])
    );
}
