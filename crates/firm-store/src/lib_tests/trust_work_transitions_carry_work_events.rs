use super::*;
use firm_core::agentfirm_api::MutationContext;

/// W3 (B): every trust-journal Work transition commits the WorkEvent the
/// ledger would have carried.
///
/// Before this slice a Result submission advanced its Work to Review only as
/// an immutable side record of the `work_report/created` envelope, and the
/// acceptance transition carried no WorkEvent at all. Nothing in the codebase
/// ever constructed `WorkEventKind::Submitted` or `WorkEventKind::Accepted`,
/// so a Work's history stopped at Started and the acceptance wake had to
/// synthesize its own Accepted event for one read.
#[test]
fn trust_work_transitions_carry_work_events() {
    let (root, store, run, member, other) = work_test_fixture("trust-work-events");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "events"),
            host_work_context("create-events", "create-events", "unix-ms:2"),
        )
        .unwrap();
    let work = assign_test_work_to_member(
        &store,
        &run,
        &work,
        &member,
        "assign-events",
        "assign-events",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &work,
        &member,
        "start-events",
        "start-events",
        "unix-ms:4",
    );

    // A Result submission is one atomic ledger rewrite carrying exactly two
    // canonical envelopes: the immutable report, and the `work` transition it
    // produced. A crash can never leave one without the other.
    let before_submit = store.canonical_operations().unwrap().len();
    let before_position = store.work_journal_position().unwrap();
    let report = result_report_for_test(
        &active,
        &member,
        "events-result",
        "done",
        vec!["artifact://events".into()],
        Vec::new(),
        "unix-ms:5",
    );
    let space = store.current_team_run_execution_space(&run).unwrap();
    let submit_context = MutationContext {
        execution_space_id: space.clone(),
        authenticated_actor: report.authored_by.clone(),
        authority_actor: None,
        command_name: "test.work_report.create".into(),
        idempotency_key: report.id.clone(),
        expected_version: 0,
        request_fingerprint: None,
    };
    let team_id = active.accountable_team_id.clone().unwrap();
    store
        .create_trust_work_report(&submit_context, &team_id, report.clone())
        .unwrap();

    let after_submit = store.canonical_operations().unwrap();
    assert_eq!(
        after_submit.len(),
        before_submit + 2,
        "one report envelope and one paired `work` transition"
    );
    let pair = &after_submit[after_submit.len() - 2..];
    assert_eq!(pair[0].event.aggregate_kind, "work_report");
    assert_eq!(pair[0].event.transition, "created");
    assert_eq!(pair[1].event.aggregate_kind, "work");
    assert_eq!(pair[1].event.transition, "submitted");
    assert_eq!(pair[1].event.aggregate_id, active.id);
    assert_eq!(pair[1].event.expected_version, active.version);
    assert_eq!(pair[1].event.resulting_version, report.work_revision);
    assert_eq!(
        pair[1].event.store_sequence,
        pair[0].event.store_sequence + 1,
        "the pair is one contiguous write"
    );
    assert_ne!(
        pair[1].event.idempotency_key, pair[0].event.idempotency_key,
        "the paired envelope derives its key, so every scan that resolves a \
         command by its exact key still finds exactly the report"
    );

    // The submission's Work revision is a named Work event now.
    let submitted = store
        .current_work(&active.id)
        .unwrap()
        .expect("submitted Work");
    assert_eq!(submitted.phase, WorkPhase::Review);
    let history = store.work_history(&active.id).unwrap();
    assert_eq!(
        history
            .iter()
            .map(|record| record.event.kind)
            .collect::<Vec<_>>(),
        [
            WorkEventKind::Created,
            WorkEventKind::Assigned,
            WorkEventKind::Started,
            WorkEventKind::Submitted,
        ],
        "one Work's history spans both journals in version order"
    );
    let submitted_event = history.last().expect("submitted record");
    assert_eq!(submitted_event.work.version, submitted.version);
    assert_eq!(submitted_event.event.work_id, active.id);
    assert_eq!(submitted_event.event.team_run_id, run.id);

    // The Work journal position advanced on the trust journal only.
    let after_position = store.work_journal_position().unwrap();
    assert_eq!(after_position.ledger, before_position.ledger);
    assert_eq!(after_position.trust, before_position.trust + 1);

    // An exact replay re-appends neither envelope.
    store
        .create_trust_work_report(&submit_context, &team_id, report.clone())
        .unwrap();
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        after_submit.len(),
        "the same idempotency key commits the same two envelopes, once"
    );
    assert_eq!(store.work_journal_position().unwrap(), after_position);

    // Acceptance is the second trust transition and carries its own event.
    accept_result_for_test(
        &store,
        &submitted,
        "events-result",
        "accept-events",
        "unix-ms:6",
    );
    let accepted_history = store.work_history(&active.id).unwrap();
    let accepted = accepted_history.last().expect("accepted record");
    assert_eq!(accepted.event.kind, WorkEventKind::Accepted);
    assert_eq!(accepted.event.expected_version, submitted.version);
    assert_eq!(accepted.work.phase, WorkPhase::Closed);
    assert_eq!(accepted.work.resolution, Some(WorkResolution::Accepted));
    assert!(
        store
            .work_events()
            .unwrap()
            .iter()
            .any(|event| event.work_id == active.id && event.kind == WorkEventKind::Accepted),
        "the store-wide event read names the acceptance"
    );
    drop(other);
    drop(root);
}

/// The paired `work` transition derives its idempotency key from the caller's
/// by appending `#<transition>`, so `#` is reserved. A caller key containing it
/// could collide with a derived key and be answered by the replay check with
/// "idempotent replay changed aggregate identity" — a refusal that names the
/// wrong cause. The separator is refused where the real reason is nameable.
#[test]
fn a_caller_idempotency_key_may_not_use_the_reserved_paired_separator() {
    let (root, store, run, member, other) = work_test_fixture("reserved-paired-key");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "reserved"),
            host_work_context("create-reserved", "create-reserved", "unix-ms:2"),
        )
        .unwrap();
    let work = assign_test_work_to_member(
        &store,
        &run,
        &work,
        &member,
        "assign-reserved",
        "assign-reserved",
        "unix-ms:3",
    );
    let active = start_claimed_work_for_test(
        &store,
        &work,
        &member,
        "start-reserved",
        "start-reserved",
        "unix-ms:4",
    );
    let mut report = result_report_for_test(
        &active,
        &member,
        "reserved-result",
        "done",
        vec!["artifact://reserved".into()],
        Vec::new(),
        "unix-ms:5",
    );
    report.id = "work-report:reserved".into();
    let space = store.current_team_run_execution_space(&run).unwrap();
    let before = store.canonical_operations().unwrap().len();
    let error = store
        .create_trust_work_report(
            &MutationContext {
                execution_space_id: space,
                authenticated_actor: report.authored_by.clone(),
                authority_actor: None,
                command_name: "test.work_report.create".into(),
                idempotency_key: "submission#submitted".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            active.accountable_team_id.as_deref().unwrap(),
            report,
        )
        .expect_err("a reserved separator in the caller key is refused");
    let message = error.to_string();
    assert!(
        message.contains("idempotency key must not contain"),
        "the refusal must name the real cause, not a replay identity change: {message}"
    );
    assert!(
        !message.contains("changed aggregate identity"),
        "the misleading replay message must not be what the caller sees: {message}"
    );
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        before,
        "a refused submission commits neither envelope"
    );
    drop(other);
    drop(root);
}
