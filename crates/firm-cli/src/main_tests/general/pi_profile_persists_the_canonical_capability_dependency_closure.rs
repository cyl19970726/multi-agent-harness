use super::*;

#[test]
fn pi_profile_persists_the_canonical_capability_dependency_closure() {
    let profile = team_member_provider_profile_for_mode("pi", Some("pi_rpc"));
    let binding = |name: &str| {
        profile
            .capability_bindings
            .iter()
            .find(|binding| binding.capability == name)
            .unwrap_or_else(|| panic!("missing Pi capability binding {name}"))
    };
    assert_eq!(
        binding("start_cycle").required_dependencies,
        vec!["open_or_resume", "observe"]
    );
    assert_eq!(
        binding("inject_current_cycle").required_dependencies,
        vec!["observe"]
    );
    assert_eq!(
        binding("quiesce").required_dependencies,
        vec!["interrupt_current_cycle", "observe"]
    );
    assert_eq!(binding("release").required_dependencies, vec!["quiesce"]);
}
