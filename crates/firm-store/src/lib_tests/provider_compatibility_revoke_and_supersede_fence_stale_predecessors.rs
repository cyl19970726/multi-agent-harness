use super::*;

#[test]
fn provider_compatibility_revoke_and_supersede_fence_stale_predecessors() {
    for lifecycle in [
        ProviderCompatibilityAdmissionLifecycle::Revoked,
        ProviderCompatibilityAdmissionLifecycle::Superseded,
    ] {
        let store = provider_admission_test_store("transition");
        let active = provider_compatibility_admission("active", "sdk", "contract-v1");
        store.admit_provider_compatibility(&active).unwrap();
        let mut transition = active.clone();
        transition.id = "transition".to_string();
        transition.lifecycle = lifecycle;
        transition.predecessor_admission_id = Some(active.id.clone());
        transition.reason = Some("contract changed".to_string());
        let mut wrong_predecessor = transition.clone();
        wrong_predecessor.id = "wrong-predecessor".to_string();
        wrong_predecessor.predecessor_admission_id = Some("another-active".to_string());
        assert!(matches!(
            store.append_provider_compatibility_admission_checked(&wrong_predecessor),
            Err(StoreError::Conflict(_))
        ));
        match lifecycle {
            ProviderCompatibilityAdmissionLifecycle::Revoked => store
                .revoke_provider_compatibility(&transition)
                .expect("revoke"),
            ProviderCompatibilityAdmissionLifecycle::Superseded => store
                .supersede_provider_compatibility(&transition)
                .expect("supersede"),
            ProviderCompatibilityAdmissionLifecycle::Active => unreachable!(),
        }
        store
            .append_provider_compatibility_admission_checked(&transition)
            .expect("terminal replay is idempotent");
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 2);
        assert!(store
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1")
            .unwrap()
            .is_none());

        let mut stale = transition;
        stale.id = "stale".to_string();
        assert!(matches!(
            store.append_provider_compatibility_admission_checked(&stale),
            Err(StoreError::Conflict(_))
        ));
    }
}
