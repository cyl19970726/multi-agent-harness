use super::*;

#[test]
fn provider_compatibility_command_replay_creates_after_terminal_row() {
    let store = provider_admission_test_store("command-after-terminal");
    let active = provider_compatibility_admission("active", "sdk", "contract-v1");
    store
        .ensure_provider_compatibility_admission(&active)
        .expect("seed active");
    let mut revoked = active.clone();
    revoked.id = "revoked".into();
    revoked.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
    revoked.predecessor_admission_id = Some(active.id.clone());
    revoked.reason = Some("operator revoked".into());
    store
        .revoke_provider_compatibility_admission(&revoked)
        .expect("revoke active");

    let mut replacement = active;
    replacement.id = "replacement".into();
    replacement.admitted_at = "unix-ms:3".into();
    let result = store
        .ensure_provider_compatibility_admission(&replacement)
        .expect("create replacement");
    assert!(result.created);
    assert_eq!(result.admission.id, "replacement");
    assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 3);
}
