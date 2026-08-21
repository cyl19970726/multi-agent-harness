use super::*;

    #[test]
    fn silence_is_only_a_provider_error_when_harness_would_mint_the_handoff() {
        // `parse_round_result("")` reads as Done, which is why silence needs a
        // rule at all.
        assert_eq!(parse_round_result(""), MemberRoundResult::Done);
        // The rule lives at the one place a handoff would be MINTED, so a
        // deferred round and a member-published handoff keep their own
        // meanings; only fabrication is refused.
        for silence in ["", "   ", "\n\n", "\t\r\n "] {
            assert!(
                canonical_member_report_text(silence).trim().is_empty(),
                "{silence:?} must read as silence"
            );
        }
        assert!(!canonical_member_report_text("## RESULT\ndone\n")
            .trim()
            .is_empty());
    }

