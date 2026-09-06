//! A Work acceptance is a reason to reconsider blocked responsibility, never
//! authority to resume it. This pure projection creates no Work or Message.

use crate::{Work, WorkCondition, WorkEvent, WorkEventKind, WorkPhase, WorkResolution};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const ACCEPTANCE_WAKE_SOURCE_PREFIX: &str = "work-acceptance-wake:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceWake {
    pub acceptance_event_id: String,
    pub accepted_work_id: String,
    pub blocked_work_ids: Vec<String>,
}

impl AcceptanceWake {
    pub fn source_record_id(&self) -> String {
        format!(
            "{ACCEPTANCE_WAKE_SOURCE_PREFIX}{}",
            self.acceptance_event_id
        )
    }
}

// CLI coordination records use unix-ms, while imported/API records can use
// RFC3339. Normalize both existing formats; never infer order from opaque text.
fn recorded_time(value: &str) -> Option<OffsetDateTime> {
    if let Some(millis) = value.strip_prefix("unix-ms:") {
        let nanos = millis.parse::<i128>().ok()?.checked_mul(1_000_000)?;
        OffsetDateTime::from_unix_timestamp_nanos(nanos).ok()
    } else {
        OffsetDateTime::parse(value, &Rfc3339).ok()
    }
}

