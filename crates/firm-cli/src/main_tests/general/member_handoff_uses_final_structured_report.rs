use super::*;

    #[test]
    fn member_handoff_uses_final_structured_report() {
        let text = "I will inspect the inbox first.\n\
                    Progress update before the result.\n\
                    ## RESULT\n\
                    done\n\
                    ## SUMMARY\n\
                    final evidence only\n";

        assert_eq!(
            canonical_member_report_text(text),
            "## RESULT\ndone\n## SUMMARY\nfinal evidence only"
        );
        assert_eq!(
            extract_report_section(text, "SUMMARY").as_deref(),
            Some("final evidence only")
        );
    }

