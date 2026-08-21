use super::*;

#[test]
fn provider_compatibility_ledger_semantic_corruption_fails_closed() {
    let root = provider_admission_test_root("semantic-corruption");
    let store = HarnessStore::new(&root);
    store.init().unwrap();
    let active = provider_compatibility_admission("active", "sdk", "contract-v1");
    let mut terminal = active.clone();
    terminal.id = "terminal".to_string();
    terminal.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
    terminal.predecessor_admission_id = Some(active.id.clone());
    terminal.reason = Some("operator revoke".to_string());

    let cases = [
        vec![active.clone(), active.clone()],
        {
            let mut unknown = terminal.clone();
            unknown.predecessor_admission_id = Some("unknown".to_string());
            vec![active.clone(), unknown]
        },
        {
            let mut drift = terminal.clone();
            drift.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
            vec![active.clone(), drift]
        },
        {
            let mut drift = terminal.clone();
            drift.store_id = "store-2".to_string();
            vec![active.clone(), drift]
        },
        vec![active.clone(), terminal.clone(), {
            let mut fork = terminal.clone();
            fork.id = "fork".to_string();
            fork
        }],
    ];

    for rows in cases {
        let text = rows
            .iter()
            .map(|row| serde_json::to_string(row).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER), text).unwrap();
        assert!(matches!(
            store.provider_compatibility_admissions(),
            Err(StoreError::Conflict(_))
        ));
    }
}
