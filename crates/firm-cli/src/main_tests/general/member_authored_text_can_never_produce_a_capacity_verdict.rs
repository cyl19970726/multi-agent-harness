use super::*;

    #[test]
    fn member_authored_text_can_never_produce_a_capacity_verdict() {
        // The recorded summary embeds the MEMBER's own first line, so a member
        // writing about a 403 must not mark its account unauthorized, and a
        // member discussing quotas must not mark it exhausted.
        let hostile = [
            "provider turn failed: empty_final_report: fixed the 403 handler and the quota math",
            "provider turn failed: empty_final_report: rate limit docs updated; 401/403 covered",
            "provider turn failed: empty_final_report: usage limit table now lists 429",
        ];
        let actions: Vec<MemberAction> = hostile
            .iter()
            .enumerate()
            .map(|(index, summary)| {
                provider_error_action(
                    "member-run-1",
                    &format!("unix-ms:{}", 1000 + index),
                    summary,
                    None,
                )
            })
            .collect();

        assert!(
            capacity_from_provider_error_actions(
                &actions,
                "member-run-1",
                "claude",
                "claude_agent_sdk",
                1_500,
                1_000,
            )
            .is_none(),
            "prose must never reach a capacity classifier"
        );
    }

