use super::*;

#[test]
fn blocked_work_can_be_resumed_by_owner_or_host_with_a_recorded_resolution() {
    let (root, store, run, member, _) = work_test_fixture("work-resume");
    let mut assigned = unassigned_test_work(&run.id, "work-resume-owner");
    assigned.active_member_run_id = Some(member.id.clone());
    assigned.claim_mode = WorkClaimMode::HostAssign;
    let assigned = store
        .insert_work(
            assigned,
            host_work_context("we-resume-1", "create-resume-1", "unix-ms:2"),
        )
        .expect("create assigned Work");
    let started = store
        .start_work(
            &assigned.id,
            assigned.version,
            &member.id,
            member_work_context(&member.id, "we-resume-2", "start-resume-1", "unix-ms:3"),
        )
        .expect("start Work");
    let blocked = store
        .block_work(
            &started.id,
            started.version,
            &member.id,
            "dependency unavailable",
            member_work_context(&member.id, "we-resume-3", "block-resume-1", "unix-ms:4"),
        )
        .expect("owner blocks Work");
    let empty = store
        .resume_work(
            &blocked.id,
            blocked.version,
            &member.id,
            "  ",
            member_work_context(&member.id, "ignored", "empty-resolution", "unix-ms:5"),
        )
        .expect_err("resume requires a resolution");
    assert!(empty.to_string().contains("resolution is required"));
    let resumed = store
        .resume_work(
            &blocked.id,
            blocked.version,
            &member.id,
            "dependency restored",
            member_work_context(&member.id, "we-resume-4", "resume-owner", "unix-ms:5"),
        )
        .expect("owner resumes Work");
    assert_eq!(resumed.phase, WorkPhase::Active);
    assert!(resumed.blocker_reason.is_none());
    let resumed_event = store
        .work_events()
        .expect("events")
        .into_iter()
        .find(|event| event.id == "we-resume-4")
        .expect("resumed event");
    assert_eq!(resumed_event.kind, WorkEventKind::Resumed);
    assert_eq!(resumed_event.payload["resolution"], "dependency restored");
    let condition_records = store.work_condition_records().expect("condition records");
    let blocked_record = condition_records
        .iter()
        .find(|record| record.condition == WorkCondition::Blocked && record.resolved_at.is_none())
        .expect("active blocker record");
    let resolved_record = condition_records
        .iter()
        .find(|record| record.resolved_at.is_some())
        .expect("resolved blocker record");
    assert_eq!(
        resolved_record.supersedes_condition_record_id.as_deref(),
        Some(blocked_record.id.as_str())
    );
    assert_eq!(resolved_record.work_version, resumed.version);
    let resumed_operation = store
        .work_operations()
        .expect("Work operations")
        .into_iter()
        .find(|operation| operation.event.id == resumed_event.id)
        .expect("resumed operation");
    let wire = serde_json::to_value(resumed_operation).expect("operation wire");
    assert!(wire.get("deliveries").is_none());
    assert!(wire.get("delivery_updates").is_none());

    let blocked_by_host = store
        .block_work_as_host(
            &resumed.id,
            resumed.version,
            "Host paused integration",
            host_work_context("we-resume-5", "block-host", "unix-ms:6"),
        )
        .expect("Host blocks Work");
    let resumed_by_host = store
        .resume_work_as_host(
            &blocked_by_host.id,
            blocked_by_host.version,
            "integration boundary cleared",
            host_work_context("we-resume-6", "resume-host", "unix-ms:7"),
        )
        .expect("Host resumes Work");
    assert_eq!(resumed_by_host.phase, WorkPhase::Active);
    assert_eq!(resumed_by_host.active_member_run_id, Some(member.id));
    std::fs::remove_dir_all(root).expect("remove temp store");
}
