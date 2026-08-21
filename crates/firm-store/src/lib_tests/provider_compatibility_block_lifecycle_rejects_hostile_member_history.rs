use super::*;

#[test]
fn provider_compatibility_block_lifecycle_rejects_hostile_member_history() {
    use firm_core::MemberCoordinationStatus;

    for (index, mutate) in [
        (
            0,
            (
                MemberRunStatus::Completed,
                MemberCoordinationStatus::Active,
                Some("done"),
            ),
        ),
        (
            1,
            (
                MemberRunStatus::Failed,
                MemberCoordinationStatus::Active,
                Some("done"),
            ),
        ),
        (
            2,
            (
                MemberRunStatus::Stopped,
                MemberCoordinationStatus::Active,
                Some("done"),
            ),
        ),
        (
            3,
            (
                MemberRunStatus::Idle,
                MemberCoordinationStatus::Closed,
                None,
            ),
        ),
        (
            4,
            (
                MemberRunStatus::Idle,
                MemberCoordinationStatus::Retired,
                None,
            ),
        ),
        (
            5,
            (
                MemberRunStatus::Idle,
                MemberCoordinationStatus::Active,
                Some("hostile"),
            ),
        ),
    ] {
        let root = provider_admission_test_root(&format!("hostile-lifecycle-{index}"));
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
        let (_run, initial, _work) =
            seed_host_attention_fixture(&store, &format!("hostile-{index}"), None);
        let mut hostile = initial.clone();
        hostile.status = mutate.0;
        hostile.coordination_status = mutate.1;
        hostile.finished_at = mutate.2.map(str::to_string);
        store
            .compare_and_append_member_run(&initial, &hostile)
            .expect("seed hostile but structurally valid history");
        let profile = provider_compatibility_test_profile();
        let cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: format!("hostile-cause-{index}"),
            member_run_id: hostile.id.clone(),
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
        assert!(store
            .block_member_run_for_provider_compatibility(&hostile, &profile, cause, "unix-ms:3")
            .expect_err("terminal/closed/retired/finished history cannot be blocked")
            .to_string()
            .contains("LIFECYCLE_INVALID"));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
