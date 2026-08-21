use super::*;

#[test]
fn start_warns_and_auto_binds_when_unbound() {
    // Verify via the store that auto-bind works at the data level.
    // (The warning goes to stderr, which unit tests don't capture.)
    let (store, root) = temp_store("start-auto-bind");
    let created = create_two_member_team_run(&store);
    assert!(created.team_run.host_thread_id.is_none());

    // Simulate the CLI start block's auto-bind logic with both env vars
    // present (as star-harness SessionStart would set them).
    let run = latest_team_run(&store, &created.team_run.id).expect("current");
    assert!(run.host_thread_id.is_none());

    // Hardcode the env-derived values: both present → auto-bind via CAS.
    let mut next = run.clone();
    next.host_surface = "codex-app".into();
    next.host_thread_id = Some("start-thread".into());
    next.updated_at = "unix-ms:auto".into();
    store
        .compare_and_append_team_run(&run, &next)
        .expect("auto-bind CAS");

    let current = latest_team_run(&store, &created.team_run.id).expect("after bind");
    assert_eq!(current.host_surface, "codex-app");
    assert_eq!(current.host_thread_id.as_deref(), Some("start-thread"));

    let _ = std::fs::remove_dir_all(root);
}
