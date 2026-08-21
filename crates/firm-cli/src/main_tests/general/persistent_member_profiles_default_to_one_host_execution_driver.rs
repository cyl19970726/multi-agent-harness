use super::*;

#[test]
fn persistent_member_profiles_default_to_one_host_execution_driver() {
    for provider in ["codex", "claude", "kimi"] {
        assert_eq!(
            team_member_provider_profile(provider).execution_driver,
            MemberExecutionDriver::HostDriven,
            "{provider} Agent Team mode must not start an independent native continuation loop"
        );
    }
}
