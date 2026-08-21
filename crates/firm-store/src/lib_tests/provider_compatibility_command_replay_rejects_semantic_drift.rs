use super::*;

#[test]
fn provider_compatibility_command_replay_rejects_semantic_drift() {
    for (tag, mutate) in [
        (
            "policy",
            (|row: &mut ProviderCompatibilityAdmission| {
                row.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
            }) as fn(&mut ProviderCompatibilityAdmission),
        ),
        ("actor", |row: &mut ProviderCompatibilityAdmission| {
            row.actor = "another-operator".into();
        }),
        ("evidence", |row: &mut ProviderCompatibilityAdmission| {
            row.evidence_refs = vec!["different-evidence".into()];
        }),
    ] {
        let store = provider_admission_test_store(tag);
        let first = provider_compatibility_admission("first", "sdk", "contract-v1");
        store
            .ensure_provider_compatibility_admission(&first)
            .expect("seed admission");
        let mut drifted = first;
        drifted.id = "second".into();
        drifted.admitted_at = "unix-ms:2".into();
        mutate(&mut drifted);
        assert!(matches!(
            store.ensure_provider_compatibility_admission(&drifted),
            Err(StoreError::Conflict(_))
        ));
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
    }
}
