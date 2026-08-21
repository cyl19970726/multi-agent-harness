use super::*;

#[test]
fn typed_provider_block_is_store_owned_and_recovery_is_exact() {
    let root = provider_admission_test_root("typed-block");
    let store = HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
    let (_run, initial, _work) = seed_host_attention_fixture(&store, "typed-block", None);
    let profile = provider_compatibility_test_profile();
    let cause = ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "cause-1".into(),
        member_run_id: initial.id.clone(),
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        provider_version: "2.1.220".into(),
        adapter_contract_version: "kimi-acp-v1".into(),
        boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:2".into(),
    };

    let mut forged = initial.clone();
    forged.status = MemberRunStatus::Blocked;
    forged.provider_compatibility_block_cause = Some(cause.clone());
    assert!(store
        .compare_and_append_member_run(&initial, &forged)
        .expect_err("generic CAS cannot forge typed cause")
        .to_string()
        .contains("AUTHORITY_REQUIRED"));

    let blocked = store
        .block_member_run_for_provider_compatibility(&initial, &profile, cause, "unix-ms:2")
        .expect("dedicated typed block");
    let mut cleared = blocked.clone();
    cleared.status = MemberRunStatus::Idle;
    cleared.provider_compatibility_block_cause = None;
    assert!(store
        .compare_and_append_member_run(&blocked, &cleared)
        .expect_err("generic CAS cannot clear typed cause")
        .to_string()
        .contains("AUTHORITY_REQUIRED"));

    let mut wrong = profile.clone();
    wrong.provider_version = Some("2.1.221".into());
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &blocked,
            &wrong,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:3"
        )
        .expect_err("an unadmitted new tuple cannot recover")
        .to_string()
        .contains("NOT_AUTHORIZED"));

    let mut admission =
        provider_compatibility_admission("typed-recovery", "kimi_acp", "kimi-acp-v1");
    admission.provider = "kimi".into();
    store
        .admit_provider_compatibility_admission(&admission)
        .expect("exact admission");
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &blocked,
            &profile,
            ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:3",
        )
        .expect_err("a Start cause cannot recover at Resume")
        .to_string()
        .contains("BOUNDARY_MISMATCH"));
    let recovered = store
        .recover_member_run_from_provider_compatibility_block(
            &blocked,
            &profile,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:3",
        )
        .expect("exact typed recovery");
    assert_eq!(recovered.id, initial.id);
    assert_eq!(recovered.status, MemberRunStatus::Idle);
    assert!(recovered.provider_compatibility_block_cause.is_none());
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &blocked,
            &profile,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:4"
        )
        .expect_err("stale recovery loses CAS")
        .to_string()
        .contains("changed concurrently"));

    let mut operator_blocked = recovered.clone();
    operator_blocked.status = MemberRunStatus::Blocked;
    store
        .compare_and_append_member_run(&recovered, &operator_blocked)
        .expect("ordinary operator block remains representable");
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &operator_blocked,
            &profile,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:5"
        )
        .expect_err("operator block has no typed cause")
        .to_string()
        .contains("CAUSE_REQUIRED"));

    let (_run2, initial2, _work2) =
        seed_host_attention_fixture(&store, "typed-source-reviewed", None);
    let mut review_pending = provider_compatibility_test_profile();
    review_pending.provider_version = Some("3.3.3".into());
    let source_cause = ProviderCompatibilityBlockCause {
        schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
        id: "cause-source-review".into(),
        member_run_id: initial2.id.clone(),
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        provider_version: "3.3.3".into(),
        adapter_contract_version: "kimi-acp-v1".into(),
        boundary: ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
        compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
        source: ProviderCompatibilityBlockSource::AdapterCompatibility,
        probe_error: None,
        caused_at: "unix-ms:6".into(),
    };
    let source_blocked = store
        .block_member_run_for_provider_compatibility(
            &initial2,
            &review_pending,
            source_cause,
            "unix-ms:6",
        )
        .expect("block pending source review");
    let mut source_reviewed = review_pending;
    source_reviewed.provider_version = Some("3.3.4".into());
    source_reviewed.compatibility_status = ProviderCompatibilityStatus::Current;
    source_reviewed.reviewed_provider_versions = vec!["3.3.4".into()];
    assert!(store
        .recover_member_run_from_provider_compatibility_block(
            &source_blocked,
            &source_reviewed,
            ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:7",
        )
        .expect_err("a Resume cause cannot recover at Start")
        .to_string()
        .contains("BOUNDARY_MISMATCH"));
    let source_recovered = store
        .recover_member_run_from_provider_compatibility_block(
            &source_blocked,
            &source_reviewed,
            ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            MemberRunStatus::Idle,
            "unix-ms:7",
        )
        .expect("exact source review authorizes recovery without an admission");
    assert!(source_recovered
        .provider_compatibility_block_cause
        .is_none());
    assert_eq!(
        source_recovered
            .provider_profile
            .as_ref()
            .and_then(|profile| profile.provider_version.as_deref()),
        Some("3.3.4"),
        "recovery atomically replaces the durable blocked profile with the authorized refreshed tuple"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
