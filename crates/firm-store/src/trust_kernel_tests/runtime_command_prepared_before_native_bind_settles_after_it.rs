use super::*;

/// GitHub #583 (DEV-231): a RuntimeCommand prepared before the provider
/// returned its native session id must still settle after the id attaches to
/// the same exact AgentSession generation, whatever the command kind. Before
/// this fix only StartSession/OpenRuntime/StartCycle tolerated the
/// attachment, so an Interrupt caught by the bind race was fenced at
/// settlement with MEMBER_RUN_GENERATION_FENCED and left an ambiguous command.
#[test]
fn runtime_command_prepared_before_native_bind_settles_after_it() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-bind-race", 0),
            identity("bind-race"),
        )
        .unwrap();
    let session = session("session-bind-race", "bind-race");
    assert!(session.native_session_ref.is_none());
    store
        .create_agent_session(
            &service_context("session.create", "session-bind-race", 0),
            session.clone(),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "session-bind-race-active", 1),
            &session.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap();

    // The first cycle is in flight before the native id is known.
    let (mut start, mut start_context) = runtime_command_fixture(
        "runtime-bind-race-start",
        RuntimeCommandKind::StartCycle,
        &session,
        "start_cycle",
    );
    start.payload["provider_attempt"] = serde_json::json!(1);
    start.payload_fingerprint = canonical_json_fingerprint(&start.payload);
    start_context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&start).unwrap());
    store
        .prepare_runtime_command(&start_context, &start, current_unix_ms(), "t-start")
        .expect("StartCycle admission before the native id is known");

    // The Host interrupts that cycle, also before the id is known.
    let (interrupt, interrupt_context) = runtime_command_fixture(
        "runtime-bind-race-interrupt",
        RuntimeCommandKind::InterruptCurrentCycle,
        &session,
        "interrupt_current_cycle",
    );
    assert!(interrupt.binding.native_session_ref.is_none());
    let admitted = store
        .prepare_runtime_command(
            &interrupt_context,
            &interrupt,
            current_unix_ms(),
            "t-interrupt",
        )
        .expect("an exact interrupt compensates the in-flight StartCycle");
    assert_eq!(admitted.projection.status, RuntimeCommandStatus::Accepted);

    // The bind race: the provider's native session id arrives and attaches to
    // the same generation while the interrupt is still in flight.
    let native = settled_native_session("thread-bind-race");
    let bound = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-bind-race", 2),
            &session.id,
            1,
            native.clone(),
        )
        .expect("the native session attaches to the same generation");
    assert_eq!(bound.projection.runtime_generation, 1);

    // The interrupt's durable settlement must not be fenced by the attachment.
    let settled = store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "runtime.interrupt.settle",
                "runtime-bind-race-interrupt:settle",
                admitted.projection.version,
            ),
            &interrupt.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({"interrupted": true})),
            None,
            "t-interrupt-applied",
        )
        .expect("an interrupt prepared before the native bind settles after it");
    assert_eq!(settled.projection.status, RuntimeCommandStatus::Applied);
    assert_eq!(
        settled.projection.effect_certainty,
        RuntimeEffectCertainty::Applied
    );

    // The tolerance is attachment only: a command whose binding names another
    // native session is still fenced at admission.
    let mut foreign = session.clone();
    foreign.native_session_ref = Some(settled_native_session("thread-other"));
    let (foreign_command, foreign_context) = runtime_command_fixture(
        "runtime-bind-race-foreign",
        RuntimeCommandKind::InterruptCurrentCycle,
        &foreign,
        "interrupt_current_cycle",
    );
    let fenced = store
        .prepare_runtime_command(
            &foreign_context,
            &foreign_command,
            current_unix_ms(),
            "t-foreign",
        )
        .expect_err("a binding naming another native session never replaces the bound one");
    assert!(
        fenced.to_string().contains("MEMBER_RUN_GENERATION_FENCED"),
        "{fenced}"
    );
    fs::remove_dir_all(root).unwrap();
}
