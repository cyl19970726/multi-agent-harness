//! W4 — one journal for Work.
//!
//! Every Work transition commits one `work` aggregate envelope carrying its
//! complete WorkOperation. `work_operations.jsonl` has no writer left: a store
//! created after this slice never gains the file, and a store that already has
//! one keeps reading it and never grows it.
use super::*;

/// Every persisted revision of one Work, as (transition name, the WorkOperation
/// the envelope carries), in store order.
fn committed_work_envelopes(
    store: &HarnessStore,
    work_id: &str,
) -> Vec<(String, firm_core::WorkOperation)> {
    store
        .canonical_operations()
        .expect("canonical operations")
        .into_iter()
        .filter(|operation| {
            operation.event.aggregate_kind == "work" && operation.event.aggregate_id == work_id
        })
        .map(|operation| {
            let transition = operation.event.transition.clone();
            let carried = operation
                .immutable_side_records
                .iter()
                .find_map(|record| {
                    serde_json::from_value::<firm_core::WorkOperation>(record.clone()).ok()
                })
                .unwrap_or_else(|| {
                    panic!("`work`/{transition} must carry its WorkOperation side record")
                });
            (transition, carried)
        })
        .collect()
}

#[test]
fn every_work_transition_commits_one_work_envelope_and_no_ledger_row() {
    let (root, store, run, member, _) = work_test_fixture("work-journal-cutover");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-cutover"),
            host_work_context("create-cutover", "create-cutover", "unix-ms:2"),
        )
        .expect("create");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "assign-cutover",
        "assign-cutover",
        "unix-ms:3",
    );
    let started = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "start-cutover",
        "start-cutover",
        "unix-ms:4",
    );
    let blocked = store
        .block_work(
            &started.id,
            started.version,
            &member.id,
            "waiting on a decision",
            member_work_context(&member.id, "block-cutover", "block-cutover", "unix-ms:5"),
        )
        .expect("block");
    let resumed = store
        .resume_work(
            &blocked.id,
            blocked.version,
            &member.id,
            "decision recorded",
            member_work_context(&member.id, "resume-cutover", "resume-cutover", "unix-ms:6"),
        )
        .expect("resume");

    assert!(
        !root.join("work_operations.jsonl").exists(),
        "no Work writer appends to the legacy ledger file"
    );
    let envelopes = committed_work_envelopes(&store, &created.id);
    assert_eq!(
        envelopes
            .iter()
            .map(|(transition, _)| transition.as_str())
            .collect::<Vec<_>>(),
        ["created", "assigned", "started", "blocked", "resumed"],
        "one envelope per transition, named by the WorkEventKind"
    );
    for (index, (transition, operation)) in envelopes.iter().enumerate() {
        let version = index as u64 + 1;
        assert_eq!(operation.work.version, version, "{transition}");
        assert_eq!(operation.event.resulting_version, version, "{transition}");
        assert_eq!(
            operation.event.kind.canonical_transition(),
            transition,
            "the side record's event kind is the envelope's transition"
        );
    }
    // The per-row records the ledger used to carry ride in the same envelope.
    let blocked_operation = &envelopes[3].1;
    assert_eq!(
        blocked_operation.condition_records.len(),
        1,
        "the blocker's condition record is committed with its transition"
    );
    assert_eq!(
        store
            .work_condition_records()
            .expect("condition records")
            .len(),
        2,
        "and both the blocker and its resolution read back"
    );

    // The one reader sees the same chain the writer committed.
    assert_eq!(
        store
            .work_history(&created.id)
            .expect("history")
            .iter()
            .map(|record| record.event.kind)
            .collect::<Vec<_>>(),
        [
            WorkEventKind::Created,
            WorkEventKind::Assigned,
            WorkEventKind::Started,
            WorkEventKind::Blocked,
            WorkEventKind::Resumed,
        ]
    );
    let position = store.work_journal_position().expect("position");
    assert_eq!(position.ledger, 0, "the ledger component never advances");
    assert_eq!(position.trust, 5);
    assert_eq!(
        store.current_work(&created.id).expect("current"),
        Some(resumed)
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn a_replayed_command_appends_nothing_and_a_changed_one_is_refused() {
    let (root, store, run, _member, _) = work_test_fixture("work-journal-replay");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-replay"),
            host_work_context("create-replay", "create-replay", "unix-ms:2"),
        )
        .expect("create");
    let cancel = |key: &str, reason: &str| {
        store.cancel_work(
            &created.id,
            created.version,
            reason,
            host_work_context("cancel-replay", key, "unix-ms:3"),
        )
    };
    let cancelled = cancel("cancel-replay", "superseded").expect("cancel");
    let before = store.canonical_operations().expect("operations").len();
    assert_eq!(
        cancel("cancel-replay", "superseded").expect("exact replay"),
        cancelled,
        "an exact replay returns the committed result"
    );
    assert_eq!(
        store.canonical_operations().expect("operations").len(),
        before,
        "and appends nothing"
    );
    let reused = cancel("cancel-replay", "a different reason")
        .expect_err("the same key with different content is a reuse refusal");
    assert!(
        reused.to_string().contains("IDEMPOTENCY_KEY_REUSED"),
        "unexpected error: {reused}"
    );
    assert_eq!(
        store.canonical_operations().expect("operations").len(),
        before,
        "a refused reuse appends nothing either"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn a_caller_key_that_could_collide_with_a_derived_paired_key_is_refused() {
    let (root, store, run, _member, _) = work_test_fixture("work-journal-reserved-key");
    let refused = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-reserved"),
            host_work_context("create-reserved", "create#submitted", "unix-ms:2"),
        )
        .expect_err("a reserved separator in a caller key is refused");
    assert!(
        refused
            .to_string()
            .contains("idempotency_key must not contain"),
        "unexpected error: {refused}"
    );
    assert!(
        store.latest_works().expect("works").is_empty(),
        "the refusal happens before anything is committed"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn a_member_write_persists_its_agent_member_and_records_the_member_run_as_evidence() {
    let (root, store, run, member, _) = work_test_fixture("work-journal-actor");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-actor"),
            host_work_context("create-actor", "create-actor", "unix-ms:2"),
        )
        .expect("create");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "assign-actor",
        "assign-actor",
        "unix-ms:3",
    );
    let started = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "start-actor",
        "start-actor",
        "unix-ms:4",
    );
    let event = |kind: WorkEventKind| {
        store
            .work_history(&started.id)
            .expect("history")
            .into_iter()
            .find(|record| record.event.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} revision"))
            .event
    };

    let host_event = event(WorkEventKind::Assigned);
    assert_eq!(
        host_event.performed_by_actor.kind,
        TeamActorKind::Host,
        "a Host write keeps the Host kind every Host predicate reads"
    );
    assert_eq!(host_event.executing_member_run_id(), None);

    let member_event = event(WorkEventKind::Started);
    assert_eq!(
        member_event.performed_by_actor.kind,
        TeamActorKind::AgentMember,
        "a member write persists the durable identity, not the runtime generation"
    );
    assert_eq!(member_event.performed_by_actor.id, member.agent_member_id);
    assert_eq!(
        member_event.performed_by_actor.authn_source,
        Some("bound-runtime:test".to_string()),
        "the caller's authn source is preserved"
    );
    assert_eq!(
        member_event.executed_by_member_run_id.as_deref(),
        Some(member.id.as_str()),
        "the MemberRun is evidence in its own field"
    );
    assert_eq!(
        member_event.executing_member_run_id(),
        Some(member.id.as_str())
    );

    // A legacy row signed the runtime generation as the performer itself, and
    // the same accessor must still read it.
    let legacy = WorkEvent {
        executed_by_member_run_id: None,
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: member.id.clone(),
            display_name: None,
            authn_source: None,
        },
        ..member_event.clone()
    };
    assert_eq!(legacy.executing_member_run_id(), Some(member.id.as_str()));

    // The Host's wake carries the submitting runtime as evidence, from either
    // shape, and a Host-performed transition still raises none.
    let attention = store
        .host_attentions()
        .expect("HostAttentions")
        .into_iter()
        .find(|row| row.work_id == started.id && row.kind == HostAttentionKind::WorkChanged)
        .expect("the member's Start woke the Host");
    assert_eq!(attention.member_run_id.as_deref(), Some(member.id.as_str()));
    assert!(
        !store
            .host_attentions()
            .expect("HostAttentions")
            .iter()
            .any(|row| row.source_event_ref == host_event.id),
        "a Host-performed transition never wakes the Host about itself"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn one_revision_persisted_twice_with_different_content_is_a_conflict_not_a_tie() {
    let (root, store, run, _member, _) = work_test_fixture("work-journal-divergence");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-divergence"),
            host_work_context("create-divergence", "create-divergence", "unix-ms:2"),
        )
        .expect("create");

    // The same revision, in the legacy shape, saying something else.
    let mut divergent = created.clone();
    divergent.title = "a second account of revision 1".into();
    let operation = WorkOperation {
        event: WorkEvent {
            id: "legacy-divergent-event".into(),
            team_run_id: run.id.clone(),
            work_id: created.id.clone(),
            sequence: 1,
            kind: WorkEventKind::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: created.created_by_actor.clone(),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: "legacy-divergent-key".into(),
            payload: serde_json::Value::Null,
            created_at: "unix-ms:2".into(),
            executed_by_member_run_id: None,
        },
        work: divergent,
        condition_records: Vec::new(),
        reports: Vec::new(),
        evidence_records: Vec::new(),
    };
    store
        .append_jsonl("work_operations.jsonl", &operation)
        .expect("stage the divergent legacy row");

    let refused = store
        .latest_works()
        .expect_err("two accounts of one revision cannot be tie-broken");
    assert!(
        refused
            .to_string()
            .contains("WORK_JOURNAL_REVISION_CONFLICT"),
        "unexpected error: {refused}"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn only_the_proven_peer_reviewer_reauthorizes_the_next_execution_admission() {
    let base = WorkEvent {
        id: "review-event".into(),
        team_run_id: "run".into(),
        work_id: "work".into(),
        sequence: 4,
        kind: WorkEventKind::ChangesRequested,
        expected_version: 3,
        resulting_version: 4,
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::AgentMember,
            id: "agent-peer".into(),
            display_name: None,
            authn_source: None,
        },
        authority_actor: None,
        causation_ref: None,
        idempotency_key: "review".into(),
        payload: serde_json::Value::Null,
        created_at: "unix-ms:9".into(),
        executed_by_member_run_id: Some("mr-peer".into()),
    };
    let reauthorizes = HarnessStore::work_event_reauthorizes_execution;

    assert!(
        !reauthorizes(&base),
        "\"some AgentMember requested changes\" is not a proven peer review"
    );
    let marked = WorkEvent {
        payload: serde_json::json!({
            crate::store_work_journal_writer::PEER_REVIEW_MARKER: true,
        }),
        ..base.clone()
    };
    assert!(
        reauthorizes(&marked),
        "the marker only the peer gate writes does re-authorize"
    );
    let legacy = WorkEvent {
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::ProviderRuntimeProjection,
            id: "mr-peer".into(),
            display_name: None,
            authn_source: None,
        },
        executed_by_member_run_id: None,
        ..base.clone()
    };
    assert!(
        reauthorizes(&legacy),
        "and so does the legacy shape, whose performer WAS the reviewing runtime"
    );
    assert!(
        !reauthorizes(&WorkEvent {
            kind: WorkEventKind::Submitted,
            ..marked.clone()
        }),
        "the marker never widens past ChangesRequested"
    );
    assert!(
        reauthorizes(&WorkEvent {
            kind: WorkEventKind::Rebound,
            performed_by_actor: TeamActorRef {
                kind: TeamActorKind::Host,
                id: "agent-host".into(),
                display_name: None,
                authn_source: None,
            },
            ..base.clone()
        }),
        "the Host set is unchanged"
    );
}

