use super::*;

    #[test]
    fn parse_codex_usage_reads_turn_completed_usage() {
        // Real codex `exec --json` shape: terminal turn.completed carries usage
        // with input/output and the SUBSET cached/reasoning counters.
        let events = ndjson_values(&[
            r#"{"type":"thread.started","thread_id":"t1"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":1200,"output_tokens":340,"cached_input_tokens":800,"reasoning_output_tokens":120}}"#,
        ]);
        let usage = parse_codex_usage(&events).expect("usage present");
        assert_eq!(usage.input, 1200);
        assert_eq!(usage.output, 340);
        // total is input+output; cached/reasoning are subsets, NOT re-added.
        assert_eq!(usage.total, 1540);
    }

