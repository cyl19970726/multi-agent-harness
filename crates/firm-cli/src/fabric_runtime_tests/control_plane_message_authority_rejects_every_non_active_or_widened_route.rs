use super::*;

    #[test]
    fn control_plane_message_authority_rejects_every_non_active_or_widened_route() {
        let mut attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
            ))
            .unwrap();
        let mut delegation: harness_core::collaboration::WorkDelegationV1 =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
            ))
            .unwrap();
        let mut policy: harness_core::collaboration::DelegationInboundPolicy =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/delegation-inbound-policy/valid/host-approval.json"
            ))
            .unwrap();
        attestation.id = delegation.source_work_attestation_id.clone();
        attestation.source_work_ref = delegation.source_work_ref.clone();
        attestation.source_owner_ref = delegation.source_owner_ref.clone();
        policy.revision = delegation.inbound_policy_snapshot.policy_revision;
        delegation.inbound_policy_snapshot.policy_digest =
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "policy_id": policy.id,
                "policy_revision": policy.revision,
                "mode": policy.mode,
                "allowed_outcome_classes": policy.allowed_outcome_classes,
                "max_active_delegations": policy.max_active_delegations,
            }));
        delegation.state = harness_core::collaboration::DelegationState::Active;
        delegation.revision = 3;
        let mut target_work = delegation.source_work_ref.clone();
        target_work.execution_space_id = "space-b".into();
        target_work.node_id = delegation.target_placement.node_id.clone();
        target_work.team_id = delegation.target_placement.team_id.clone();
        target_work.team_revision = delegation.target_placement.team_revision;
        target_work.work_id = "work-b".into();
        target_work.work_revision = 1;
        delegation.target_work_ref = Some(target_work.clone());
        let mut authority = harness_core::collaboration::CollaborationMessageAuthority {
            company_id: delegation.company_id.clone(),
            delegation_id: delegation.id.clone(),
            delegation_revision: delegation.revision,
            source_work_ref: delegation.source_work_ref.clone(),
            target_work_ref: target_work,
            target_placement: delegation.target_placement.clone(),
            source_owner_ref: delegation.source_owner_ref.clone(),
            source_host_ref: attestation.source_host_ref.clone(),
            target_host_ref: delegation.target_host_ref.clone(),
            inbound_policy_snapshot: delegation.inbound_policy_snapshot.clone(),
            authority_digest: String::new(),
        };
        authority.authority_digest =
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
        let reference = harness_fabric::CollaborationBusinessReference {
            business_kind: "team_message_deliver".into(),
            required_capability: "collaboration.team_message_deliver".into(),
            business_actor_kind: "agent_member".into(),
            business_actor_id: attestation.source_host_ref.id.clone(),
            target_team_id: delegation.target_placement.team_id.clone(),
            target_team_revision: delegation.target_placement.team_revision,
            placement_generation: delegation.target_placement.placement_generation,
            expected_revision: delegation.revision,
            payload_digest: format!("sha256:{:064x}", 1),
            payload: serde_json::Value::Null,
        };
        validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &reference,
        )
        .expect("exact active authority");

        for state in [
            harness_core::collaboration::DelegationState::Proposed,
            harness_core::collaboration::DelegationState::AwaitingTargetDecision,
            harness_core::collaboration::DelegationState::CancellationRequested,
            harness_core::collaboration::DelegationState::Terminal,
        ] {
            let mut hostile = delegation.clone();
            hostile.state = state;
            assert!(validate_current_collaboration_message_authority(
                "company-1",
                &hostile,
                &attestation,
                &policy,
                &authority,
                &reference,
            )
            .is_err());
        }
        let mut stale_reference = reference.clone();
        stale_reference.expected_revision -= 1;
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &stale_reference,
        )
        .is_err());
        let mut wrong_actor = reference.clone();
        wrong_actor.business_actor_id = "member-from-source-team-without-work-authority".into();
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &wrong_actor,
        )
        .is_err());
        let mut stale_policy = policy.clone();
        stale_policy.revoked_at = Some("unix-ms:4".into());
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &stale_policy,
            &authority,
            &reference,
        )
        .is_err());
        let mut widened_placement = reference;
        widened_placement.target_team_id = "sibling-target-team".into();
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &widened_placement,
        )
        .is_err());
    }

