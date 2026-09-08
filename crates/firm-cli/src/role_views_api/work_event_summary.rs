//! Display-only Work history across the two persisted operation formats.
//! Execution authority continues to use its existing canonical binding reads.
use super::*;
use harness_core::agentfirm_api::CanonicalOperation;
use harness_core::WorkOperation;

pub(super) fn display_events(
    work_operations: &[WorkOperation],
    canonical_operations: &[CanonicalOperation],
) -> Vec<Value> {
    let mut events = work_operations
        .iter()
        .map(|operation| serde_json::to_value(&operation.event).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    for operation in canonical_operations {
        let event = &operation.event;
        if event.aggregate_kind == "work" {
            if let Ok(work) = serde_json::from_value::<Work>(operation.resulting_projection.clone())
            {
                if work.id == event.aggregate_id && work.version == event.resulting_version {
                    events.push(canonical_display_event(operation, &work, &event.transition));
                }
            }
        }
        // Result reports submit a Work through an immutable side projection.
        // Keep the actual report event label: its `created` is not Work creation.
        if event.aggregate_kind == "work_report" && event.transition == "created" {
            for value in &operation.immutable_side_records {
                if let Ok(work) = serde_json::from_value::<Work>(value.clone()) {
                    if operation.resulting_projection["work_id"] == work.id {
                        events.push(canonical_display_event(operation, &work, "report_created"));
                    }
                }
            }
        }
    }
    events
}

fn canonical_display_event(operation: &CanonicalOperation, work: &Work, kind: &str) -> Value {
    json!({
        "id":operation.event.id,
        "work_id":work.id,
        "kind":kind,
        "resulting_version":work.version,
        "performed_by_actor":operation.event.performed_by_actor,
        // Work's persisted update time shares the ISO timestamp format of
        // ordinary WorkEvents; trust event timestamps use `unix-ms:` instead.
        "created_at":work.updated_at,
        "payload":operation.event.payload,
    })
}

pub(super) fn latest_event<'a>(events: &'a [Value], work_id: &str) -> Option<&'a Value> {
    events
        .iter()
        .filter(|event| event["work_id"] == work_id)
        // Sequence is scoped to its source ledger, not comparable across them.
        .max_by_key(|event| event["resulting_version"].as_u64().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(version: u64) -> Work {
        let draft = harness_core::CurrentWorkDraft::new(
            "work-a".into(),
            "run-a".into(),
            "team-a".into(),
            "Work".into(),
            String::new(),
            "Evidence".into(),
            harness_core::WorkClaimMode::HostAssign,
            harness_core::WorkPriority::Normal,
            harness_core::TeamActorRef {
                kind: harness_core::TeamActorKind::Host,
                id: "host".into(),
                display_name: None,
                authn_source: None,
            },
            "2026-09-07T13:10:25.134Z".into(),
        );
        let mut work = Work::from_current_draft(draft);
        work.version = version;
        work
    }

    fn operation(kind: &str, transition: &str, version: u64) -> CanonicalOperation {
        serde_json::from_value(json!({
            "event": {"id":format!("event-{version}"),"aggregate_kind":kind,
                "aggregate_id":if kind=="work" {"work-a"} else {"report-a"},
                "sequence":1,"store_sequence":100+version,"transition":transition,
                "expected_version":version-1,"resulting_version":version,
                "performed_by_actor":{"kind":"agent_member","id":"host"},
                "idempotency_key":format!("key-{version}"),"canonical_request_fingerprint":"fingerprint",
                "payload":{},"created_at":"unix-ms:1788786625885"},
            "resulting_projection":if kind=="work" {serde_json::to_value(work(version)).unwrap()} else {json!({"id":"report-a","work_id":"work-a"})},
            "immutable_side_records":if kind=="work_report" {vec![serde_json::to_value(work(4)).unwrap()]} else {vec![]}
        })).unwrap()
    }

    #[test]
    fn latest_work_event_follows_work_versions_across_operation_sources() {
        let report = operation("work_report", "created", 1);
        let accepted = operation("work", "accepted", 5);
        let mut events = display_events(&[], &[accepted, report]);
        events.push(json!({"id":"started","work_id":"work-a","kind":"started",
            "sequence":3,"resulting_version":3}));
        assert_eq!(latest_event(&events, "work-a").unwrap()["kind"], "accepted");
        assert!(events
            .iter()
            .any(|event| event["kind"] == "report_created" && event["resulting_version"] == 4));
        events.retain(|event| event["kind"] != "accepted");
        assert_eq!(
            latest_event(&events, "work-a").unwrap()["kind"],
            "report_created"
        );
        assert!(latest_event(&events, "other-work").is_none());
    }

    #[test]
    fn unrelated_report_side_projection_is_not_a_work_event() {
        let mut report = operation("work_report", "created", 1);
        report.resulting_projection["work_id"] = json!("other-work");
        assert!(display_events(&[], &[report]).is_empty());
    }
}
