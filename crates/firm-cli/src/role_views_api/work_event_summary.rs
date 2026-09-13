//! Display-only Work history, read from the one Store Work journal.
//!
//! Execution authority continues to use its existing canonical binding reads.
//! Before W3 this module folded the two persisted operation formats itself and
//! had to label a Result submission `report_created`, because the Review
//! revision existed only as a side record of the report envelope and no
//! journal named the transition that produced it. The Store reader now
//! materializes every revision from both journals as one event, so the display
//! label is simply the event kind.
use super::*;
use harness_store::WorkJournalRecord;

pub(super) fn display_events(records: &[WorkJournalRecord]) -> Vec<Value> {
    records
        .iter()
        .map(|record| {
            json!({
                "id": record.event.id,
                "work_id": record.work.id,
                "kind": work_event_kind_label(&record.event.kind),
                "resulting_version": record.event.resulting_version,
                "performed_by_actor": record.event.performed_by_actor,
                "created_at": record.event.created_at,
                "payload": record.event.payload,
            })
        })
        .collect()
}

fn work_event_kind_label(kind: &harness_core::WorkEventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub(super) fn latest_event<'a>(events: &'a [Value], work_id: &str) -> Option<&'a Value> {
    events
        .iter()
        .filter(|event| event["work_id"] == work_id)
        // Sequence is scoped to its source journal, not comparable across
        // them; the Work version chain is the one order both journals share.
        .max_by_key(|event| event["resulting_version"].as_u64().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::{WorkEvent, WorkEventKind};
    use harness_store::WorkJournalSource;

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

    fn record(source: WorkJournalSource, kind: WorkEventKind, version: u64) -> WorkJournalRecord {
        WorkJournalRecord {
            source,
            execution_space_id: matches!(source, WorkJournalSource::Trust)
                .then(|| "space-a".to_string()),
            event: WorkEvent {
                id: format!("event-{version}"),
                team_run_id: "run-a".into(),
                work_id: "work-a".into(),
                sequence: version,
                kind,
                expected_version: version - 1,
                resulting_version: version,
                performed_by_actor: harness_core::TeamActorRef {
                    kind: harness_core::TeamActorKind::AgentMember,
                    id: "host".into(),
                    display_name: None,
                    authn_source: None,
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("key-{version}"),
                payload: json!({}),
                created_at: "unix-ms:1788786625885".into(),
            },
            work: work(version),
        }
    }

    #[test]
    fn latest_work_event_follows_work_versions_across_journals() {
        let events = display_events(&[
            record(WorkJournalSource::Ledger, WorkEventKind::Started, 3),
            record(WorkJournalSource::Trust, WorkEventKind::Submitted, 4),
            record(WorkJournalSource::Trust, WorkEventKind::Accepted, 5),
        ]);
        assert_eq!(latest_event(&events, "work-a").unwrap()["kind"], "accepted");
        assert!(events
            .iter()
            .any(|event| event["kind"] == "submitted" && event["resulting_version"] == 4));
        assert!(latest_event(&events, "other-work").is_none());
    }

    #[test]
    fn every_journal_record_is_one_display_event() {
        assert_eq!(display_events(&[]).len(), 0);
        assert_eq!(
            display_events(&[record(
                WorkJournalSource::Trust,
                WorkEventKind::Cancelled,
                2
            )])
            .len(),
            1
        );
    }
}
