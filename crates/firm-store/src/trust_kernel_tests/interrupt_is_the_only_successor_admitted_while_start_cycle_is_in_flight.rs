use super::*;

#[test]
fn interrupt_is_the_only_successor_admitted_while_start_cycle_is_in_flight() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "host",
                "identity.create",
                "identity-compensating-control",
                0,
            ),
            identity("compensating-control"),
        )
        .unwrap();
    let session = session("session-compensating-control", "compensating-control");
    store
        .create_agent_session(
            &service_context("session.create", "session-compensating-control", 0),
            session.clone(),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "session-compensating-active", 1),
            &session.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap();

    let (mut start, mut start_context) = runtime_command_fixture(
        "runtime-compensating-start",
        RuntimeCommandKind::StartCycle,
        &session,
        "start_cycle",
    );
    start.payload["provider_attempt"] = serde_json::json!(1);
    start.payload_fingerprint = canonical_json_fingerprint(&start.payload);
    start_context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&start).unwrap());
    store
        .prepare_runtime_command(&start_context, &start, current_unix_ms(), "t-start")
        .expect("StartCycle admission");

    let (interrupt, interrupt_context) = runtime_command_fixture(
        "runtime-compensating-interrupt",
        RuntimeCommandKind::InterruptCurrentCycle,
        &session,
        "interrupt_current_cycle",
    );
    let admitted = store
        .prepare_runtime_command(
            &interrupt_context,
            &interrupt,
            current_unix_ms(),
            "t-interrupt",
        )
        .expect("an exact interrupt must be able to compensate an in-flight StartCycle");
    assert_eq!(admitted.projection.status, RuntimeCommandStatus::Accepted);

    let (quiesce, quiesce_context) = runtime_command_fixture(
        "runtime-compensating-quiesce",
        RuntimeCommandKind::QuiesceExecutionLane,
        &session,
        "quiesce_execution_lane",
    );
    let error = store
        .prepare_runtime_command(&quiesce_context, &quiesce, current_unix_ms(), "t-quiesce")
        .expect_err("quiesce still requires the cycle and interrupt to become terminal");
    assert!(error.to_string().contains("reconciliation is required"));
    fs::remove_dir_all(root).unwrap();
}
