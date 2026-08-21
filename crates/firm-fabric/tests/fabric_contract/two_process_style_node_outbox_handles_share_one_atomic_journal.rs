use super::*;

#[test]
fn two_process_style_node_outbox_handles_share_one_atomic_journal() {
    let root = TestRoot::new("cross-process-node-local-lock");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut joins = Vec::new();
    for _ in 0..2 {
        let root = root.path().to_path_buf();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let store = NodeLocalFabricStore::open(root, COMPANY, "node-a")
                .expect("open independent Node-local handle");
            let request = operation(1, 1);
            let session = fabric_session("node-a", 1, 1);
            let source_actor = request.actor.clone();
            store
                .bind_gateway_session(&session)
                .expect("bind shared exact session");
            barrier.wait();
            store.prepare_outbox(&session, &source_actor, &request, 100)
        }));
    }
    let results = joins
        .into_iter()
        .map(|join| join.join().expect("join writer").expect("prepare outbox"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|(_, replay)| !replay).count(), 1);
    assert_eq!(results.iter().filter(|(_, replay)| *replay).count(), 1);
    assert_eq!(
        NodeLocalFabricStore::open(root.path(), COMPANY, "node-a")
            .expect("reopen Node-local Store")
            .snapshot()
            .expect("snapshot")
            .outboxes
            .len(),
        1
    );
}
