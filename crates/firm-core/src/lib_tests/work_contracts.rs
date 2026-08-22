use super::*;

#[test]
fn work_prerequisite_satisfaction_is_distinct_from_claim_readiness() {
    fn work(
        id: &str,
        phase: WorkPhase,
        resolution: Option<WorkResolution>,
        prerequisites: Vec<&str>,
    ) -> Work {
        Work {
            id: id.into(),
            team_run_id: "team-1".into(),
            accountable_team_id: None,
            assignee_membership_id: None,
            created_by_member_id: None,
            legacy_parent_work_id: None,
            title: id.into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            phase,
            condition: WorkCondition::Normal,
            resolution,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: prerequisites.into_iter().map(str::to_string).collect(),
            priority: WorkPriority::Normal,
            created_by_actor: TeamActorRef {
                kind: TeamActorKind::Host,
                id: "host".into(),
                display_name: None,
                authn_source: None,
            },
            result_summary: None,
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            version: 1,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        }
    }

    let prerequisite = work(
        "prerequisite",
        WorkPhase::Closed,
        Some(WorkResolution::Accepted),
        vec![],
    );
    let in_progress = work("dependent", WorkPhase::Active, None, vec!["prerequisite"]);
    assert!(in_progress.prerequisites_satisfied([&prerequisite]));
    assert!(!in_progress.is_claim_ready([&prerequisite]));

    let open = work(
        "dependent-open",
        WorkPhase::Open,
        None,
        vec!["prerequisite"],
    );
    assert!(open.is_claim_ready([&prerequisite]));

    let unfinished = work("prerequisite", WorkPhase::Review, None, vec![]);
    assert!(!open.prerequisites_satisfied([&unfinished]));
    assert!(!open.is_claim_ready([&unfinished]));
}

#[test]
fn legacy_work_delivery_update_defaults_to_unsequenced() {
    let update: ProviderWorkDispatchUpdate = serde_json::from_value(serde_json::json!({
        "delivery_id": "delivery-legacy",
        "status": "queued",
        "attempt": 1,
        "updated_at": "unix-ms:1"
    }))
    .expect("legacy delivery update remains readable");
    assert_eq!(update.update_sequence, 0);
}

#[test]
fn legacy_parent_work_is_decode_only_evidence() {
    let mut value = serde_json::json!({
        "id": "work-legacy",
        "team_run_id": "run-1",
        "accountable_team_id": "team-1",
        "parent_work_id": "historical-parent",
        "title": "Legacy row",
        "context_markdown": "",
        "completion_criteria_markdown": "done",
        "phase": "open",
        "condition": "normal",
        "claim_mode": "team_claim",
        "priority": "normal",
        "created_by_actor": {"kind": "host", "id": "host-1"},
        "version": 1,
        "created_at": "unix-ms:1",
        "updated_at": "unix-ms:1"
    });
    let work: Work = serde_json::from_value(value.clone()).expect("legacy parent decodes");
    assert_eq!(
        work.legacy_parent_work_id.as_deref(),
        Some("historical-parent")
    );
    value = serde_json::to_value(work).expect("current Work serializes");
    assert!(value.get("parent_work_id").is_none());
    assert!(value.get("legacy_parent_work_id").is_none());
}

