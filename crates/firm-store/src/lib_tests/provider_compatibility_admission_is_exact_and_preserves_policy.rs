use super::*;

#[test]
fn provider_compatibility_admission_is_exact_and_preserves_policy() {
    let store = provider_admission_test_store("exact");
    let strict = provider_compatibility_admission("strict", "sdk", "contract-v1");
    let mut advisory = provider_compatibility_admission("advisory", "interactive", "contract-v2");
    advisory.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
    store.admit_provider_compatibility(&strict).expect("strict");
    store
        .admit_provider_compatibility(&advisory)
        .expect("advisory");

    assert_eq!(
        store
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1")
            .expect("lookup"),
        Some(strict)
    );
    assert!(store
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v2")
        .expect("contract isolation")
        .is_none());
    assert_eq!(
        store
            .latest_provider_compatibility_admissions()
            .unwrap()
            .into_iter()
            .find(|row| row.id == "advisory")
            .expect("advisory projection")
            .policy,
        firm_core::ProviderCompatibilityAdmissionPolicy::Advisory
    );
}
