use super::*;

#[test]
fn close_without_live_supervisor_requires_runtime_command_recovery_without_side_effects() {
    // A missing Supervisor does not prove the provider effect is absent.
    // The lifecycle API must not invent a second stop authority by reaping
    // a Running projection outside the RuntimeCommand ledger.
    let (store, root) = temp_store("durable-close-reap");
    let created = create_two_member_team_run(&store);
    let initial_run = created.team_run;
    let mut run = initial_run.clone();
    run.status = TeamRunStatus::Running;
    run.updated_at = "unix-ms:2".into();
    store
        .compare_and_append_team_run_lifecycle(&initial_run, &run)
        .expect("mark run running");
    let initial_member = created.member_runs[0].clone();
    let mut member = initial_member.clone();
    member.status = MemberRunStatus::Running;
    member.last_event_at = Some("unix-ms:2".into());
    store
        .compare_and_append_member_run(&initial_member, &member)
        .expect("mark member running");

    let member_rows_before = store.member_runs().expect("member rows before");
    let close_rows_before = store
        .team_member_close_requests()
        .expect("close rows before");
    let error = close_team_member_value(
        &store,
        &run.id,
        &member.id,
        &serde_json::json!({
            "requested_by": "host",
            "reason": "lane accepted"
        }),
    )
    .expect_err("ambiguous runtime must require recovery");
    assert!(error
        .to_string()
        .contains("RUNTIME_COMMAND_RECOVERY_REQUIRED"));
    assert_eq!(
        store.member_runs().expect("member rows after"),
        member_rows_before
    );
    assert_eq!(
        store
            .team_member_close_requests()
            .expect("close rows after"),
        close_rows_before
    );
    let _ = std::fs::remove_dir_all(root);
}
