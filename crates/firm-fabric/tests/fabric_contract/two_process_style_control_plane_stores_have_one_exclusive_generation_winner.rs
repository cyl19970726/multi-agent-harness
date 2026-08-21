use super::*;

#[test]
fn two_process_style_control_plane_stores_have_one_exclusive_generation_winner() {
    let root = TestRoot::new("cross-process-control-plane-lock");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut joins = Vec::new();
    for instance in ["control-a", "control-b"] {
        let root = root.path().to_path_buf();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let store = FabricStore::open(root).expect("open independent FabricStore handle");
            let keys = InMemoryArtifactKeyBackend::default();
            keys.insert(COMPANY, [7; 32]);
            let control = ControlPlane::new(COMPANY, instance, &store, &keys, [9; 32]);
            barrier.wait();
            control.acquire_lease(&format!("lease-{instance}"), 0, 1)
        }));
    }
    let results = joins
        .into_iter()
        .map(|join| join.join().expect("join competitor"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        FabricStore::open(root.path())
            .expect("reopen authoritative Store")
            .snapshot()
            .expect("snapshot")
            .control_plane_leases
            .len(),
        1
    );
}
