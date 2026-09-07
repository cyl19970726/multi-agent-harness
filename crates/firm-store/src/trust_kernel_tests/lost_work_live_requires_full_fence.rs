use super::host_redelivers_open_work_after_member_close_and_reopen::{
    deliver_to_provider, host_context, member_context, reopened_member_fixture,
    ReopenedMemberFixture,
};
use super::work_responsibility_execution_admission_is_exact_and_idempotent::assign_responsibility;
use super::*;

fn active_fixture(suffix: &str, native: bool) -> (ReopenedMemberFixture, PathBuf, firm_core::Work) {
    let (mut fixture, root) = reopened_member_fixture(suffix);
    if native {
        fixture.session = fixture
            .store
            .bind_agent_session_native_session(
                &service_context(
                    "session.native.bind",
                    "initial-native",
                    fixture.session.version,
                ),
                &fixture.session.id,
                fixture.session.runtime_generation,
                settled_native_session("original-native"),
            )
            .unwrap()
            .projection;
        fixture.runtime_binding.native_session_ref = fixture.session.native_session_ref.clone();
    }
    let work = assign_responsibility(
        &fixture.store,
        &format!("work-{suffix}"),
        &fixture.membership.id,
    );
    deliver_to_provider(&fixture, &work, suffix);
    let started = fixture
        .store
        .start_work(
            &work.id,
            work.version,
            &fixture.member_run_id,
            member_context(
                &fixture.member_run_id,
                "start-fence-work",
                "start-fence-work",
            ),
        )
        .unwrap();
    (fixture, root, started)
}

fn assert_unproven_without_writes(fixture: &ReopenedMemberFixture, work: &firm_core::Work) {
    let store = &fixture.store;
    let before = store.canonical_operations().unwrap();
    let work_before = store.work_operations().unwrap();
    let error = store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(store, "recover-unproven", "recover-unproven"),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("WORK_EXECUTION_AUTHORITY_UNPROVEN"),
        "{error}"
    );
    assert!(!error.contains("WORK_EXECUTION_AUTHORITY_LIVE"), "{error}");
    let scan = store
        .lost_work_executions("space-test", &work.team_run_id)
        .unwrap();
    assert!(scan.lost.is_empty());
    assert!(scan.errors.iter().any(|error| error.work_id == work.id
        && error.error.contains("WORK_EXECUTION_AUTHORITY_UNPROVEN")));
    assert_eq!(store.canonical_operations().unwrap(), before);
    assert_eq!(store.work_operations().unwrap(), work_before);
}

#[test]
fn recovery_never_calls_an_incomplete_runtime_fence_live_or_releases_it() {
    // Fault-inject one durable projection field at a time to isolate each
    // conjunct. These are adversarial source facts, not claims that the normal
    // session mutation API admits arbitrary transfers. Legal transfers are
    // covered separately below.
    type SessionDrift = fn(&mut AgentSession);
    let cases: &[(&str, SessionDrift)] = &[
        ("driver-generation", |s| {
            s.control_state.driver_generation += 1
        }),
        ("driver-reference", |s| {
            s.control_state.driver_ref = RuntimeDriverRef::Unknown
        }),
        ("native-reference", |s| {
            s.native_session_ref.as_mut().unwrap().native_session_id = "different-native".into()
        }),
        ("permission", |s| {
            s.permission_envelope_ref = "different-permission".into()
        }),
        ("composition", |s| {
            s.control_state.composition_fingerprint = Some("different-composition".into())
        }),
        ("capability", |s| {
            s.control_state.capability_fingerprint = Some("different-capability".into())
        }),
        ("node-daemon-generation", |s| s.node_daemon_generation += 1),
    ];
    for (label, mutate) in cases {
        let (fixture, root, work) = active_fixture(label, true);
        let mut changed = fixture.session.clone();
        mutate(&mut changed);
        changed.version += 1;
        {
            let _lock = fixture.store.acquire_write_lock().unwrap();
            fixture
                .store
                .commit_trust_projection_unlocked(
                    &service_context("test.session.drift", label, fixture.session.version),
                    "agent_session",
                    &changed.id,
                    "test_projection_drift",
                    serde_json::to_value(&changed).unwrap(),
                    &changed,
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap();
        }
        assert_unproven_without_writes(&fixture, &work);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn legal_driver_handoff_and_daemon_drain_are_unproven_not_live() {
    for drain in [false, true] {
        let (fixture, root, work) =
            active_fixture(if drain { "draining" } else { "handoff" }, false);
        if drain {
            fixture
                .store
                .drain_node_daemon_lease(
                    &fixture.session.node_id,
                    "daemon-1",
                    1,
                    "instance-1",
                    current_unix_ms(),
                    60_000,
                )
                .unwrap();
        } else {
            let mut control = fixture.session.control_state.clone();
            control.activity = RuntimeActivity::Idle;
            let quiet = fixture
                .store
                .bind_agent_session_control_state(
                    &service_context(
                        "session.control.bind",
                        "idle-before-handoff",
                        fixture.session.version,
                    ),
                    &fixture.session.id,
                    fixture.session.runtime_generation,
                    control.clone(),
                    "t-idle",
                )
                .unwrap()
                .projection;
            control.driver_generation += 1;
            control.composition_fingerprint = Some("composition:successor".into());
            fixture
                .store
                .bind_agent_session_control_state(
                    &service_context("session.control.bind", "legal-handoff", quiet.version),
                    &fixture.session.id,
                    fixture.session.runtime_generation,
                    control,
                    "t-handoff",
                )
                .unwrap();
        }
        assert_unproven_without_writes(&fixture, &work);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn first_native_attachment_remains_current_under_the_existing_admission_rule() {
    let (fixture, root, work) = active_fixture("first-attachment", false);
    fixture
        .store
        .bind_agent_session_native_session(
            &service_context(
                "session.native.bind",
                "first-native",
                fixture.session.version,
            ),
            &fixture.session.id,
            fixture.session.runtime_generation,
            settled_native_session("first-native"),
        )
        .unwrap();
    let before = fixture.store.canonical_operations().unwrap();
    let error = fixture
        .store
        .recover_lost_work_execution(
            &work.id,
            work.version,
            "space-test",
            None,
            host_context(&fixture.store, "recover-current", "recover-current"),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("WORK_EXECUTION_AUTHORITY_LIVE"), "{error}");
    let scan = fixture
        .store
        .lost_work_executions("space-test", &work.team_run_id)
        .unwrap();
    assert!(scan.errors.is_empty());
    assert!(scan.lost.is_empty());
    assert_eq!(fixture.store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
