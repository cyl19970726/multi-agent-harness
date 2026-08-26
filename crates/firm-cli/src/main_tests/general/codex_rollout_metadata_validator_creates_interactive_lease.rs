use super::*;

#[test]
fn codex_rollout_metadata_validator_creates_interactive_lease() {
    let (store, root) = temp_store("rollout-metadata-bind-lease");
    let created = create_two_member_team_run(&store);
    let codex_home = root.join("codex-home");
    let sessions = codex_home.join("sessions/2026/08/09");
    std::fs::create_dir_all(&sessions).expect("sessions");
    let session_id = "019f-rollout-bind";
    let mut rollout = format!(
        "{{\"timestamp\":\"2026-08-09T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"
    )
    .into_bytes();
    rollout.extend_from_slice(
        b"{\"timestamp\":\"2026-08-09T00:00:01Z\",\"type\":\"event_msg\",\"payload\":\"",
    );
    rollout.extend(std::iter::repeat_n(b'x', 1024 * 1024));
    rollout.extend_from_slice(b"\"}\n");
    std::fs::write(
        sessions.join(format!("rollout-2026-08-09-{session_id}.jsonl")),
        rollout,
    )
    .expect("rollout");
    let validator = RuntimeHostSessionValidator::for_codex_home(codex_home);
    let result = bind_host_with_validator(
        &store,
        &created.team_run.id,
        "codex",
        session_id,
        30_000,
        &validator,
        100,
    )
    .expect("validated bind");
    let lease = result.lease.expect("lease");
    assert_eq!(lease.owner_id, format!("interactive:codex:{session_id}"));
    std::fs::remove_dir_all(root).expect("cleanup");
}
