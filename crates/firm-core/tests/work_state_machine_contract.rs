use firm_core::{
    TeamActorKind, TeamActorRef, Validate, Work, WorkCondition, WorkDecisionKind, WorkEvent,
    WorkOperationalDecision, WorkPhase, WorkReport, WorkResolution,
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
        (WorkCondition::OnHold, "on_hold"),
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
        (WorkResolution::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(resolution).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<WorkResolution>(json!(wire)).unwrap(),
            resolution
        );
    }

    assert!(serde_json::from_value::<WorkPhase>(json!("blocked")).is_err());
    assert!(serde_json::from_value::<WorkCondition>(json!("done")).is_err());
    assert!(serde_json::from_value::<WorkResolution>(json!("open")).is_err());
}

#[test]
fn closed_work_requires_normal_condition_and_resolution() {
    let mut work: Work = serde_json::from_value(work_json()).expect("canonical Work");
    work.validate().expect("accepted Work is valid");

    work.resolution = None;
    assert!(work.validate().is_err(), "closed Work needs a resolution");

    work.resolution = Some(WorkResolution::Failed);
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
        source_work_version: 3,
        report_revision: 1,
        submitted_by_actor: host_actor(),
        base_revision: Some("base-sha".into()),
        candidate_revision: Some("candidate-sha".into()),
        result_summary: "implemented".into(),
        artifact_refs: vec!["diff:1".into()],
        check_refs: vec!["test:1".into()],
        evidence_refs: vec!["evidence:1".into()],
        known_risks: Vec::new(),
        created_at: "unix-ms:2".into(),
    };
    report.validate().expect("bound report");
    report.candidate_revision = None;
    assert!(report.validate().is_err(), "half-bound report must fail");
}

#[test]
fn accept_and_revise_decisions_require_an_exact_report() {
    let mut decision = WorkOperationalDecision {
        id: "decision-1".into(),
        work_id: "work-1".into(),
        expected_work_version: 3,
        kind: WorkDecisionKind::Accept,
        decided_by_actor: host_actor(),
        rationale: "all declared gates passed".into(),
        work_report_id: Some("report-1".into()),
        gate_requirement_ref: None,
        failure_analysis_ref: None,
        evidence_refs: vec!["gate-evaluation:1".into()],
        created_at: "unix-ms:3".into(),
    };
    decision.validate().expect("report-bound acceptance");
    decision.work_report_id = None;
    assert!(
        decision.validate().is_err(),
        "accept cannot float free of a report"
    );
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