/// Select the latest related acceptance after at least one current block.
/// Sequence numbers are only compared within one Work; cross-Work ordering
/// uses parsed recorded times, with missing/invalid evidence failing closed.
pub fn select_acceptance_wake(
    works: &[Work],
    events: &[WorkEvent],
    team_id: &str,
    membership_id: &str,
    member_id: &str,
) -> Option<AcceptanceWake> {
    let owned = |work: &&Work| {
        work.accountable_team_id.as_deref() == Some(team_id)
            && work.assignee_membership_id.as_deref() == Some(membership_id)
            && work.owner_member_id.as_deref() == Some(member_id)
    };
    let owned: Vec<_> = works.iter().filter(owned).collect();
    if owned.iter().any(|work| {
        work.condition == WorkCondition::Normal
            && matches!(work.phase, WorkPhase::Active | WorkPhase::Review)
    }) {
        return None;
    }
    let blocks: Vec<_> = owned
        .iter()
        .filter(|work| work.phase != WorkPhase::Closed && work.condition == WorkCondition::Blocked)
        .filter_map(|work| {
            let event = events
                .iter()
                .filter(|event| {
                    event.work_id == work.id
                        && event.kind == WorkEventKind::Blocked
                        && event.resulting_version <= work.version
                })
                .max_by_key(|event| event.sequence)?;
            let time = recorded_time(&event.created_at)?;
            Some((work.id.clone(), time))
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    let (event, _) = events
        .iter()
        .filter(|event| {
            event.kind == WorkEventKind::Accepted
                && event.performed_by_actor.id != member_id
                && owned.iter().any(|work| {
                    work.id == event.work_id
                        && work.phase == WorkPhase::Closed
                        && work.resolution == Some(WorkResolution::Accepted)
                        && work.version == event.resulting_version
                })
        })
        .filter_map(|event| Some((event, recorded_time(&event.created_at)?)))
        .filter(|(_, time)| blocks.iter().any(|(_, blocked_at)| time > blocked_at))
        .max_by(|(a, at), (b, bt)| at.cmp(bt).then_with(|| a.id.cmp(&b.id)))?;
    let accepted_at = recorded_time(&event.created_at)?;
    let mut blocked_work_ids: Vec<_> = blocks
        .into_iter()
        .filter(|(_, blocked_at)| accepted_at > *blocked_at)
        .map(|(id, _)| id)
        .collect();
    blocked_work_ids.sort();
    Some(AcceptanceWake {
        acceptance_event_id: event.id.clone(),
        accepted_work_id: event.work_id.clone(),
        blocked_work_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CurrentWorkDraft, TeamActorKind, TeamActorRef, WorkClaimMode, WorkPriority};

    fn actor(id: &str) -> TeamActorRef {
        TeamActorRef {
            kind: TeamActorKind::AgentMember,
            id: id.into(),
            display_name: None,
            authn_source: None,
        }
    }

    fn work(id: &str, phase: WorkPhase, condition: WorkCondition) -> Work {
        let mut work = CurrentWorkDraft::new(
            id.into(),
            "run".into(),
            "team".into(),
            id.into(),
            "context".into(),
            "criteria".into(),
            WorkClaimMode::HostAssign,
            WorkPriority::Normal,
            actor("host"),
            "2026-09-07T00:00:00Z".into(),
        )
        .into_work();
        work.assignee_membership_id = Some("membership".into());
        work.owner_member_id = Some("member".into());
        work.phase = phase;
        work.condition = condition;
        work.version = 4;
        if phase == WorkPhase::Closed {
            work.resolution = Some(WorkResolution::Accepted);
        }
        work
    }

    fn event(work: &Work, kind: WorkEventKind, time: &str) -> WorkEvent {
        WorkEvent {
            id: format!("{}-event", work.id),
            team_run_id: work.team_run_id.clone(),
            work_id: work.id.clone(),
            sequence: work.version,
            kind,
            expected_version: work.version - 1,
            resulting_version: work.version,
            performed_by_actor: actor("host"),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: format!("{}-key", work.id),
            payload: Default::default(),
            created_at: time.into(),
        }
    }

    fn fixture() -> (Vec<Work>, Vec<WorkEvent>) {
        let blocked = work("standing", WorkPhase::Active, WorkCondition::Blocked);
        let accepted = work("slice", WorkPhase::Closed, WorkCondition::Normal);
        let events = vec![
            event(&blocked, WorkEventKind::Blocked, "2026-09-07T00:00:01Z"),
            event(&accepted, WorkEventKind::Accepted, "2026-09-07T00:00:02Z"),
        ];
        (vec![blocked, accepted], events)
    }

    fn select(works: &[Work], events: &[WorkEvent]) -> Option<AcceptanceWake> {
        select_acceptance_wake(works, events, "team", "membership", "member")
    }

    #[test]
    fn related_host_acceptance_reconsiders_block_without_mutation() {
        let (works, events) = fixture();
        let original = works.clone();
        let wake = select(&works, &events).unwrap();
        assert_eq!(wake.blocked_work_ids, ["standing"]);
        assert_eq!(wake.source_record_id(), "work-acceptance-wake:slice-event");
        assert_eq!(works, original);
    }

    #[test]
    fn outstanding_normal_work_and_foreign_responsibility_do_not_wake() {
        let (works, events) = fixture();
        for phase in [WorkPhase::Active, WorkPhase::Review] {
            let mut pending = works.clone();
            pending.push(work("pending", phase, WorkCondition::Normal));
            assert!(select(&pending, &events).is_none());
        }
        for field in 0..3 {
            let mut foreign = works.clone();
            match field {
                0 => foreign[1].accountable_team_id = Some("other-team".into()),
                1 => foreign[1].assignee_membership_id = Some("other-membership".into()),
                _ => foreign[1].owner_member_id = Some("other-member".into()),
            }
            assert!(select(&foreign, &events).is_none());
        }
    }

    #[test]
    fn unproven_order_self_acceptance_and_new_block_fail_closed() {
        let (works, events) = fixture();
        for timestamp in [
            "",
            "invalid",
            "2026-09-07T00:00:00Z",
            "2026-09-07T00:00:01Z",
        ] {
            let mut changed = events.clone();
            changed[1].created_at = timestamp.into();
            assert!(select(&works, &changed).is_none());
        }
        let mut changed = events.clone();
        changed[1].performed_by_actor.id = "member".into();
        assert!(select(&works, &changed).is_none());
        let mut changed = events.clone();
        changed[0].created_at = "2026-09-07T00:00:03Z".into();
        assert!(select(&works, &changed).is_none());
    }

    #[test]
    fn production_cli_times_and_api_times_share_one_order() {
        let (works, mut events) = fixture();
        let blocked = recorded_time(&events[0].created_at)
            .unwrap()
            .unix_timestamp_nanos()
            / 1_000_000;
        events[0].created_at = format!("unix-ms:{blocked}");
        assert!(select(&works, &events).is_some());
        events[1].created_at = format!("unix-ms:{}", blocked + 1);
        assert!(select(&works, &events).is_some());
        events[1].created_at = format!("unix-ms:{blocked}");
        assert!(select(&works, &events).is_none());
        events[1].created_at = format!("unix-ms:{}", i128::MAX);
        assert!(select(&works, &events).is_none());
    }

    #[test]
    fn cross_work_sequence_numbers_do_not_determine_order() {
        let (mut works, mut events) = fixture();
        works[0].version = 200;
        events[0].sequence = 200;
        events[0].resulting_version = 200;
        events[0].expected_version = 199;
        assert!(select(&works, &events).is_some());
        events[1].created_at = "2026-09-07T01:00:00+01:00".into();
        assert!(select(&works, &events).is_none());
    }
}
