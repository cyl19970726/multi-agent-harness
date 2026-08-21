use super::*;

#[test]
fn team_run_completion_and_work_create_serialize_without_invalid_state() {
    for iteration in 0..16 {
        let (root, store, run, _, _) =
            work_test_fixture(&format!("completion-create-race-{iteration}"));
        let barrier = Arc::new(Barrier::new(3));

        let completion_store = store.clone();
        let completion_run = run.clone();
        let completion_barrier = Arc::clone(&barrier);
        let completion = std::thread::spawn(move || {
            completion_barrier.wait();
            completion_store.compare_and_append_team_run_lifecycle(
                &completion_run,
                &completed_team_run(&completion_run, "unix-ms:3"),
            )
        });

        let work_store = store.clone();
        let work_run_id = run.id.clone();
        let work_barrier = Arc::clone(&barrier);
        let create = std::thread::spawn(move || {
            work_barrier.wait();
            work_store.insert_work(
                unassigned_test_work(&work_run_id, "work-racing"),
                host_work_context("we-racing", "create-racing", "unix-ms:2"),
            )
        });

        barrier.wait();
        let completion_result = completion.join().expect("completion thread");
        let create_result = create.join().expect("Work create thread");
        assert_ne!(
            completion_result.is_ok(),
            create_result.is_ok(),
            "the write lock must serialize the race so exactly one operation succeeds"
        );

        let latest_run = store
            .team_runs()
            .expect("read TeamRuns")
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == run.id)
            .expect("TeamRun remains present");
        let has_nonterminal_work = store
            .latest_works()
            .expect("read Works")
            .into_iter()
            .any(|work| work.team_run_id == run.id && !work.is_terminal());
        assert!(
            latest_run.status != TeamRunStatus::Completed || !has_nonterminal_work,
            "completed TeamRun plus non-terminal Work is forbidden regardless of race winner"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}
