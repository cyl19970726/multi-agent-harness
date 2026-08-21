use super::*;

#[test]
fn team_run_completion_guard_is_store_authoritative() {
    let (root, store, run, _, _) = work_test_fixture("completion-guard");
    store
        .insert_work(
            unassigned_test_work(&run.id, "work-open"),
            host_work_context("we-open", "create-open", "unix-ms:2"),
        )
        .expect("create open Work");

    let error = store
        .compare_and_append_team_run_lifecycle(&run, &completed_team_run(&run, "unix-ms:3"))
        .expect_err("Store must reject completion while Work is non-terminal");
    assert!(
        error
            .to_string()
            .contains("Works remain non-terminal: work-open (open/normal, version 1)"),
        "completion guard should identify the authoritative unfinished Work: {error}"
    );
    assert_eq!(
        store
            .team_runs()
            .expect("read TeamRuns")
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == run.id)
            .expect("TeamRun remains present")
            .status,
        TeamRunStatus::Running,
        "a rejected completion must not append a terminal TeamRun row"
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
