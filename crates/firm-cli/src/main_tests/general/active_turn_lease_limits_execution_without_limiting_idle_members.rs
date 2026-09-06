use super::*;

#[test]
fn active_turn_lease_limits_execution_without_limiting_idle_members() {
    let pool = Arc::new(ActiveTurnLeasePool::new(1));
    let first = pool.acquire();
    let contender_pool = Arc::clone(&pool);
    let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
    let contender = std::thread::spawn(move || {
        let _second = contender_pool.acquire();
        acquired_tx.send(()).expect("report second lease");
    });

    assert!(
        acquired_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "a second active provider turn must wait while the only lease is held"
    );
    drop(first);
    acquired_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("idle/finished turn must release the lease");
    contender.join().expect("contender");
}

#[test]
fn queued_prepared_cycle_rechecks_quiesce_after_occupied_slot_is_released() {
    let (store, root) = temp_store("queued-prepared-cycle-quiesce");
    let (ledger, member) =
        persisted_native_test_member(&store, "codex", "codex_app_server", "thread-queued-quiesce");
    let effect = prepare_provider_effect(&ledger, &member, "queued-cycle", "must not drive", 1)
        .expect("prepare B before quiesce");
    let space = store.trust_member_run_scope(&member.id).unwrap().unwrap();
    let pool = Arc::new(ActiveTurnLeasePool::new(1));
    let first = pool.acquire();
    let drive_calls = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let contender = scope.spawn(|| {
            ledger
                .require_supervisor_lease()
                .expect("B passed the pre-wait check");
            let result =
                crate::runtime_adapter::acquire_prepared_cycle_turn(&ledger, &effect, &pool, &[]);
            if result.is_ok() {
                drive_calls.fetch_add(1, Ordering::SeqCst);
            }
            assert!(
                result.is_err(),
                "B must not enter provider drive after quiesce"
            );
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while pool.waiting.load(Ordering::Acquire) != 1 {
            assert!(
                Instant::now() < deadline,
                "B never reached the occupied-slot wait"
            );
            std::thread::yield_now();
        }
        let prepared = store
            .runtime_commands(&space)
            .unwrap()
            .into_iter()
            .find(|command| command.id == effect.command_id)
            .unwrap();
        assert_eq!(
            prepared.phase,
            harness_core::agentfirm_api::RuntimeCommandPhase::Prepared
        );
        ledger.supervisor_valid.store(false, Ordering::Release);
        drop(first);
        contender.join().unwrap();
    });
    assert_eq!(drive_calls.load(Ordering::SeqCst), 0);
    let rejected = store
        .runtime_commands(&space)
        .unwrap()
        .into_iter()
        .find(|command| command.id == effect.command_id)
        .unwrap();
    assert_eq!(
        rejected.phase,
        harness_core::agentfirm_api::RuntimeCommandPhase::Rejected
    );
    assert_eq!(
        rejected.effect_certainty,
        harness_core::agentfirm_api::RuntimeEffectCertainty::NotApplied
    );
    assert_eq!(
        *pool.active.lock().unwrap(),
        0,
        "refusal releases B's acquired slot"
    );
    std::fs::remove_dir_all(root).unwrap();
}
