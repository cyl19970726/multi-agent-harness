use super::*;

#[test]
fn jsonl_read_retries_a_concurrently_incomplete_final_row() {
    let root = team_test_root("concurrent-partial-row");
    let store = HarnessStore::new(&root);
    store.init().expect("initialize store");
    let path = root.join("concurrent.jsonl");
    let (partial_ready_tx, partial_ready_rx) = std::sync::mpsc::channel();

    let writer_store = store.clone();
    let writer = std::thread::spawn(move || {
        let _lock = writer_store.acquire_write_lock().expect("writer lock");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open concurrent ledger");
        file.write_all(br#"{"id":"row-1""#)
            .expect("write partial row");
        file.flush().expect("flush partial row");
        partial_ready_tx.send(()).expect("announce partial row");
        std::thread::sleep(Duration::from_millis(30));
        file.write_all(b",\"value\":1}\n")
            .expect("finish concurrent row");
        file.sync_all().expect("sync concurrent row");
    });

    partial_ready_rx.recv().expect("wait for partial row");
    let rows = store
        .read_jsonl::<serde_json::Value>("concurrent.jsonl")
        .expect("reader waits for the complete final row");
    writer.join().expect("writer completes");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "row-1");
    assert_eq!(rows[0]["value"], 1);
    std::fs::remove_dir_all(root).expect("remove temp store");
}
