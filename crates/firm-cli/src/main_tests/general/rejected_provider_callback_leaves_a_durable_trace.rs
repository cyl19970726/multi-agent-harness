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

    // A MemberAction may name a reviewed protocol selector because that is a
    // machine-readable transport identifier, not provider-authored text. An
    // unrecognized or absent `method` must therefore not reach the ledger as a
    // free-form provider string.
    for hostile in [
        serde_json::json!({"method": "totally/unreviewed", "params": {}}),
        serde_json::json!({"method": {"not": "a string"}, "params": {}}),
        serde_json::json!({"params": {}}),
    ] {
        let outcome: CliResult<()> = trace_provider_callback_rejection(
            &ledger,
            &supplied.id,
            &hostile,
            Err(CliError::Usage("denied fail-closed".into())),
        );
        assert!(outcome.is_err());
        let recorded = store
            .member_actions()
            .expect("member actions")
            .into_iter()
            .filter(|action| action.action_type == "provider_callback_rejected")
            .map(|action| action.title)
            .collect::<Vec<_>>();
        assert!(
            recorded
                .iter()
                .all(|title| title == "session/request_permission"
                    || title == "unreviewed_provider_method"),
            "an unreviewed callback selector must not reach a MemberAction title: {recorded:?}"
        );
    }

    // The summary channel needs the same bound. The Codex unsupported-method
    // error is Harness-authored but used to quote the raw provider selector,
    // and this trace records that error verbatim.
    let (codex_store, _codex_root) = temp_store("provider-callback-rejection-trace-codex");
    let (codex_ledger, codex_member) = persisted_native_test_member(
        &codex_store,
        "codex",
        "codex_app_server",
        "session-rejection-trace-codex",
    );
    let hostile_method = "item/tool/HostileUnreviewedSelector";
    let codex_frame = serde_json::json!({
        "id": 991,
        "method": hostile_method,
        "params": {"threadId": "session-rejection-trace-codex"}
    });
    let codex_outcome = trace_provider_callback_rejection(
        &codex_ledger,
        &codex_member.id,
        &codex_frame,
        handle_codex_provider_request(&codex_ledger, &codex_member, &codex_frame),
    );
    assert!(
        codex_outcome.is_err(),
        "an unsupported Codex reverse request must still fail closed"
    );
    let codex_trace = codex_store
        .member_actions()
        .expect("member actions")
        .into_iter()
        .find(|action| action.action_type == "provider_callback_rejected")
        .expect("the Codex fail-closed path leaves a durable trace");
    assert_eq!(codex_trace.title, "unreviewed_provider_method");
    assert!(
        !codex_trace.summary.contains(hostile_method),
        "an unreviewed selector must not reach the summary through the error: {}",
        codex_trace.summary
    );
    assert!(
        codex_trace.summary.contains("unreviewed_provider_method"),
        "the bounded token takes its place: {}",
        codex_trace.summary
    );
}
