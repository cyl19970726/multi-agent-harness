use super::*;

    #[test]
    fn claude_sdk_native_session_has_deterministic_desktop_import_target() {
        let target = native_session_open_target(&native_open_test_member(
            "claude",
            "claude_agent_sdk",
            "851b37dd-1234-5678-9abc-0123456789ab",
        ))
        .expect("Claude SDK target");
        assert_eq!(
            target["uri"],
            "claude://resume?session=851b37dd-1234-5678-9abc-0123456789ab"
        );
        assert_eq!(
            target["desktop_session_id"],
            "local_851b37dd-1234-5678-9abc-0123456789ab"
        );
        assert_eq!(target["ownership"], "provider_native");
    }

