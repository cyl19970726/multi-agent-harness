use super::*;

#[test]
fn member_handoff_last_structured_report_wins() {
    let text = "## RESULT\nblocked\n## SUMMARY\nfirst attempt\n\
                    Retrying after Host feedback.\n\
                    ## RESULT\n\
                    done\n\
                    ## SUMMARY\n\
                    accepted attempt\n";

    assert_eq!(
        canonical_member_report_text(text),
        "## RESULT\ndone\n## SUMMARY\naccepted attempt"
    );
    assert_eq!(parse_round_result(text), MemberRoundResult::Done);
    assert_eq!(
        extract_report_section(text, "SUMMARY").as_deref(),
        Some("accepted attempt")
    );
}
