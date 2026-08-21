use super::*;

    #[test]
    fn kimi_safe_approval_survives_same_session_verification_timestamp_refresh() {
        let (store, _root) = temp_store("kimi-safe-session-verification-refresh");
        let session_id = "session-verification-refresh";
        let (ledger, supplied) =
            persisted_native_test_member(&store, "kimi", "kimi_acp", session_id);
        let mut latest = supplied.clone();
        latest
            .native_session
            .as_mut()
            .expect("native session")
            .last_verified_at = Some("unix-ms:after-first-turn".into());
        store
            .compare_and_append_member_run(&supplied, &latest)
            .expect("advance only the native-session observation timestamp");

        let reply = handle_kimi_provider_request(
            &ledger,
            &supplied,
            &kimi_safe_approval_frame(session_id, 731),
        )
        .expect("same-session reverse-RPC remains authorized after a settled turn");
        assert_eq!(
            reply.result["outcome"]["outcome"],
            serde_json::json!("selected")
        );
        assert!(reply.result["outcome"]["optionId"]
            .as_str()
            .is_some_and(|id| id.starts_with("tool_allow_always_")));
    }

