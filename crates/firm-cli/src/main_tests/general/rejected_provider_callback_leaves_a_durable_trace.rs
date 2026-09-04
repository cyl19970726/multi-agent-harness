use super::*;

/// A reverse-RPC handler error becomes a JSON-RPC error frame the provider
/// records as a rejected tool call. It never reaches `run_cycle` and no
/// `ExecutionCycleOutcome` field carries it, so the Supervisor logs an
/// ordinary completed round while the member has silently lost a capability.
/// The runner seam must therefore leave the denial in the coordination ledger.
#[test]
fn rejected_provider_callback_leaves_a_durable_trace() {
    let (store, _root) = temp_store("provider-callback-rejection-trace");
    let session_id = "session-rejection-trace";
    let (ledger, supplied) = persisted_native_test_member(&store, "kimi", "kimi_acp", session_id);
    let mut latest = supplied.clone();
    latest.runtime_generation += 1;
    store
        .compare_and_advance_member_run_generation(&supplied, &latest)
        .expect("advance the canonical runtime generation before the callback");
    let frame = kimi_safe_approval_frame(session_id, 734);

    let outcome = trace_provider_callback_rejection(
        &ledger,
        &supplied.id,
        &frame,
        handle_kimi_provider_request(&ledger, &supplied, &frame),
    );
    let Err(error) = outcome else {
        panic!("a stale-generation callback must still fail closed");
    };

    let trace = store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .find(|action| action.action_type == "provider_callback_rejected")
        .expect("a rejected reverse-RPC callback leaves a durable trace");
    assert_eq!(trace.member_run_id, supplied.id);
    assert_eq!(trace.status, MemberActionStatus::Failed);
    assert_eq!(trace.title, "session/request_permission");
    assert_eq!(
        trace.summary,
        error.to_string(),
        "the trace must carry the exact rejection the provider saw"
    );
}
