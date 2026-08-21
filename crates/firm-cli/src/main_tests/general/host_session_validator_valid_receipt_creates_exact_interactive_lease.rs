use super::*;

#[test]
fn host_session_validator_valid_receipt_creates_exact_interactive_lease() {
    let (store, root) = temp_store("validated-bind-lease");
    let created = create_two_member_team_run(&store);
    let validator = FakeHostSessionValidator {
        receipt: Ok(HostSessionValidationReceipt {
            host_surface: "codex".into(),
            host_thread_id: "native-thread-1".into(),
            owner_id: "interactive:test-session".into(),
            discovery_source: "deterministic_test_fake",
        }),
    };
    let result = bind_host_with_validator(
        &store,
        &created.team_run.id,
        "codex-app",
        "native-thread-1",
        30_000,
        &validator,
        100,
    )
    .expect("validated bind");
    let lease = result.lease.expect("active lease");
    assert_eq!(lease.owner_kind, HostBindingLeaseOwnerKind::Interactive);
    assert_eq!(lease.host_surface, "codex");
    assert_eq!(lease.host_thread_id, "native-thread-1");
    assert!(lease.is_effective_at(101));
    std::fs::remove_dir_all(root).expect("cleanup");
}