#[test]
fn a_revision_event_id_is_stable_and_is_what_a_remote_ref_binds_to() {
    let (root, store, run, member, _) = work_test_fixture("work-journal-event-id");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-event-id"),
            host_work_context("create-event-id", "create-event-id", "unix-ms:2"),
        )
        .expect("create");
    let id_at = |version: u64| {
        store
            .work_history(&created.id)
            .expect("history")
            .into_iter()
            .find(|record| record.work.version == version)
            .expect("the revision")
            .event
            .id
    };
    let creation_event_id = id_at(1);
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "assign-event-id",
        "assign-event-id",
        "unix-ms:3",
    );
    start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "start-event-id",
        "start-event-id",
        "unix-ms:4",
    );
    assert_eq!(
        id_at(1),
        creation_event_id,
        "one revision, one `work` transition, one event id — later revisions never rebind it"
    );
    assert_eq!(
        committed_work_envelopes(&store, &created.id)
            .into_iter()
            .map(|(_, operation)| operation.event.id)
            .collect::<Vec<_>>(),
        vec![id_at(1), id_at(2), id_at(3)],
        "and the id the journal reports is the one the committed transition carries"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}

#[test]
fn a_caller_of_the_wrong_kind_cannot_replay_a_host_key() {
    let (root, store, run, member, _) = work_test_fixture("work-journal-replay-authority");
    let created = store
        .insert_work(
            unassigned_test_work(&run.id, "work-journal-replay-authority"),
            host_work_context("create-authority", "create-authority", "unix-ms:2"),
        )
        .expect("create");
    let assigned = assign_test_work_to_member(
        &store,
        &run,
        &created,
        &member,
        "assign-authority",
        "assign-authority",
        "unix-ms:3",
    );
    let started = start_claimed_work_for_test(
        &store,
        &assigned,
        &member,
        "start-authority",
        "start-authority",
        "unix-ms:4",
    );
    let blocked = store
        .block_work_as_host(
            &started.id,
            started.version,
            "Host paused it",
            host_work_context("block-authority", "block-authority", "unix-ms:5"),
        )
        .expect("the Host blocks its own Work");

    // The exact disclosure shape: a caller whose kind is NOT Host, presenting
    // the Host's key and request, whose canonical authenticated actor is
    // nonetheless identical to the Host's — `Host/agent-host` and
    // `AgentMember/agent-host` both canonicalise to `AgentMember/agent-host`,
    // so the replay lookup alone would match and hand back the committed Work.
    let mut impersonating = host_work_context("block-authority", "block-authority", "unix-ms:5");
    impersonating.performed_by_actor.kind = TeamActorKind::AgentMember;
    let refused = store
        .block_work_as_host(
            &started.id,
            started.version,
            "Host paused it",
            impersonating,
        )
        .expect_err("a non-Host caller kind is refused before the replay is looked up");
    assert!(
        refused.to_string().contains("Host authority is required"),
        "the caller-shape gate must answer first, not the replay: {refused}"
    );
    assert_eq!(
        store.current_work(&started.id).expect("current"),
        Some(blocked),
        "and the Work is untouched either way"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
