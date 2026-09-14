use firm_core::{
    TeamActorKind, TeamActorRef, Validate, Work, WorkCondition, WorkEvent, WorkPhase, WorkReport,
    WorkResolution,
};
use serde_json::json;

#[test]
fn work_lifecycle_axes_have_exact_wire_contracts() {
    for (phase, wire) in [
        (WorkPhase::Open, "open"),
        (WorkPhase::Active, "active"),
        (WorkPhase::Review, "review"),
        (WorkPhase::Closed, "closed"),
    ] {
        assert_eq!(serde_json::to_value(phase).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<WorkPhase>(json!(wire)).unwrap(),
            phase
        );
    }
    for (condition, wire) in [
        (WorkCondition::Normal, "normal"),
        (WorkCondition::Blocked, "blocked"),
    ] {
        assert_eq!(serde_json::to_value(condition).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<WorkCondition>(json!(wire)).unwrap(),
            condition
        );
    }
    for (resolution, wire) in [
        (WorkResolution::Accepted, "accepted"),
        (WorkResolution::Cancelled, "cancelled"),
    ] {
        assert_eq!(serde_json::to_value(resolution).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<WorkResolution>(json!(wire)).unwrap(),
            resolution
        );
    }

    assert!(serde_json::from_value::<WorkPhase>(json!("blocked")).is_err());
    assert!(serde_json::from_value::<WorkCondition>(json!("done")).is_err());
    assert!(
        serde_json::from_value::<WorkCondition>(json!("on_hold")).is_err(),
        "the retired on_hold condition has no writer and no reader"
    );
    assert!(
        serde_json::from_value::<WorkResolution>(json!("failed")).is_err(),
        "the retired failed resolution has no writer and no reader"
    );
    assert!(serde_json::from_value::<WorkResolution>(json!("open")).is_err());
}

#[test]
fn closed_work_requires_normal_condition_and_resolution() {
    let mut work: Work = serde_json::from_value(work_json()).expect("canonical Work");
    work.validate().expect("accepted Work is valid");

    work.resolution = None;
    assert!(work.validate().is_err(), "closed Work needs a resolution");

    work.resolution = Some(WorkResolution::Cancelled);
    work.condition = WorkCondition::Blocked;
    assert!(
        work.validate().is_err(),
        "closed Work cannot remain blocked"
    );

    work.phase = WorkPhase::Review;
    work.condition = WorkCondition::Normal;
    assert!(
        work.validate().is_err(),
        "open lifecycle cannot carry resolution"
    );
}

#[test]
fn event_defaults_optional_provenance_fields() {
    let event_json = json!({
        "id": "work-event-1",
        "team_run_id": "team-run-1",
        "work_id": "work-1",
        "sequence": 1,
        "kind": "created",
        "expected_version": 0,
        "resulting_version": 1,
        "performed_by_actor": { "kind": "host", "id": "host" },
        "idempotency_key": "create-work-1",
        "created_at": "unix-ms:1"
    });
    let event: WorkEvent = serde_json::from_value(event_json).expect("WorkEvent");
    assert!(event.authority_actor.is_none());
    assert!(event.causation_ref.is_none());
    assert_eq!(event.payload, serde_json::Value::Null);
}

#[test]
fn report_revision_binding_is_exact() {
    let mut report = WorkReport {
        id: "report-1".into(),
        work_id: "work-1".into(),
        work_version: 3,
        report_revision: 1,
        submitted_by_actor: host_actor(),
        base_revision: Some("base-sha".into()),
        candidate_revision: "candidate-sha".into(),
        result_summary: "implemented".into(),
        artifact_refs: vec!["diff:1".into()],
        check_refs: vec!["test:1".into()],
        evidence_refs: vec!["evidence:1".into()],
        known_risks: Vec::new(),
        created_at: "unix-ms:2".into(),
    };
    report.validate().expect("bound report");
    report.candidate_revision.clear();
    assert!(report.validate().is_err(), "unbound report must fail");
}

/// Every WorkOperation row written before the operational-decision record was
/// retired (#944) carries `"decisions": []`, and the Wave 5/6 acceptance
/// evidence checked into `docs/current/operations/evidence/` carries it too.
/// `WorkOperation` is not `deny_unknown_fields`, so those rows must still fold
/// -- the retirement removes a writerless record, never the ability to read
/// history.
#[test]
fn legacy_rows_carrying_a_retired_decisions_array_still_decode() {
    let row = serde_json::json!({
        "event": {
            "id": "work-event-1",
            "team_run_id": "run-1",
            "work_id": "work-1",
            "sequence": 1,
            "kind": "created",
            "expected_version": 0,
            "resulting_version": 1,
            "performed_by_actor": {"kind": "host", "id": "host"},
            "idempotency_key": "create-1",
            "payload": null,
            "created_at": "unix-ms:1",
        },
        "work": {
            "id": "work-1",
            "team_run_id": "run-1",
            "accountable_team_id": "team-1",
            "title": "legacy row",
            "context_markdown": "",
            "completion_criteria_markdown": "folds",
            "phase": "open",
            "condition": "normal",
            "claim_mode": "host_assign",
            "priority": "normal",
            "created_by_actor": {"kind": "host", "id": "host"},
            "version": 1,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
        },
        "condition_records": [],
        "reports": [],
        "evidence_records": [],
        // The retired field, in both shapes history holds.
        "decisions": [{
            "id": "decision-1",
            "work_id": "work-1",
            "expected_work_version": 1,
            "kind": "accept",
            "decided_by_actor": {"kind": "host", "id": "host"},
            "rationale": "all declared gates passed",
            "work_report_id": "report-1",
            "evidence_refs": [],
            "created_at": "unix-ms:2",
        }],
        "delegation_revisions": [],
    });
    let operation: firm_core::WorkOperation =
        serde_json::from_value(row).expect("a legacy row with decisions still folds");
    assert_eq!(operation.work.id, "work-1");
    operation.work.validate().expect("and its Work is current");

    let mut empty = serde_json::to_value(&operation).expect("re-serialize");
    assert!(
        empty.get("decisions").is_none(),
        "but nothing writes the field back"
    );
    empty["decisions"] = serde_json::json!([]);
    serde_json::from_value::<firm_core::WorkOperation>(empty)
        .expect("the empty array every current row carries folds too");
}

fn host_actor() -> TeamActorRef {
    TeamActorRef {
        kind: TeamActorKind::Host,
        id: "host".into(),
        display_name: None,
        authn_source: None,
    }
}

fn work_json() -> serde_json::Value {
    json!({
        "id": "work-1",
        "team_run_id": "team-run-1",
        "accountable_team_id": "team-1",
        "title": "Verify canonical lifecycle",
        "context_markdown": "No compatibility status field",
        "completion_criteria_markdown": "All lifecycle invariants hold",
        "phase": "closed",
        "condition": "normal",
        "resolution": "accepted",
        "claim_mode": "team_claim",
        "priority": "normal",
        "created_by_actor": { "kind": "host", "id": "host" },
        "version": 1,
        "created_at": "unix-ms:1",
        "updated_at": "unix-ms:1"
    })
}
