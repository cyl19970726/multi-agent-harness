use super::*;

#[test]
#[allow(clippy::result_large_err)]
fn collaboration_authority_fence_holds_writer_lock_through_route_commit() {
    let test = TestStore::new("authority-fence");
    install_policy(&test.store);
    let first = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let root = test.root.clone();
    let first_worker = first.clone();
    let release_worker = release.clone();
    let worker = std::thread::spawn(move || {
        let store = HarnessStore::new(root);
        store
            .with_collaboration_authority_fence(
                |locked| {
                    assert!(locked
                        .collaboration_inbound_policy("company-1", "policy-a-b")
                        .unwrap()
                        .unwrap()
                        .revoked_at
                        .is_none());
                    first_worker.wait();
                    release_worker.wait();
                    Ok(())
                },
                || Ok::<_, firm_fabric::FabricError>("fabric-commit"),
            )
            .unwrap()
    });
    first.wait();
    let writer_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer_finished_thread = writer_finished.clone();
    let writer_root = test.root.clone();
    let writer = std::thread::spawn(move || {
        let store = HarnessStore::new(writer_root);
        let mut revoked = policy();
        revoked.revision = 2;
        revoked.revoked_at = Some("unix-ms:99".into());
        store
            .put_collaboration_inbound_policy(
                &context(
                    actor(ActorKind::AgentMember, "host-b"),
                    "delegation.policy.put",
                    "policy-revoke",
                    1,
                ),
                &revoked,
                &actor(ActorKind::AgentMember, "host-b"),
            )
            .unwrap();
        writer_finished_thread.store(true, Ordering::SeqCst);
    });
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(
        !writer_finished.load(Ordering::SeqCst),
        "authority writer must not cross the admission→Fabric commit fence"
    );
    release.wait();
    assert_eq!(worker.join().unwrap(), "fabric-commit");
    writer.join().unwrap();
    assert!(writer_finished.load(Ordering::SeqCst));
}
