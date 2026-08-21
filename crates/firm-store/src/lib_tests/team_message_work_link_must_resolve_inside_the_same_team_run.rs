use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn team_message_work_link_must_resolve_inside_the_same_team_run() {
    let (root, store, run, member, _) = work_test_fixture("message-work-link");
    let work = store
        .insert_work(
            unassigned_test_work(&run.id, "work-discussed"),
            host_work_context("we-discussed", "create-discussed", "unix-ms:2"),
        )
        .expect("create discussed Work");
    let message = TeamMessageProjection {
        id: "tm-work-discussion".into(),
        team_run_id: run.id.clone(),
        work_id: Some(work.id.clone()),
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec![member.id.clone()],
        kind: ProviderDispatchIntent::Message,
        body: "Clarify the evidence for this Work.".into(),
        correlation_id: "corr-work-discussion".into(),
        causation_id: None,
        response_intent: Some(ProviderResponseIntent::ResponseRequired),
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: member.id.clone(),
            policy: TeamDeliveryPolicy::Queue,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:3".into(),
        }],
        created_at: "unix-ms:3".into(),
    };
    store
        .append_team_message_checked(&message)
        .expect("same-TeamRun Work discussion");

    let mut foreign = message;
    foreign.id = "tm-cross-run-work".into();
    foreign.team_run_id = "another-team-run".into();
    let error = store
        .append_team_message_checked(&foreign)
        .expect_err("cross-TeamRun Work link must be rejected");
    assert!(
        error.to_string().contains("belongs to TeamRun"),
        "error: {error}"
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
