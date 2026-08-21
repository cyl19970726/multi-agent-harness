use super::*;

    #[test]
    fn concurrent_cancellation_cannot_cross_remote_fact_admission_fence() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-remote-fact-cancel-fence-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let (attestation, delegation, policy, _publication, operation) =
            current_remote_fact_fixture();
        seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let application_root = root.clone();
        let operation_for_thread = operation.clone();
        let entered_application = entered.clone();
        let release_application = release.clone();
        let admission = std::thread::spawn(move || {
            let application = Wave6ControlPlaneApplication {
                collaboration_root: application_root,
                company_id: "company-1".into(),
                actor_id: "control-plane".into(),
            };
            let mut accept = || {
                entered_application.wait();
                release_application.wait();
                Err(FabricError::unknown(
                    operation_for_thread.id.clone(),
                    "deterministic Fabric commit abort",
                ))
            };
            application.admit_and_accept_source(
                &operation_for_thread,
                &operation_for_thread.actor,
                &mut accept,
            )
        });
        entered.wait();

        let cancellation_finished = Arc::new(AtomicBool::new(false));
        let cancellation_finished_thread = cancellation_finished.clone();
        let cancellation_root = root.clone();
        let cancellation_attestation = attestation.clone();
        let cancellation_delegation = delegation.clone();
        let cancellation = std::thread::spawn(move || {
            let store = HarnessStore::new(cancellation_root);
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: cancellation_attestation.source_host_ref.clone(),
                source_work_owner: cancellation_attestation.source_owner_ref.clone(),
                target_host: cancellation_delegation.target_host_ref.clone(),
                target_placement: cancellation_delegation.target_placement.clone(),
                source_work_application_service: cancellation_attestation
                    .work_application_service_ref,
                source_gateway_generation: cancellation_attestation.source_gateway_generation,
            };
            let request = harness_core::collaboration::DelegationCancellationRequest {
                id: "cancel-after-admission".into(),
                delegation_id: cancellation_delegation.id.clone(),
                expected_delegation_revision: cancellation_delegation.revision,
                requested_by: cancellation_attestation.source_host_ref.clone(),
                reason: "stop after authority linearization".into(),
                state: harness_core::collaboration::CancellationRequestState::Pending,
                target_host_decision_ref: None,
                revision: 1,
                created_at: "unix-ms:20".into(),
                updated_at: "unix-ms:20".into(),
            };
            store
                .request_delegation_cancellation(
                    &harness_store::CollaborationMutationContext {
                        company_id: "company-1".into(),
                        authenticated_actor: cancellation_attestation.source_host_ref,
                        command_name: "delegation_cancel_request".into(),
                        idempotency_key: "cancel-after-admission".into(),
                        expected_revision: cancellation_delegation.revision,
                        occurred_at: "unix-ms:20".into(),
                    },
                    &request,
                    &authority,
                )
                .unwrap();
            cancellation_finished_thread.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            !cancellation_finished.load(Ordering::SeqCst),
            "cancellation writer must not cross admission through Fabric commit"
        );
        release.wait();
        assert!(admission.join().unwrap().is_err());
        cancellation.join().unwrap();
        assert!(cancellation_finished.load(Ordering::SeqCst));

        let application = Wave6ControlPlaneApplication {
            collaboration_root: root.clone(),
            company_id: "company-1".into(),
            actor_id: "control-plane".into(),
        };
        let commits = std::sync::atomic::AtomicUsize::new(0);
        let mut accept = || {
            commits.fetch_add(1, Ordering::SeqCst);
            panic!("the pre-cancellation route must not reach Fabric")
        };
        assert!(application
            .admit_and_accept_source(&operation, &operation.actor, &mut accept)
            .is_err());
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

