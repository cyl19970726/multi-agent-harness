use firm_core::{Work, WorkEvent, WorkStatus};
use serde_json::json;

#[test]
fn work_status_is_exactly_the_six_state_contract() {
    let statuses = [
        WorkStatus::Open,
        WorkStatus::InProgress,
        WorkStatus::Blocked,
        WorkStatus::Review,
        WorkStatus::Done,
        WorkStatus::Cancelled,
    ];

    for status in statuses {
        let wire_name = work_status_wire_name(status);
        assert_eq!(serde_json::to_value(status).unwrap(), json!(wire_name));
        assert_eq!(
            serde_json::from_value::<WorkStatus>(json!(wire_name)).unwrap(),
            status
        );
    }

    assert!(
        serde_json::from_value::<WorkStatus>(json!("orphaned")).is_err(),
        "orphaned must not become a seventh Work status"
    );
}

#[test]
fn legacy_work_omitting_optional_fields_remains_readable() {
    let work: Work = serde_json::from_value(legacy_work_json()).expect("legacy Work");

    assert_eq!(work.status, WorkStatus::Open);
    assert!(work.team_id.is_none());
    assert!(work.created_by_member_id.is_none());
    assert!(work.parent_work_id.is_none());
    assert!(work.source_work_item_ref.is_none());
    assert!(work.owner_member_id.is_none());
    assert!(work.active_member_run_id.is_none());
    assert!(work.eligible_member_ids.is_empty());
    assert!(work.prerequisite_work_ids.is_empty());
    assert!(work.artifact_refs.is_empty());
    assert!(work.check_refs.is_empty());
    assert!(work.github_links.is_empty());
    assert!(work.gates.is_empty());
    assert!(work.workspace.is_none());
}

#[test]
fn legacy_event_defaults_optional_fields() {
    let event_json = json!({
        "id": "work-event-legacy-1",
        "team_run_id": "team-run-legacy-1",
        "work_id": "work-legacy-1",
        "sequence": 1,
        "kind": "created",
        "expected_version": 0,
        "resulting_version": 1,
        "performed_by_actor": { "kind": "host", "id": "host" },
        "idempotency_key": "create-work-legacy-1",
        "created_at": "unix-ms:1"
    });
    let event: WorkEvent = serde_json::from_value(event_json.clone()).expect("legacy WorkEvent");
    assert!(event.authority_actor.is_none());
    assert!(event.causation_ref.is_none());
    assert_eq!(event.payload, serde_json::Value::Null);
}

fn work_status_wire_name(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Open => "open",
        WorkStatus::InProgress => "in_progress",
        WorkStatus::Blocked => "blocked",
        WorkStatus::Review => "review",
        WorkStatus::Done => "done",
        WorkStatus::Cancelled => "cancelled",
    }
}

fn legacy_work_json() -> serde_json::Value {
    json!({
        "id": "work-legacy-1",
        "team_run_id": "team-run-legacy-1",
        "title": "Read a pre-extension Work row",
        "context_markdown": "Compatibility fixture",
        "completion_criteria_markdown": "The row replays without migration",
        "status": "open",
        "claim_mode": "team_claim",
        "priority": "normal",
        "created_by_actor": { "kind": "host", "id": "host" },
        "version": 1,
        "created_at": "unix-ms:1",
        "updated_at": "unix-ms:1"
    })
}
