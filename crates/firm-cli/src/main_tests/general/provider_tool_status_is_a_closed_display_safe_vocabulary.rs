use super::*;

    #[test]
    fn provider_tool_status_is_a_closed_display_safe_vocabulary() {
        assert_eq!(display_safe_tool_status("in_progress", true), "running");
        assert_eq!(display_safe_tool_status("failed", false), "failed");
        assert_eq!(display_safe_tool_status("cancelled", false), "cancelled");
        assert_eq!(display_safe_tool_status("/private/path", true), "running");
        assert_eq!(
            display_safe_tool_status("secret-provider-status", false),
            "completed"
        );
    }

