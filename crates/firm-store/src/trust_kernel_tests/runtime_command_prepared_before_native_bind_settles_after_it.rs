use super::fabric_foundation::RuntimeCommandPoststate;
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
    let (mut interrupt, mut interrupt_context) = runtime_command_fixture(
        "runtime-bind-race-interrupt",
        RuntimeCommandKind::InterruptCurrentCycle,
        &session,
        "interrupt_current_cycle",
    );
    assert!(interrupt.binding.native_session_ref.is_none());
    // Every production caller pins the session version it prepared against;
    // the bind below moves it by exactly one, which settlement must tolerate.
    let version_at_prepare = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == session.id)
        .expect("the session")
        .version;
    interrupt.precondition.expected_session_version = Some(version_at_prepare);
    interrupt_context.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&interrupt).unwrap());
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
    assert_eq!(bound.projection.version, version_at_prepare + 1);

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
    // native session is still fenced at admission. (A foreign native id can
    // never reach settlement: prepare is strict and the session's native ref
    // is write-once, so this negative sits on the strict path by construction.)
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

/// The version-precondition escapes compose: a StopSession prepared before
/// the bind sees the bind's bump and its own close bump, and exactly those.
#[test]
fn version_precondition_tolerates_exactly_the_bind_and_the_commands_own_bump() {
    let mut closed = session("session-composed", "composed");
    closed.native_session_ref = Some(settled_native_session("thread-composed"));
    closed.lifecycle = AgentSessionStatus::Closed;
    let precondition = firm_core::agentfirm_api::RuntimeCommandPrecondition {
        expected_session_version: Some(5),
        ..Default::default()
    };
    let check = |version: u64, poststate: RuntimeCommandPoststate| {
        let mut session = closed.clone();
        session.version = version;
        HarnessStore::require_runtime_command_precondition_unlocked(
            &session,
            RuntimeCommandKind::StopSession,
            &precondition,
            poststate,
            "runtime_command",
            "composed",
            None,
        )
    };
    // Both bumps happened: bind (+1) and this command's close (+1).
    check(
        7,
        RuntimeCommandPoststate::CommandWithNativeSessionAttachment,
    )
    .expect("bind plus close is exactly the tolerated advance");
    // Only the close is tolerated without the attachment poststate.
    check(6, RuntimeCommandPoststate::Command).expect("the close alone");
    check(7, RuntimeCommandPoststate::Command)
        .expect_err("two bumps without the attachment poststate are not the command's own");
    // With both flags one bump is within the tolerated range (the predicate
    // reads the current lifecycle, so a session already Cold/Closed at prepare
    // contributes no bump of its own); three is never tolerated.
    check(
        6,
        RuntimeCommandPoststate::CommandWithNativeSessionAttachment,
    )
    .expect("the bind alone is within the tolerated range");
    check(
        8,
        RuntimeCommandPoststate::CommandWithNativeSessionAttachment,
    )
    .expect_err("anything beyond the two named mutations is fenced");
    // The exact expectation always passes.
    check(5, RuntimeCommandPoststate::Command).expect("exact version");
}
