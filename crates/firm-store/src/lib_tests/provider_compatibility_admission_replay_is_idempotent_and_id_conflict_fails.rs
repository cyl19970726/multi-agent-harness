use super::*;

#[test]
fn provider_compatibility_admission_replay_is_idempotent_and_id_conflict_fails() {
    let store = provider_admission_test_store("replay");
    let admission = provider_compatibility_admission("stable", "sdk", "contract-v1");
    store
        .append_provider_compatibility_admission(&admission)
        .expect("first append");
    store
        .append_provider_compatibility_admission(&admission)
        .expect("identical replay");
    assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);

    let mut conflict = admission;
    conflict.actor = "another-operator".to_string();
    assert!(matches!(
        store.append_provider_compatibility_admission(&conflict),
        Err(StoreError::Conflict(_))
    ));
}
