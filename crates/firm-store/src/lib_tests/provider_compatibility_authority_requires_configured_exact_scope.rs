use super::*;

#[test]
fn provider_compatibility_authority_requires_configured_exact_scope() {
    let root = provider_admission_test_root("scope-required");
    let unscoped = HarnessStore::new(&root);
    let active = provider_compatibility_admission("unscoped-active", "sdk", "contract-v1");
    assert!(unscoped
        .admit_provider_compatibility(&active)
        .expect_err("unscoped Store cannot mint an admission")
        .to_string()
        .contains("SCOPE_REQUIRED"));
    assert!(unscoped
        .append_provider_compatibility_admission_checked(&active)
        .expect_err("the internal checked seam is also scope-fenced")
        .to_string()
        .contains("SCOPE_REQUIRED"));

    let mut revoked = active.clone();
    revoked.id = "unscoped-revoked".into();
    revoked.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
    revoked.predecessor_admission_id = Some(active.id.clone());
    revoked.reason = Some("operator revoke".into());
    assert!(unscoped
        .revoke_provider_compatibility(&revoked)
        .expect_err("unscoped Store cannot revoke an admission")
        .to_string()
        .contains("SCOPE_REQUIRED"));

    let mut superseded = revoked.clone();
    superseded.id = "unscoped-superseded".into();
    superseded.lifecycle = ProviderCompatibilityAdmissionLifecycle::Superseded;
    assert!(unscoped
        .supersede_provider_compatibility(&superseded)
        .expect_err("unscoped Store cannot supersede an admission")
        .to_string()
        .contains("SCOPE_REQUIRED"));
    assert!(unscoped
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
        .expect_err("unscoped Store cannot return effective authority")
        .to_string()
        .contains("SCOPE_REQUIRED"));

    let wrong_scope =
        HarnessStore::new(&root).with_provider_compatibility_scope("project-2", "store-1");
    assert!(wrong_scope
        .admit_provider_compatibility(&active)
        .expect_err("configured scope must exactly match the row")
        .to_string()
        .contains("scope mismatch"));

    unscoped.init().unwrap();
    let mut hostile_row = active.clone();
    hostile_row.id = "manually-seeded-foreign-scope".into();
    hostile_row.project_id = "foreign-project".into();
    std::fs::write(
        root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER),
        format!("{}\n", serde_json::to_string(&hostile_row).unwrap()),
    )
    .unwrap();
    let scoped = HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
    assert!(scoped
        .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
        .expect("foreign ledger rows remain readable audit data")
        .is_none());

    let exact_root = provider_admission_test_root("scope-exact");
    let exact =
        HarnessStore::new(&exact_root).with_provider_compatibility_scope("project-1", "store-1");
    exact
        .admit_provider_compatibility(&active)
        .expect("exact configured scope can mint authority");
    assert_eq!(
        exact
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
            .unwrap()
            .as_ref()
            .map(|row| row.id.as_str()),
        Some("unscoped-active")
    );
}
