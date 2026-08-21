use super::*;

#[test]
fn resume_replayed_leaf_does_not_advance_spend() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Without replay and a $1.00 budget, three $0.6 leaves would skip leaf 2
    // (0 -> 0.6 -> 1.2 >= 1.0). With leaves 0 and 1 REPLAYED (no spend), leaf 2
    // is the FIRST real dispatch (spent still 0), so it runs instead of skipping.
    let script = "\nagent(\"a\")\nagent(\"b\")\nagent(\"c\")\n";
    // First normal run with a spending driver to mint cached results.
    let calls0 = AtomicUsize::new(0);
    let first = {
        let driver = spending_driver(&calls0, 0.6);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
    };
    let mut replay = HashMap::new();
    replay.insert(0u64, first.outcome.steps[0].clone());
    replay.insert(1u64, first.outcome.steps[1].clone());

    let calls = AtomicUsize::new(0);
    let second = {
        let driver = spending_driver(&calls, 0.6);
        run_starlark_with_budget(
            &format!("{HEADER}{script}"),
            "demo",
            None,
            &driver,
            Some(1.0),
            Some(replay),
        )
        .expect("run ok")
    };
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "leaf 2 dispatches because replayed leaves cost $0 (no re-spend)"
    );
    assert!(
        second.outcome.steps[2].ok && !second.outcome.steps[2].output_summary.contains("budget"),
        "leaf 2 ran rather than being budget-skipped"
    );
}
