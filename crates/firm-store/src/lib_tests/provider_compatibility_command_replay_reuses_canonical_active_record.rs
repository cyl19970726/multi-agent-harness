use super::*;

#[test]
fn provider_compatibility_command_replay_reuses_canonical_active_record() {
    let store = provider_admission_test_store("command-replay");
    let mut first = provider_compatibility_admission("generated-one", "sdk", "contract-v1");
    first.evidence_refs = vec![
        "evidence-b".into(),
        "evidence-a".into(),
        "evidence-b".into(),
    ];
    let created = store
        .ensure_provider_compatibility_admission(&first)
        .expect("create admission");
    assert!(created.created);
    assert_eq!(
        created.admission.evidence_refs,
        ["evidence-a", "evidence-b"]
    );

    let mut replay = first;
    replay.id = "generated-two".into();
    replay.admitted_at = "unix-ms:999".into();
    replay.evidence_refs = vec!["evidence-a".into(), "evidence-b".into()];
    let reused = store
        .ensure_provider_compatibility_admission(&replay)
        .expect("reuse admission");
    assert!(!reused.created);
    assert_eq!(reused.admission.id, created.admission.id);
    assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
}
