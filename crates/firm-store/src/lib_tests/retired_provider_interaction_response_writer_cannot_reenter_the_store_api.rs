#[test]
fn retired_provider_interaction_response_writer_cannot_reenter_the_store_api() {
    let source = include_str!("../store_node_runtime.rs");
    let retired_writer = ["fn record_provider_", "interaction_response"].concat();
    assert!(
        !source.contains(&retired_writer),
        "retired provider-interaction response writer must not be compiled into the Store"
    );

    let checked_writer = source
        .split("pub fn append_team_message_checked")
        .nth(1)
        .and_then(|tail| tail.split("pub fn insert_execution_node").next())
        .expect("retired checked writer remains an explicit fail-closed API");
    assert!(
        checked_writer.contains("RETIRED_RUNTIME_WRITER"),
        "retired checked writer must fail closed"
    );
    assert!(
        !checked_writer.contains("append_jsonl_unlocked")
            && !checked_writer.contains("team_messages.jsonl")
            && !checked_writer.contains("Acknowledged"),
        "retired checked writer must not retain a hidden ledger mutation or ACK path"
    );
}
