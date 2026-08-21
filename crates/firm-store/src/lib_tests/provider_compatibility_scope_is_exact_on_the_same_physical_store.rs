use super::*;

#[test]
fn provider_compatibility_scope_is_exact_on_the_same_physical_store() {
    let root = provider_admission_test_root("scope");
    let writer = HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
    let admission = provider_compatibility_admission("scoped", "sdk", "contract-v1");
    writer.admit_provider_compatibility(&admission).unwrap();

    let other_project =
        HarnessStore::new(&root).with_provider_compatibility_scope("project-2", "store-1");
    assert!(other_project
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
        .unwrap()
        .is_none());
    let migrated_store =
        HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-2");
    assert!(migrated_store
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
        .unwrap()
        .is_none());
}
