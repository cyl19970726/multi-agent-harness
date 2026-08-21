use super::*;

#[test]
fn provider_compatibility_recovery_authorizes_refreshed_tuple_and_preserves_refusals() {
    let root = provider_admission_test_root("refreshed-recovery");
    let store = HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");

    let (_run, initial, _work) =
        seed_host_attention_fixture(&store, "unavailable-to-current", None);
    let mut unavailable = provider_compatibility_test_profile();
    unavailable.provider_version = None;
    unavailable.compatibility_status = ProviderCompatibilityStatus::Unavailable;
    let unavailable_cause = ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "unavailable-cause".into(),
        member_run_id: initial.id.clone(),
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        provider_version: "unavailable".into(),
        adapter_contract_version: "kimi-acp-v1".into(),
        boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::Unavailable,
        source: ProviderCompatibilityBlockSource::ProbeFailure,
        probe_error: Some("runner missing".into()),
        caused_at: "unix-ms:2".into(),
    };
    let unavailable_blocked = store
        .block_member_run_for_provider_compatibility(
            &initial,
            &unavailable,
            unavailable_cause,
            "unix-ms:2",
        )
        .expect("durably block unavailable tuple");
    let mut current = provider_compatibility_test_profile();
    current.compatibility_status = ProviderCompatibilityStatus::Current;
    current.reviewed_provider_versions = vec!["2.1.220".into()];
    let current_recovered = store
        .recover_member_run_from_provider_compatibility_block(
            &unavailable_blocked,
            &current,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:3",
        )
        .expect("source-reviewed refreshed tuple recovers old unavailable cause");
    assert_eq!(current_recovered.provider_profile.as_ref(), Some(&current));

    let (_run2, initial2, _work2) = seed_host_attention_fixture(&store, "drift-to-admitted", None);
    let old_drift = provider_compatibility_test_profile();
    let old_cause = ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "old-drift-cause".into(),
        member_run_id: initial2.id.clone(),
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        provider_version: "2.1.220".into(),
        adapter_contract_version: "kimi-acp-v1".into(),
        boundary: ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:4".into(),
    };
    let drift_blocked = store
        .block_member_run_for_provider_compatibility(&initial2, &old_drift, old_cause, "unix-ms:4")
        .expect("durably block old drift tuple");
    let mut new_drift = old_drift.clone();
    new_drift.provider_version = Some("2.1.221".into());

    let mut wrong_scope =
        provider_compatibility_admission("wrong-scope", "kimi_acp", "kimi-acp-v1");
    wrong_scope.project_id = "other-project".into();
    wrong_scope.provider = "kimi".into();
    wrong_scope.provider_version = "2.1.221".into();
    store
        .append_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, &wrong_scope)
        .expect("seed a valid admission belonging to another scope");
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &drift_blocked,
            &new_drift,
            ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:5",
        )
        .expect_err("wrong-scope admission cannot recover")
        .to_string()
        .contains("NOT_AUTHORIZED"));
    assert_eq!(
        store
            .member_runs()
            .expect("read durable member")
            .into_iter()
            .rfind(|row| row.id == drift_blocked.id),
        Some(drift_blocked.clone()),
        "refused recovery leaves the durable blocked row unchanged"
    );

    let mut exact = provider_compatibility_admission("new-exact", "kimi_acp", "kimi-acp-v1");
    exact.provider = "kimi".into();
    exact.provider_version = "2.1.221".into();
    store
        .admit_provider_compatibility_admission(&exact)
        .expect("append exact admission");
    let admitted_recovered = store
        .recover_member_run_from_provider_compatibility_block(
            &drift_blocked,
            &new_drift,
            ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:6",
        )
        .expect("new exact admission authorizes atomic recovery");
    assert_eq!(
        admitted_recovered.provider_profile.as_ref(),
        Some(&new_drift)
    );
    assert!(admitted_recovered
        .provider_compatibility_block_cause
        .is_none());
    std::fs::remove_dir_all(root).expect("cleanup");
}
