use super::*;

    #[test]
    fn kimi_safe_approval_rejects_closed_retired_generation_and_session_drift() {
        for case in ["closed", "retired", "generation", "session"] {
            let (store, _root) = temp_store(&format!("kimi-safe-stale-{case}"));
            let session_id = format!("session-{case}");
            let (ledger, supplied) =
                persisted_native_test_member(&store, "kimi", "kimi_acp", &session_id);
            let mut latest = supplied.clone();
            match case {
                "closed" => {
                    latest.coordination_status = MemberCoordinationStatus::Closed;
                    latest.status = MemberRunStatus::Stopped;
                    latest.finished_at = Some("unix-ms:closed".into());
                }
                "retired" => {
                    latest.coordination_status = MemberCoordinationStatus::Retired;
                    latest.status = MemberRunStatus::Stopped;
                    latest.finished_at = Some("unix-ms:retired".into());
                }
                "generation" => latest.runtime_generation += 1,
                "session" => {
                    latest
                        .native_session
                        .as_mut()
                        .expect("native session")
                        .native_session_id = "replacement-session".into();
                }
                _ => unreachable!(),
            }
            if case == "generation" {
                store
                    .compare_and_advance_member_run_generation(&supplied, &latest)
                    .expect("advance canonical member generation before callback");
            } else {
                store
                    .compare_and_append_member_run(&supplied, &latest)
                    .expect("advance canonical member before callback");
            }

            let error = handle_kimi_provider_request(
                &ledger,
                &supplied,
                &kimi_safe_approval_frame(&session_id, 730),
            )
            .expect_err("stale safe approval must fail closed");
            assert!(
                error.to_string().contains("crossed identity")
                    || error
                        .to_string()
                        .contains("changed during provider callback")
                    || error.to_string().contains("is no longer active"),
                "unexpected {case} error: {error}"
            );
            assert!(
                store
                    .member_actions()
                    .expect("member actions")
                    .into_iter()
                    .all(|action| action.action_type != "provider_control"),
                "{case} callback wrote a provider-control receipt"
            );
        }
    }

