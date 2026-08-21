use super::*;

#[test]
fn malformed_provider_compatibility_ledger_fails_closed_and_roots_are_isolated() {
    let first_root = provider_admission_test_root("first-root");
    let second_root = provider_admission_test_root("second-root");
    let first =
        HarnessStore::new(&first_root).with_provider_compatibility_scope("project-1", "store-1");
    let second =
        HarnessStore::new(second_root).with_provider_compatibility_scope("project-1", "store-1");
    let admission = provider_compatibility_admission("one", "sdk", "contract-v1");
    first.admit_provider_compatibility(&admission).unwrap();
    assert!(second
        .provider_compatibility_admissions()
        .unwrap()
        .is_empty());

    std::fs::write(
        first_root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER),
        b"{not-json}\n",
    )
    .unwrap();
    assert!(matches!(
        first.provider_compatibility_admissions(),
        Err(StoreError::Json(_))
    ));
    assert!(first
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1")
        .is_err());
    let mut replay = admission;
    replay.id = "two".into();
    replay.admitted_at = "unix-ms:2".into();
    assert!(matches!(
        first.ensure_provider_compatibility_admission(&replay),
        Err(StoreError::Json(_))
    ));
}
