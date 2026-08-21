use super::*;

#[test]
fn provider_compatibility_recovery_rejects_closed_retired_or_finished_block() {
    use firm_core::MemberCoordinationStatus;

    for (index, coordination, finished_at) in [
        (0, MemberCoordinationStatus::Closed, None),
        (1, MemberCoordinationStatus::Retired, None),
        (
            2,
            MemberCoordinationStatus::Active,
            Some("hostile-finished"),
        ),
    ] {
        let root = provider_admission_test_root(&format!("hostile-recovery-{index}"));
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
        let (_run, initial, _work) =
            seed_host_attention_fixture(&store, &format!("hostile-recovery-{index}"), None);
        let profile = provider_compatibility_test_profile();
        let cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: format!("recovery-cause-{index}"),
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
        let blocked = store
            .block_member_run_for_provider_compatibility(&initial, &profile, cause, "unix-ms:2")
            .expect("seed typed block");
        let mut hostile = blocked.clone();
        hostile.coordination_status = coordination;
        hostile.finished_at = finished_at.map(str::to_string);
        store
            .compare_and_append_member_run(&blocked, &hostile)
            .expect("seed hostile blocked history without changing typed cause");
        let mut admission = provider_compatibility_admission(
            &format!("hostile-recovery-admission-{index}"),
            "kimi_acp",
            "kimi-acp-v1",
        );
        admission.provider = "kimi".into();
        store
            .admit_provider_compatibility_admission(&admission)
            .expect("admit tuple");
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &hostile,
                &profile,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:3",
            )
            .expect_err("closed/retired/finished block cannot recover")
            .to_string()
            .contains("LIFECYCLE_INVALID"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
