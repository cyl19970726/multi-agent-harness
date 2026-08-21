use super::*;

#[test]
fn continuation_is_emitted_only_for_in_progress_work() {
    let in_progress = continuation_test_work(WorkPhase::Active, WorkCondition::Normal, None);
    assert!(is_active_work_continuation_candidate(
        &in_progress,
        "member-run-test",
        std::slice::from_ref(&in_progress),
    ));

    for (label, phase, condition, resolution) in [
        ("open", WorkPhase::Open, WorkCondition::Normal, None),
        ("blocked", WorkPhase::Active, WorkCondition::Blocked, None),
        ("on_hold", WorkPhase::Active, WorkCondition::OnHold, None),
        ("review", WorkPhase::Review, WorkCondition::Normal, None),
        (
            "accepted",
            WorkPhase::Closed,
            WorkCondition::Normal,
            Some(WorkResolution::Accepted),
        ),
        (
            "cancelled",
            WorkPhase::Closed,
            WorkCondition::Normal,
            Some(WorkResolution::Cancelled),
        ),
    ] {
        let work = continuation_test_work(phase, condition, resolution);
        assert!(
            !is_active_work_continuation_candidate(
                &work,
                "member-run-test",
                std::slice::from_ref(&work),
            ),
            "{label} Work must not receive active continuation",
        );
    }
}
