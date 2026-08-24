use super::*;

fn wait_for_issued_ticket(store: &HarnessStore, expected_next_ticket: u64) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let next_ticket = store
            .process_write_lock
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .next_ticket;
        if next_ticket >= expected_next_ticket {
            return;
        }
        assert!(Instant::now() < deadline, "writer did not enter FIFO queue");
        std::thread::yield_now();
    }
}

#[test]
fn write_lock_fifo_admission_skips_timed_out_waiters() {
    let store = Arc::new(lock_policy_test_store("fifo-timeout"));
    let first = store
        .acquire_write_lock_with_policy(Duration::from_secs(1), Duration::from_millis(1))
        .expect("first writer owns the Store");

    let (order_tx, order_rx) = mpsc::channel();
    let timed_out_store = Arc::clone(&store);
    let timed_out = std::thread::spawn(move || {
        timed_out_store
            .acquire_write_lock_with_policy(Duration::from_millis(40), Duration::from_millis(1))
    });
    wait_for_issued_ticket(&store, 2);

    let next_store = Arc::clone(&store);
    let next = std::thread::spawn(move || {
        let permit = next_store
            .acquire_write_lock_with_policy(Duration::from_secs(1), Duration::from_millis(1))
            .expect("next FIFO writer acquires after cancelled ticket");
        order_tx.send("next").expect("record acquisition");
        drop(permit);
    });
    wait_for_issued_ticket(&store, 3);

    let error = match timed_out.join().expect("timed-out writer joins") {
        Ok(_) => panic!("middle ticket must time out"),
        Err(error) => error,
    };
    assert!(matches!(error, StoreError::LockTimeout(_)));
    drop(first);
    assert_eq!(
        order_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("next writer"),
        "next"
    );
    next.join().expect("next writer joins");
    std::fs::remove_dir_all(store.root()).expect("cleanup store");
}

#[test]
fn queued_store_writers_acquire_in_ticket_order() {
    let store = Arc::new(lock_policy_test_store("fifo-order"));
    let first = store
        .acquire_write_lock_with_policy(Duration::from_secs(1), Duration::from_millis(1))
        .expect("first writer owns the Store");
    let (order_tx, order_rx) = mpsc::channel();
    let mut writers = Vec::new();

    for index in 0..8 {
        let writer_store = Arc::clone(&store);
        let writer_tx = order_tx.clone();
        writers.push(std::thread::spawn(move || {
            let permit = writer_store
                .acquire_write_lock_with_policy(Duration::from_secs(2), Duration::from_millis(1))
                .expect("queued writer acquires");
            writer_tx.send(index).expect("record acquisition");
            drop(permit);
        }));
        wait_for_issued_ticket(&store, index + 2);
    }
    drop(order_tx);
    drop(first);

    let order = order_rx.iter().collect::<Vec<_>>();
    assert_eq!(order, (0..8).collect::<Vec<_>>());
    for writer in writers {
        writer.join().expect("writer joins");
    }
    std::fs::remove_dir_all(store.root()).expect("cleanup store");
}

#[test]
fn physical_store_path_aliases_share_one_fifo_queue() {
    let store = lock_policy_test_store("physical-alias");
    let root = store.root().to_path_buf();
    let parent = root.parent().expect("Store parent");
    let name = root.file_name().expect("Store name");
    let dotdot_alias = root.join("..").join(name);
    let symlink_alias = parent.join(format!("{}-alias", name.to_string_lossy()));
    std::os::unix::fs::symlink(&root, &symlink_alias).expect("create Store root alias");

    let dotdot_store = HarnessStore::new(dotdot_alias);
    let symlink_store = HarnessStore::new(&symlink_alias);
    assert!(Arc::ptr_eq(
        &store.process_write_lock,
        &dotdot_store.process_write_lock
    ));
    assert!(Arc::ptr_eq(
        &store.process_write_lock,
        &symlink_store.process_write_lock
    ));

    std::fs::remove_file(&symlink_alias).expect("remove Store root alias");
    std::fs::remove_dir_all(&root).expect("cleanup store");
}
