use super::*;

    #[test]
    fn pi_full_access_profile_denies_quiesce_without_child_inventory() {
        let mut read_only = team_member_provider_profile_for_mode("pi", Some("pi_rpc"));
        apply_permission_enforcement_to_profile(
            &mut read_only,
            harness_core::agentfirm_api::PermissionCeiling::ReadOnly,
        )
        .unwrap();
        assert_eq!(
            read_only
                .capability_bindings
                .iter()
                .find(|binding| binding.capability == "quiesce")
                .unwrap()
                .admission,
            harness_core::ProviderBindingAdmission::PendingDependency
        );

        let mut full_access = team_member_provider_profile_for_mode("pi", Some("pi_rpc"));
        apply_permission_enforcement_to_profile(
            &mut full_access,
            harness_core::agentfirm_api::PermissionCeiling::FullAccess,
        )
        .unwrap();
        for capability in ["quiesce", "release"] {
            let binding = full_access
                .capability_bindings
                .iter()
                .find(|binding| binding.capability == capability)
                .unwrap();
            assert_eq!(
                binding.admission,
                harness_core::ProviderBindingAdmission::PendingDependency
            );
            assert!(binding.evidence.iter().any(|evidence| {
                evidence
                    .evidence_ref
                    .contains("requires_native_job_inventory_or_os_containment")
            }));
        }
        assert_ne!(
            full_access.capability_fingerprint,
            read_only.capability_fingerprint
        );
        assert_ne!(
            full_access.composition_fingerprint,
            read_only.composition_fingerprint
        );
    }