#[test]
fn host_attention_keeps_transport_intake_distinct_from_work_semantics() {
    let mut attention = HostAttention {
        id: "host-attention-work-event-1".into(),
        team_run_id: "team-run-1".into(),
        kind: HostAttentionKind::WorkReviewRequested,
        work_id: "work-1".into(),
        work_version: 3,
        source_event_ref: "work-event-1".into(),
        member_run_id: Some("member-run-1".into()),
        status: HostAttentionStatus::Actionable,
        attempt: 0,
        claim_id: None,
        claimed_host_surface: None,
        claimed_host_thread_id: None,
        claimed_host_lease_id: None,
        claimed_host_lease_generation: None,
        claimed_host_lease_owner_id: None,
        claimed_recipient_member_run_id: None,
        claimed_recipient_session_id: None,
        claimed_recipient_session_generation: None,
        claimed_node_daemon_id: None,
        claimed_node_daemon_generation: None,
        provider_receipt_id: None,
        last_failure_reason: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    assert!(attention.validate().is_ok());
    assert!(attention.needs_host_action());

    attention.claim_id = Some("claim-1".into());
    attention.claimed_host_surface = Some("codex".into());
    attention.claimed_host_thread_id = Some("thread-1".into());
    attention.status = HostAttentionStatus::Claimed;
    assert!(attention.validate().is_ok(), "interactive claim is valid");

    attention.status = HostAttentionStatus::Delivered;
    attention.provider_receipt_id = Some("provider-receipt-1".into());
    assert!(
        attention.validate().is_ok(),
        "interactive delivery is valid"
    );
    assert!(
        attention.needs_host_action(),
        "delivery is transport receipt, not Host intake or Work acceptance"
    );
    attention.status = HostAttentionStatus::Acknowledged;
    assert!(attention.validate().is_ok());
    assert!(!attention.needs_host_action());

    attention.claimed_host_lease_id = Some("lease-1".into());
    assert!(
        attention.validate().is_err(),
        "partial lease fence is invalid"
    );
    attention.claimed_host_lease_generation = Some(1);
    attention.claimed_host_lease_owner_id = Some("dispatcher-1".into());
    assert!(attention.validate().is_ok(), "dispatcher delivery is valid");

    let json = serde_json::to_value(&attention).expect("serialize Host attention");
    assert_eq!(json["kind"], "work_review_requested");
    assert_eq!(json["status"], "acknowledged");
    assert!(json.get("team_message_id").is_none());
    assert!(json.get("work_status").is_none());
}

#[test]
fn agent_team_wire_is_flat_node_placed_and_mission_independent() {
    let team: AgentTeam = serde_json::from_value(serde_json::json!({
        "id": "team-1",
        "name": "Core",
        "description": "Durable Team on one Node",
        "node_id": "0f95cac7-5ff8-4c76-8f36-9c8f208815d3",
        "revision": 1,
        "created_at": "unix-ms:1",
        "updated_at": "unix-ms:1"
    }))
    .expect("flat AgentTeam wire");
    assert_eq!(team.validate(), Ok(()));
    assert!(team.legacy_mission_id.is_none());
    assert!(team.mission_id.is_empty());
    assert!(team.host_agent_id.is_empty());
    assert!(team.member_ids.is_empty());

    let mut migrated = serde_json::to_value(&team).expect("serialize AgentTeam");
    migrated["legacy_mission_id"] = serde_json::json!("mission-1");
    let migrated: AgentTeam =
        serde_json::from_value(migrated).expect("optional legacy Mission provenance");
    assert_eq!(migrated.legacy_mission_id.as_deref(), Some("mission-1"));

    for legacy_field in [
        "mission_id",
        "host_agent_id",
        "member_ids",
        "owner_agent_id",
        "parent_team_id",
        "host_member_id",
    ] {
        let mut value = serde_json::to_value(&team).expect("serialize AgentTeam");
        value[legacy_field] = serde_json::json!("legacy");
        assert!(
            serde_json::from_value::<AgentTeam>(value).is_err(),
            "clean cutover rejects {legacy_field}"
        );
    }
}

#[test]
fn node_and_daemon_fences_validate_generation_and_time() {
    let node = ExecutionNode {
        id: "0f95cac7-5ff8-4c76-8f36-9c8f208815d3".into(),
        display_name: "build-node-a".into(),
        status: ExecutionNodeStatus::Active,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    assert_eq!(node.validate(), Ok(()));

    let mut lease = NodeDaemonLease {
        node_id: node.id,
        daemon_id: "daemon-a".into(),
        generation: 1,
        instance_id: "pid:4242:start:1000".into(),
        status: NodeDaemonLeaseStatus::Active,
        acquired_unix_ms: 1_000,
        renewed_unix_ms: 1_200,
        expires_unix_ms: 6_200,
        released_unix_ms: None,
    };
    assert_eq!(lease.validate(), Ok(()));
    lease.generation = 0;
    assert!(lease.validate().is_err());
    lease.generation = 1;
    lease.expires_unix_ms = 1_100;
    assert!(lease.validate().is_err());
}

#[cfg(test)]
fn test_actor(id: &str) -> TeamActorRef {
    TeamActorRef {
        kind: TeamActorKind::AgentMember,
        id: id.into(),
        display_name: None,
        authn_source: None,
    }
}

#[test]
fn work_delegation_is_cross_work_versioned_responsibility() {
    let mut delegation = WorkDelegation {
        id: "delegation-1".into(),
        source_work_ref: WorkRef {
            team_run_id: "run-source".into(),
            work_id: "work-source".into(),
        },
        source_work_version: 3,
        source_owner_member_id: "member-source".into(),
        created_by_member_run_id: Some("member-run-source".into()),
        target_agent_team_id: "team-target".into(),
        target_work_ref: WorkRef {
            team_run_id: "run-target".into(),
            work_id: "work-target".into(),
        },
        delegated_by_actor: test_actor("member-source"),
        state: WorkDelegationState::Active,
        resolution_summary: None,
        blocker_reason: None,
        version: 1,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
    };
    assert_eq!(delegation.validate(), Ok(()));

    delegation.state = WorkDelegationState::Blocked;
    assert!(delegation.validate().is_err());
    delegation.blocker_reason = Some("target capacity unavailable".into());
    assert_eq!(delegation.validate(), Ok(()));
    delegation.state = WorkDelegationState::Completed;
    delegation.blocker_reason = None;
    delegation.resolution_summary = Some("target result returned to source owner".into());
    assert_eq!(delegation.validate(), Ok(()));
}

#[test]
fn work_delegation_event_enforces_cas_version_fence() {
    let mut event = WorkDelegationEvent {
        id: "delegation-event-1".into(),
        delegation_id: "delegation-1".into(),
        sequence: 1,
        transition: WorkDelegationTransition::Created,
        expected_version: 0,
        resulting_version: 1,
        performed_by_actor: test_actor("member-source"),
        causation_ref: Some(WorkCausationRef {
            kind: "work_event".into(),
            id: "source-event-3".into(),
        }),
        idempotency_key: "create:delegation-1".into(),
        payload: serde_json::json!({"target_agent_team_id": "team-target"}),
        created_at: "unix-ms:1".into(),
    };
    assert_eq!(event.validate(), Ok(()));
    event.resulting_version = 2;
    assert!(event.validate().is_err());
    event.resulting_version = 1;
    event.payload = serde_json::Value::Null;
    assert_eq!(event.validate(), Ok(()));
}
