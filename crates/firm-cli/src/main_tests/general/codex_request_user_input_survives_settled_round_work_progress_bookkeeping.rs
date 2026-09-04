use super::*;

/// Codex freezes the same reverse-RPC callback snapshot at attach as Kimi, so
/// the identical supervisor round write must not fence its request handler.
/// The frame carries two questions, which is the handler's own fail-closed
/// route: reaching that route proves the drift validator let the callback
/// through and `item/tool/requestUserInput` was still decoded.
#[test]
fn codex_request_user_input_survives_settled_round_work_progress_bookkeeping() {
    for case in ["work_version", "zero_output_streak", "finished_at"] {
        let (store, _root) = temp_store(&format!("codex-round-bookkeeping-{case}"));
        let thread_id = format!("thread-round-{case}");
        let (ledger, supplied) =
            persisted_native_test_member(&store, "codex", "codex_app_server", &thread_id);
        let mut settled = supplied.clone();
        match case {
            "work_version" => settled.last_consumed_work_version = Some(2),
            "zero_output_streak" => settled.zero_output_streak += 1,
            "finished_at" => settled.finished_at = Some("unix-ms:round-end".into()),
            _ => unreachable!(),
        }
        store
            .compare_and_append_member_run(&supplied, &settled)
            .expect("advance only supervisor round bookkeeping");

        let frame = serde_json::json!({
            "id": 733,
            "method": "item/tool/requestUserInput",
            "params": {
                "threadId": thread_id,
                "questions": [
                    {"id": "first", "header": "First", "question": "First question?", "options": []},
                    {"id": "second", "header": "Second", "question": "Second question?", "options": []}
                ]
            }
        });
        let Err(error) = handle_codex_provider_request(&ledger, &supplied, &frame) else {
            panic!("{case}: a multi-question request still fails closed on its own route");
        };
        assert!(
            error
                .to_string()
                .contains("supports exactly one question; received 2"),
            "{case} callback was fenced before reaching requestUserInput: {error}"
        );
    }
}
