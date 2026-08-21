use super::*;

#[test]
fn bind_host_via_cas_preserves_explicit_authority_path() {
    // L2: explicit bind-host remains the authority/takeover path.
    let (store, root) = temp_store("bind-host-authority");
    let created = create_two_member_team_run(&store);
    assert!(created.team_run.host_thread_id.is_none());

    // bind-host via CAS
    let current = latest_team_run(&store, &created.team_run.id).expect("current");
    let mut next = current.clone();
    next.host_surface = "codex-app".into();
    next.host_thread_id = Some("explicit-thread".into());
    next.updated_at = "unix-ms:explicit".into();
    store
        .compare_and_append_team_run(&current, &next)
        .expect("CAS bind");

    let bound = latest_team_run(&store, &created.team_run.id).expect("bound");
    assert_eq!(bound.host_surface, "codex-app");
    assert_eq!(bound.host_thread_id.as_deref(), Some("explicit-thread"));
    let _ = std::fs::remove_dir_all(root);
}
