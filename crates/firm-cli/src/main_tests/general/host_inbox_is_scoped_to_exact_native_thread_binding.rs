use super::*;

#[cfg(any())] // Historical host-inbox fixture authored through retired TeamMessageProjection.
fn host_inbox_is_scoped_to_exact_native_thread_binding() {
    let (store, root) = temp_store("host-native-inbox");
    let created = create_two_member_team_run(&store);
    let member = &created.member_runs[0];
    let assignment = seed_host_conversation(&store, &created, 0);
    let current = latest_team_run(&store, &created.team_run.id).expect("current run");
    let mut bound = current.clone();
    bound.host_surface = "codex-app".into();
    bound.host_thread_id = Some("codex-thread-a".into());
    bound.updated_at = "unix-ms:host-bound".into();
    store
        .compare_and_append_team_run(&current, &bound)
        .expect("bind native Host");

    let mail = send_team_message(
        &store,
        &bound.id,
        &member.id,
        vec!["host".into()],
        ProviderDispatchIntent::Message,
        "QUESTION: choose interface A or B",
        Some(assignment.correlation_id.clone()),
        Some(assignment.id.clone()),
        None,
        None,
    )
    .expect("member asks Host");

    let exact = host_inbox_for_native_thread(&store, "codex-app", "codex-thread-a", false)
        .expect("exact Host inbox");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0]["team_run_id"], bound.id);
    assert_eq!(exact[0]["messages"][0]["id"], mail.id);
    assert!(
        host_inbox_for_native_thread(&store, "codex-app", "another-thread", false,)
            .expect("other Host inbox")
            .is_empty(),
        "one native Host task must never receive another task's mail"
    );
    assert!(
        host_inbox_for_native_thread(&store, "claude-code", "codex-thread-a", false)
            .expect("other surface")
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(root);
}
