use super::*;

    #[test]
    fn member_handoff_without_result_heading_is_backward_compatible() {
        assert_eq!(
            canonical_member_report_text("  legacy free-form report  "),
            "legacy free-form report"
        );
    }

