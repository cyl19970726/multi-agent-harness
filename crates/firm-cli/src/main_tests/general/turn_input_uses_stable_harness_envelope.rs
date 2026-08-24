use super::*;

#[test]
fn turn_input_uses_stable_harness_envelope() {
    let message = RegistryMessage {
        id: "message-1".into(),
        task_id: Some("task-1".into()),
        from_agent_id: "leader".into(),
        to_agent_id: Some("agent-1".into()),
        channel: Some("assignment".into()),
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Acknowledged,
        content: "Do the task".into(),
        evidence_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    };

    let input = build_turn_input(&message, "delivery-1");
    let text = input[0]["text"].as_str().expect("turn text");

    assert!(text.contains("message_id: message-1"));
    assert!(text.contains("kind: message"));
    assert!(text.contains("task_id: task-1"));
    assert!(text.contains("from_agent_id: leader"));
    assert!(text.contains("to_agent_id: agent-1"));
    assert!(text.contains("channel: assignment"));
    assert!(text.contains("delivery_attempt: delivery-1"));
    assert!(text.contains("content:\nDo the task"));
    assert!(!text.contains("kind: Assignment"));
}

#[test]
fn work_contract_keeps_host_messages_on_the_work_being_discussed() {
    let mut host = native_open_test_member("codex", "codex_app_server", "thread-host");
    host.id = "member-run-host".into();
    host.agent_member_id = "agent-host".into();
    host.name = "Managed Host".into();
    host.role = "host".into();
    host.team_run_id = "team-run-test".into();
    let work = continuation_test_work(WorkPhase::Open, WorkCondition::Normal, None);
    let envelope = MemberCollaborationEnvelope {
        harness_bin: Some("firm".into()),
        execution_space_id: Some("space-test".into()),
        project_id: Some("project-test".into()),
        project_selector: None,
        mission_id: None,
        team_run_id: host.team_run_id.clone(),
        member_run_id: host.id.clone(),
        work_id: Some(work.id.clone()),
        work_version: Some(work.version),
        roster: vec![host.clone()],
    };

    let prompt = work_contract_prompt("coordinate exact Works", &host, &work, &envelope);

    assert!(prompt.contains("--work-id <discussed-work-id>"));
    assert!(prompt.contains("<recipient-work-id-from-board>"));
    assert!(prompt.contains("--work-id <incoming-work-id>"));
    assert!(prompt.contains(&format!("your current Work is {}", work.id)));
    assert!(!prompt.contains(&format!(
        "member message send --recipient-agent-id <stable-agent-identity> --work-id {}",
        work.id
    )));
    assert!(!prompt.contains(&format!(
        "member message reply --recipient-agent-id <stable-agent-identity> --correlation-id <correlation-id> --causation-id <message-id> --work-id {}",
        work.id
    )));
}
