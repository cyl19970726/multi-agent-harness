use super::*;

#[test]
fn continuation_is_emitted_only_for_in_progress_work() {
    let in_progress = continuation_test_work(WorkPhase::Active, WorkCondition::Normal, None);
    assert!(is_active_work_continuation_candidate(
        &in_progress,
        "agent-member-test",
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
                "agent-member-test",
                std::slice::from_ref(&work),
            ),
            "{label} Work must not receive active continuation",
        );
    }
}

#[test]
fn continuation_requires_provider_received_current_member_run_authority() {
    let work = continuation_test_work(WorkPhase::Active, WorkCondition::Normal, None);
    let mut member = native_open_test_member("codex", "codex_app_server", "session-current");
    member.id = "member-run-current".into();
    member.team_run_id = work.team_run_id.clone();
    member.agent_member_id = "agent-member-test".into();
    let mut delivery = harness_application::CurrentWorkDeliveryView {
        authority: harness_application::CurrentWorkDeliveryAuthority::CanonicalTrust,
        read_only: true,
        execution_space_id: Some("unit-test-space".into()),
        team_run_id: work.team_run_id.clone(),
        work_id: work.id.clone(),
        work_revision: work.version.saturating_sub(1),
        work_execution_binding_id: Some("binding-current".into()),
        delivery_id: "delivery-current".into(),
        recipient_agent_member_id: Some(member.agent_member_id.clone()),
        recipient_member_run_id: Some(member.id.clone()),
        recipient_agent_session_id: Some("session-current".into()),
        recipient_agent_session_generation: Some(1),
        target_node_id: Some("node-current".into()),
        status: harness_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived,
        attempt: 1,
        claim_id: Some("claim-current".into()),
        claimed_node_daemon_generation: Some(1),
        provider_receipt_id: Some("provider-receipt".into()),
        failure_code: None,
        version: 3,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:2".into(),
        integrity_annotations: Vec::new(),
    };

    assert!(current_work_delivery_authorizes_continuation(
        &work,
        &member,
        std::slice::from_ref(&delivery),
    ));

    delivery.recipient_member_run_id = None;
    assert!(
        !current_work_delivery_authorizes_continuation(
            &work,
            &member,
            std::slice::from_ref(&delivery),
        ),
        "a released or generation-fenced binding must not authorize continuation",
    );

    delivery.recipient_member_run_id = Some(member.id.clone());
    delivery.status = harness_core::agentfirm_api::WorkDeliveryStatus::Claimed;
    assert!(
        !current_work_delivery_authorizes_continuation(&work, &member, &[delivery]),
        "provider acceptance is required before another native cycle may continue the Work",
    );
}
