use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn member_close_or_session_cas_wins_before_receipt_with_zero_action() {
    for mutation in ["close", "session"] {
        let root = team_test_root(&format!("member-action-current-{mutation}"));
        let store = HarnessStore::new(&root);
        let (_, request) =
            seed_provider_interaction_bridge(&store, &format!("run-action-{mutation}"));
        let expected = latest_by_id(store.member_runs().expect("members"), |member| {
            member.id.clone()
        })
        .remove(&request.sender_runtime_id)
        .expect("member");
        let action = provider_control_action(&request.team_run_id, &expected.id);
        assert!(store
            .append_member_action(&action)
            .expect_err("raw provider control is forbidden before lifecycle change")
            .to_string()
            .contains("PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN"));
        assert!(store.member_actions().expect("actions").is_empty());
        let mut next = expected.clone();
        if mutation == "close" {
            next.coordination_status = firm_core::MemberCoordinationStatus::Closed;
            next.status = MemberRunStatus::Stopped;
            next.finished_at = Some("unix-ms:4".into());
        } else {
            next.native_session
                .as_mut()
                .expect("native session")
                .native_session_id = "replacement-session".into();
        }
        store
            .compare_and_append_member_run(&expected, &next)
            .expect("lifecycle/session CAS wins first");
        let mut raw_after = action.clone();
        raw_after.id.push_str("-after");
        assert!(store
            .append_member_action(&raw_after)
            .expect_err("raw provider control is forbidden after lifecycle change")
            .to_string()
            .contains("PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN"));
        assert!(store
            .append_member_action_if_member_run_current(&expected, &action)
            .expect_err("stale expected member must fail")
            .to_string()
            .contains("changed concurrently"));
        assert!(store.member_actions().expect("actions").is_empty());
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}
