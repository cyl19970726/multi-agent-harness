use super::*;

#[test]
fn kimi_safe_approval_atomic_receipt_loses_to_concurrent_close() {
    let (store, _root) = temp_store("kimi-safe-atomic-close-race");
    let (ledger, expected) =
        persisted_native_test_member(&store, "kimi", "kimi_acp", "session-atomic-close");
    let mut closed = expected.clone();
    closed.coordination_status = MemberCoordinationStatus::Closed;
    closed.status = MemberRunStatus::Stopped;
    closed.finished_at = Some("unix-ms:atomic-close".into());

    let error = ledger
        .append_provider_control_receipt_once_with_hook(
            &expected,
            "Kimi full-access tool permission acknowledged",
            "safe acknowledgement",
            || {
                store_conflict_as_usage(store.compare_and_append_member_run(&expected, &closed))?;
                Ok(())
            },
        )
        .expect_err("concurrent close must win over the receipt append");
    assert!(
        error
            .to_string()
            .contains("provider receipt was not appended"),
        "unexpected atomic conflict: {error}"
    );
    assert!(
        store
            .member_actions()
            .expect("member actions")
            .into_iter()
            .all(|action| action.action_type != "provider_control"),
        "atomic CAS loss must not leave a receipt"
    );
    let latest = ledger.latest_member_run(&expected.id).unwrap().unwrap();
    assert_eq!(latest.coordination_status, MemberCoordinationStatus::Closed);
    assert_eq!(latest.status, MemberRunStatus::Stopped);
}
