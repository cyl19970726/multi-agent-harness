use super::*;

    #[test]
    fn delegation_proposal_admission_rejects_revoked_policy_before_fabric_commit() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-proposal-policy-fence-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let (mut attestation, delegation, mut policy, _, _) = current_remote_fact_fixture();
        attestation.attestation_digest =
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "id": attestation.id,
                "company_id": attestation.company_id,
                "source_work_ref": attestation.source_work_ref,
                "source_owner_ref": attestation.source_owner_ref,
                "source_host_ref": attestation.source_host_ref,
                "work_application_service_ref": attestation.work_application_service_ref,
                "source_gateway_generation": attestation.source_gateway_generation,
                "issued_at": attestation.issued_at,
            }));
        policy.revoked_at = Some("unix-ms:9".into());
        seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
        let request = harness_store::ProposeDelegationRequest {
            delegation_id: "delegation-policy-fence".into(),
            source_work_attestation_id: attestation.id.clone(),
            target_placement: delegation.target_placement.clone(),
            requested_outcome: "bounded implementation".into(),
            outcome_class: "implementation".into(),
            acceptance_contract: "focused checks".into(),
            operation_id: "proposal-policy-fence".into(),
        };
        let payload = serde_json::json!({
            "request": request,
            "source_work_attestation": attestation,
            "policy_id": policy.id,
        });
        let business = harness_core::collaboration::RoutedBusinessOperation {
            id: "proposal-policy-fence".into(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: "company-1".into(),
            kind: harness_core::collaboration::RoutedBusinessKind::DelegationPropose,
            authenticated_actor: attestation.source_host_ref.clone(),
            source_node_id: attestation.source_work_ref.node_id.clone(),
            target_placement: delegation.target_placement,
            expected_revision: 0,
            idempotency_key: "proposal-policy-fence".into(),
            payload_digest: harness_store::canonical_json_fingerprint(&payload),
            payload,
            required_capability: harness_core::collaboration::RoutedBusinessKind::DelegationPropose
                .required_capability(),
            ordering_key: "delegation:proposal-policy-fence".into(),
            created_at: "unix-ms:10".into(),
        };
        let operation = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: AuthenticatedActor {
                    company_id: "company-1".into(),
                    actor_id: attestation.source_work_ref.node_id.clone(),
                    actor_kind: harness_fabric::ActorKind::Service,
                    role_bindings: BTreeSet::from(["fabric_submit".into()]),
                    session_id: "daemon-a:1".into(),
                    issued_at_unix_ms: 10,
                    expires_at_unix_ms: 1_000,
                },
                resolved_business_actor: attestation.source_host_ref,
                source: harness_store::CollaborationFabricSource::Node {
                    source_execution_space_id: "space-a".into(),
                    source_gateway_generation: 9,
                    source_node_daemon_id: "daemon-a".into(),
                    source_node_daemon_generation: 1,
                },
                control_plane_generation: 3,
                target_execution_space_id: Some("space-b".into()),
                created_at_unix_ms: 10,
                expires_at_unix_ms: 1_000,
            },
        )
        .unwrap();
        let application = Wave6ControlPlaneApplication {
            collaboration_root: root.clone(),
            company_id: "company-1".into(),
            actor_id: "control-plane".into(),
        };
        let commits = std::sync::atomic::AtomicUsize::new(0);
        let mut accept = || {
            commits.fetch_add(1, Ordering::SeqCst);
            panic!("revoked policy must reject before Fabric commit")
        };
        assert!(application
            .admit_and_accept_source(&operation, &operation.actor, &mut accept)
            .is_err());
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

