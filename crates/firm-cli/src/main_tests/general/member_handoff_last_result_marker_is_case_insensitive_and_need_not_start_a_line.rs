use super::*;

#[test]
fn member_handoff_last_result_marker_is_case_insensitive_and_need_not_start_a_line() {
    let text = "## RESULT\nblocked\n## SUMMARY\nfirst attempt\n\
                    ACP appended the terminal chunk without a newline:## rEsUlT\n\
                    done\n\
                    ## SUMMARY\n\
                    accepted concatenated attempt\n";

    assert_eq!(
        canonical_member_report_text(text),
        "## rEsUlT\ndone\n## SUMMARY\naccepted concatenated attempt"
    );
    assert_eq!(parse_round_result(text), MemberRoundResult::Done);
    assert_eq!(
        extract_report_section(text, "SUMMARY").as_deref(),
        Some("accepted concatenated attempt")
    );
}
