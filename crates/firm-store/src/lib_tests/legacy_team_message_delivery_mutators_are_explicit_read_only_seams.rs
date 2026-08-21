#[test]
fn legacy_team_message_delivery_mutators_are_explicit_read_only_seams() {
    let source = concat!(
        include_str!("../store_node_runtime.rs"),
        include_str!("../store_read_models.rs")
    );
    let ambiguous_reader = ["pub fn team_", "messages("].concat();
    assert!(
        !source.contains(&ambiguous_reader),
        "historical TeamMessageProjection reads must be explicitly Legacy-named"
    );
    assert!(
        source.contains("pub fn legacy_team_messages("),
        "the explicit Legacy history reader must remain available"
    );
    let retired_work_gate = ["ensure_work_store_", "compatible_unlocked"].concat();
    assert!(
        !source.contains(&retired_work_gate),
        "retired TeamMessage history must never gate current Work mutations"
    );
    for retired_function in [
        "pub fn claim_team_message_delivery(",
        "pub fn complete_team_message_delivery_claim(",
        "pub fn fail_team_message_delivery(",
    ] {
        let function_offset = source
            .find(retired_function)
            .unwrap_or_else(|| panic!("retired seam missing: {retired_function}"));
        let attribute_window = &source[function_offset.saturating_sub(180)..function_offset];
        assert!(
            attribute_window.contains("#[cfg(any())]"),
            "{retired_function} must not compile into the production Store API"
        );
    }
    let acknowledge_writer = source
        .split("pub fn acknowledge_team_message_delivery")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub fn reconcile_team_message_delivery_claim")
                .next()
        })
        .expect("legacy acknowledgement seam remains explicit");
    let reconcile_writer = source
        .split("pub fn reconcile_team_message_delivery_claim")
        .nth(1)
        .and_then(|tail| tail.split("pub fn fail_team_message_delivery").next())
        .expect("legacy reconciliation seam remains explicit");

    for retired_writer in [acknowledge_writer, reconcile_writer] {
        assert!(
            retired_writer.contains("RETIRED_RUNTIME_WRITER"),
            "legacy delivery mutators must fail closed"
        );
        assert!(
            !retired_writer.contains("append_jsonl_unlocked")
                && !retired_writer.contains("acquire_write_lock"),
            "legacy delivery mutators must not retain a hidden ledger write path"
        );
    }
}
