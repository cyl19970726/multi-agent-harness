use super::*;

    #[test]
    fn member_handoff_accepts_acp_message_chunk_shape() {
        let chunks = [
            serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"text": "ordinary assistant narration"}
            }),
            serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"text": "## RESULT\ndone\n"}
            }),
            serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"text": "## SUMMARY\nchunk-shaped terminal report"}
            }),
        ];
        let accumulated = chunks
            .iter()
            .filter_map(|chunk| chunk["content"]["text"].as_str())
            .collect::<String>();

        assert_eq!(
            canonical_member_report_text(&accumulated),
            "## RESULT\ndone\n## SUMMARY\nchunk-shaped terminal report"
        );
        assert_eq!(parse_round_result(&accumulated), MemberRoundResult::Done);
    }

