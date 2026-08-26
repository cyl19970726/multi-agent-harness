use super::*;

fn wait_for_write_ticket(store: &HarnessStore, expected_next_ticket: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        let next_ticket = store
            .process_write_lock
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .next_ticket;
        if next_ticket >= expected_next_ticket {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "writer did not enter the Store FIFO queue"
        );
        std::thread::yield_now();
    }
}

fn canonical_member_run(id: &str, agent_member_id: &str, team_run_id: &str) -> MemberRun {
    MemberRun {
        id: id.into(),
        agent_member_id: agent_member_id.into(),
        team_run_id: team_run_id.into(),
        role_snapshot: "member".into(),
        provider_profile_snapshot: None,
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: MemberCoordinationStatus::Active,
        runtime_status: MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: None,
        native_session: None,
        version: 1,
        started_at: "t-member".into(),
        last_event_at: None,
        finished_at: None,
    }
}

fn assign_responsibility(
    store: &HarnessStore,
    work_id: &str,
    membership_id: &str,
) -> firm_core::Work {
    let work = insert_runtime_work(store, work_id, "team-admission", "run-admission");
    store
        .assign_work_to_membership(
            &work.id,
            work.version,
            membership_id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: format!("event-assign-{work_id}"),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("command-assign-{work_id}"),
                created_at: "t-assign".into(),
                duplicate_ok: false,
            },
        )
        .unwrap()
}

fn execution_binding(
    work: &firm_core::Work,
    membership: &TeamMembership,
    session: &AgentSession,
    id: &str,
) -> WorkExecutionBinding {
    WorkExecutionBinding {
        id: id.into(),
        work_id: work.id.clone(),
        work_revision: work.version,
        team_id: membership.team_id.clone(),
        team_membership_id: membership.id.clone(),
        agent_member_id: membership.agent_member_id.clone(),
        agent_session_id: session.id.clone(),
        agent_session_generation: session.runtime_generation,
        delivery_id: format!("delivery-{id}"),
        binding_generation: 1,
        status: WorkExecutionBindingStatus::Active,
        version: 1,
        created_by: ActorRef {
            kind: ActorKind::Service,
            id: "daemon-1".into(),
        },
        bound_at: "t-bind".into(),
        ended_at: None,
    }
}

#[test]
fn responsibility_resolves_one_current_member_run_and_repeated_admission_replays() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-admission",
                0,
            ),
            identity("worker-admission"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", "session-worker-admission", 0),
            target.clone(),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        )
        .unwrap();
    let work = assign_responsibility(&store, "work-admission", &membership.id);
    assert_eq!(work.active_member_run_id, None);

    let mut runtime_binding = runtime_command_fixture(
        "runtime-admission",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let binding = execution_binding(&work, &membership, &target, "binding-admission");
    let admission = service_context("work.bind", "binding-admission", 0);

    let mut stale = runtime_binding.clone();
    stale.target_member_run_generation = Some(2);
    let before_stale = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(&admission, &stale, binding.clone())
        .expect_err("stale MemberRun generation must not bind Work");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_stale);

    let accepted = store
        .bind_responsible_work_execution(&admission, &runtime_binding, binding.clone())
        .expect("exact current runtime admission");
    assert!(!accepted.replayed);
    let replay = store
        .bind_responsible_work_execution(&admission, &runtime_binding, binding)
        .expect("same scheduler admission is idempotent");
    assert!(replay.replayed);
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.fabric_work_deliveries("space-test").unwrap().len(), 1);
}

#[test]
fn missing_or_ambiguous_current_member_run_fails_before_delivery() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-admission",
                0,
            ),
            identity("worker-admission"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", "session-worker-admission", 0),
            target.clone(),
        )
        .unwrap();
    let missing_work = assign_responsibility(&store, "work-missing-run", &membership.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-missing",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let before_missing = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-missing", 0),
            &runtime_binding,
            execution_binding(&missing_work, &membership, &target, "binding-missing"),
        )
        .expect_err("missing current MemberRun must fail closed");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_missing);

    for id in ["member-run-admission", "member-run-duplicate"] {
        store
            .legacy_import_create_trust_member_run_projection(
                &context("host", "member_run.create", id, 0),
                canonical_member_run(id, "worker-admission", "run-admission"),
            )
            .unwrap();
    }
    let ambiguous_work = assign_responsibility(&store, "work-ambiguous-run", &membership.id);
    let before_ambiguous = store.canonical_operations().unwrap();
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-ambiguous", 0),
            &runtime_binding,
            execution_binding(&ambiguous_work, &membership, &target, "binding-ambiguous"),
        )
        .expect_err("ambiguous current MemberRun must fail closed");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_ambiguous);
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .is_empty());
    assert!(store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .is_empty());
}

#[test]
fn member_run_cutover_linearizes_before_stale_execution_admission() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context(
                "operator",
                "identity.create",
                "identity-worker-admission",
                0,
            ),
            identity("worker-admission"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let target = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", "session-worker-admission", 0),
            target.clone(),
        )
        .unwrap();
    let generation_one =
        canonical_member_run("member-run-admission", "worker-admission", "run-admission");
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            generation_one.clone(),
        )
        .unwrap();
    let work = assign_responsibility(&store, "work-cutover", &membership.id);
    let mut stale_runtime_binding = runtime_command_fixture(
        "runtime-cutover",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    stale_runtime_binding.target_member_run_id = Some(generation_one.id.clone());
    stale_runtime_binding.target_member_run_generation = Some(1);
    let stale_binding = execution_binding(&work, &membership, &target, "binding-cutover");

    let store = std::sync::Arc::new(store);
    let first = store.acquire_write_lock().expect("hold Store writer");
    let cutover_store = std::sync::Arc::clone(&store);
    let mut generation_two = generation_one;
    generation_two.runtime_generation = 2;
    generation_two.version = 2;
    let cutover = std::thread::spawn(move || {
        let _lock = cutover_store.acquire_write_lock()?;
        cutover_store.commit_trust_projection_unlocked(
            &context("host", "member_run.cutover", "member-run-admission", 1),
            "member_run",
            "member-run-admission",
            "runtime_generation_advanced",
            serde_json::to_value(&generation_two)?,
            &generation_two,
            Vec::new(),
            Vec::new(),
        )
    });
    wait_for_write_ticket(&store, 2);

    let admission_store = std::sync::Arc::clone(&store);
    let admission = std::thread::spawn(move || {
        admission_store.bind_responsible_work_execution(
            &service_context("work.bind", "binding-cutover", 0),
            &stale_runtime_binding,
            stale_binding,
        )
    });
    wait_for_write_ticket(&store, 3);
    drop(first);

    cutover
        .join()
        .expect("cutover writer joins")
        .expect("generation cutover commits first");
    let error = admission
        .join()
        .expect("admission writer joins")
        .expect_err("old generation cannot bind after cutover");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert!(store
        .fabric_work_execution_bindings("space-test")
        .unwrap()
        .is_empty());
    assert!(store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .is_empty());
}
