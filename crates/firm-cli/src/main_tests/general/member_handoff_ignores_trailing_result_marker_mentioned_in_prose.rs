use super::*;

    #[test]
    fn member_handoff_ignores_trailing_result_marker_mentioned_in_prose() {
        let text = "interim narration## RESULT\n\
                    blocked\n\
                    ## SUMMARY\n\
                    real terminal report\n\
                    Reviewer note: do not repeat ## RESULT in prose.";

        assert_eq!(
            canonical_member_report_text(text),
            "## RESULT\nblocked\n## SUMMARY\nreal terminal report\nReviewer note: do not repeat ## RESULT in prose."
        );
        assert_eq!(parse_round_result(text), MemberRoundResult::Blocked);
    }

