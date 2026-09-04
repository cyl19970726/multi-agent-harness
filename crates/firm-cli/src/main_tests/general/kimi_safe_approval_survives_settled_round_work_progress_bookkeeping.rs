use super::*;

/// The Supervisor rewrites `zero_output_streak`, `last_consumed_work_version`
/// and `finished_at` on the member row after every settled provider round,
/// while the Kimi reverse-RPC callback still holds the snapshot frozen at
/// runtime attach. Observed live: a full-access Kimi member had all 13
/// `session/request_permission` calls approved in its first cycle and every
/// one rejected from the second cycle on, silently losing Bash and Read.
#[test]
fn kimi_safe_approval_survives_settled_round_work_progress_bookkeeping() {
    for case in ["work_version", "zero_output_streak", "finished_at"] {
        let (store, _root) = temp_store(&format!("kimi-safe-round-bookkeeping-{case}"));
        let session_id = format!("session-round-{case}");
        let (ledger, supplied) =
            persisted_native_test_member(&store, "kimi", "kimi_acp", &session_id);
        let mut settled = supplied.clone();
        match case {
            // A settled round consumed Work version 2.
            "work_version" => {
                assert_eq!(settled.last_consumed_work_version, None);
                settled.last_consumed_work_version = Some(2);
            }
            // A settled round produced no tool calls and no Work transitions.
            "zero_output_streak" => settled.zero_output_streak += 1,
            // Round end clears the previous round's terminal timestamp.
            "finished_at" => {
                let mut stopped = supplied.clone();
                stopped.finished_at = Some("unix-ms:previous-round".into());
                store
                    .compare_and_append_member_run(&supplied, &stopped)
                    .expect("record the previous round's terminal timestamp");
                settled = stopped.clone();
                settled.finished_at = None;
                store
                    .compare_and_append_member_run(&stopped, &settled)
                    .expect("clear the terminal timestamp at round end");
            }
            _ => unreachable!(),
        }
        if case != "finished_at" {
            store
                .compare_and_append_member_run(&supplied, &settled)
                .expect("advance only supervisor round bookkeeping");
        }

        let reply = handle_kimi_provider_request(
            &ledger,
            &supplied,
            &kimi_safe_approval_frame(&session_id, 732),
        )
        .unwrap_or_else(|error| {
            panic!("{case} round bookkeeping must not reject the callback: {error}")
        });
        assert_eq!(
            reply.result["outcome"]["outcome"],
            serde_json::json!("selected"),
            "{case} callback lost its full-access approval"
        );
        assert!(
            reply.result["outcome"]["optionId"]
                .as_str()
                .is_some_and(|id| id.starts_with("tool_allow_always_")),
            "{case} callback selected an unexpected option: {}",
            reply.result
        );
    }
}
