use super::*;

#[test]
fn responsibility_aba_and_stale_session_generation_do_not_revive_old_binding() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    for member_id in ["worker-admission", "alternate-admission"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "operator",
                    "identity.create",
                    &format!("identity-{member_id}"),
                    0,
                ),
                identity(member_id),
            )
            .unwrap();
    }
    let worker = join_runtime_membership(
        &store,
        "membership-worker-admission",
        "team-admission",
        "worker-admission",
        TeamMembershipRole::Member,
    );
    let alternate = join_runtime_membership(
        &store,
        "membership-alternate-admission",
        "team-admission",
        "alternate-admission",
        TeamMembershipRole::Member,
    );
    let worker_session = session("session-worker-admission", "worker-admission");
    store
        .create_agent_session(
            &service_context("session.create", &worker_session.id, 0),
            worker_session.clone(),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-admission", 0),
            canonical_member_run("member-run-admission", "worker-admission", "run-admission"),
        )
        .unwrap();
    let assigned = assign_responsibility(&store, "work-aba", &worker.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-aba",
        RuntimeCommandKind::StartCycle,
        &worker_session,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-admission".into());
    runtime_binding.target_member_run_generation = Some(1);
    let old_binding = execution_binding(&assigned, &worker, &worker_session, "binding-aba");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-aba", 0),
            &runtime_binding,
            old_binding,
        )
        .unwrap();
    let assigned_to_b = store
        .assign_work_to_membership(
            &assigned.id,
            assigned.version,
            &alternate.id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: "event-assign-b".into(),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-assign-b".into(),
                created_at: "t-b".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let assigned_back_to_a = store
        .assign_work_to_membership(
            &assigned.id,
            assigned_to_b.version,
            &worker.id,
            "space-test",
            firm_core::WorkCommandContext {
                event_id: "event-assign-a-again".into(),
                performed_by_actor: store.exact_team_run_host_actor("run-admission").unwrap(),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-assign-a-again".into(),
                created_at: "t-a-again".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let projection_error = store
        .current_work_deliveries("space-test")
        .expect_err("A to B to A cannot revive the old delivery projection");
    assert!(projection_error
        .to_string()
        .contains("CURRENT_WORK_DELIVERY_CANONICAL_JOIN_CONFLICT"));
    let before_aba = store.canonical_operations().unwrap();
    let progress = WorkReport {
        id: "report-aba".into(),
        work_id: assigned.id.clone(),
        work_revision: assigned_back_to_a.version,
        report_revision: 1,
        kind: WorkReportKind::Progress,
        authored_by: ActorRef {
            kind: ActorKind::AgentMember,
            id: "worker-admission".into(),
        },
        summary: "old binding must not revive".into(),
        base_revision: None,
        candidate: None,
        candidate_fingerprint: None,
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        evidence_refs: Vec::new(),
        known_risks: Vec::new(),
        confidence: None,
        recommended_next_action: None,
        created_at: "t-aba-report".into(),
    };
    let error = store
        .create_trust_work_report(
            &member_context("worker-admission", "report.create", &progress.id, 0),
            "team-admission",
            progress,
        )
        .expect_err("A to B to A responsibility must not revive the old binding");
    assert!(error.to_string().contains("UNAUTHORIZED_ACTOR"));
    assert_eq!(store.canonical_operations().unwrap(), before_aba);

    let stale_generation_work = assign_responsibility(&store, "work-stale-session", &worker.id);
    let stale_generation_binding = execution_binding(
        &stale_generation_work,
        &worker,
        &worker_session,
        "binding-stale-session",
    );
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-stale-session", 0),
            &runtime_binding,
            stale_generation_binding,
        )
        .unwrap();

    let mut advanced_session = worker_session.clone();
    advanced_session.runtime_generation = 2;
    advanced_session.version += 1;
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &service_context("session.advance", "session-worker-generation-2", 1),
                "agent_session",
                &advanced_session.id,
                "runtime_generation_advanced",
                serde_json::json!({"runtime_generation": 2}),
                &advanced_session,
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
    }
    let before_stale_session = store.canonical_operations().unwrap();
    let error = store
        .author_message(
            &service_context("message.author", "message-stale-session", 0),
            work_message(
                "message-stale-session",
                &stale_generation_work,
                "worker-admission",
                &advanced_session.id,
                "alternate-admission",
            ),
        )
        .expect_err("stale binding generation cannot authorize a Work-linked Message");
    assert!(error.to_string().contains("NATIVE_SESSION_INCOMPATIBLE"));
    assert_eq!(store.canonical_operations().unwrap(), before_stale_session);
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
