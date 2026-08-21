use super::*;

#[test]
fn codex_thread_id_spoof_without_rollout_stays_unleased() {
    let (store, root) = temp_store("codex-thread-id-spoof");
    let created = create_two_member_team_run(&store);
    let codex_home = root.join("empty-codex-home");
    std::fs::create_dir_all(codex_home.join("sessions")).expect("sessions");
    let spoofed = "019f-spoofed-session";
    // This environment hint formerly created evidence. The validator no
    // longer reads it; only parsed rollout session_meta can do so.
    std::env::set_var("CODEX_THREAD_ID", spoofed);
    let result = bind_host_with_validator(
        &store,
        &created.team_run.id,
        "codex",
        spoofed,
        30_000,
        &RuntimeHostSessionValidator::for_codex_home(codex_home),
        100,
    )
    .expect("observable unleased bind");
    std::env::remove_var("CODEX_THREAD_ID");
    assert!(result.lease.is_none());
    assert!(result
        .validation_warning
        .as_deref()
        .is_some_and(|warning| warning.contains("rollout metadata")));
    assert!(store
        .latest_host_binding_lease(&created.team_run.id)
        .expect("lease read")
        .is_none());
    std::fs::remove_dir_all(root).expect("cleanup");
}
