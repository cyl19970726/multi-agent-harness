use super::*;

#[test]
fn runtime_composition_fingerprint_commits_to_security_and_capabilities() {
    let baseline = team_member_provider_profile_for_mode("pi", Some("pi_rpc"));
    let baseline_fingerprint = baseline
        .composition_fingerprint
        .clone()
        .expect("Pi composition fingerprint");

    let mut security_changed = baseline.clone();
    security_changed.security_enforcement_locus.note =
        Some("different reviewed enforcement mapping".into());
    finalize_provider_integration_profile(&mut security_changed);
    assert_ne!(
        security_changed.composition_fingerprint.as_deref(),
        Some(baseline_fingerprint.as_str())
    );

    let mut capability_changed = baseline;
    capability_changed
        .capability_bindings
        .iter_mut()
        .find(|binding| binding.capability == "start_cycle")
        .expect("start_cycle binding")
        .status = harness_core::ProviderCapabilityStatus::Unsupported;
    capability_changed
        .capability_bindings
        .iter_mut()
        .find(|binding| binding.capability == "start_cycle")
        .expect("start_cycle binding")
        .admission = harness_core::ProviderBindingAdmission::Failed;
    capability_changed.capability_fingerprint = profile_capability_fingerprint(&capability_changed);
    capability_changed.composition_fingerprint =
        resolved_profile_composition_fingerprint(&capability_changed);
    assert_ne!(
        capability_changed.composition_fingerprint.as_deref(),
        Some(baseline_fingerprint.as_str())
    );
}
