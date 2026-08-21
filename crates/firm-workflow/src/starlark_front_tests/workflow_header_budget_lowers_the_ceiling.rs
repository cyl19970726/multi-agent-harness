use super::*;

#[test]
fn workflow_header_budget_lowers_the_ceiling() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    let driver = spending_driver(&calls, 0.6);
    // Header budget_usd=0.5, no CLI budget: step1 runs (spends 0.6); step2 sees
    // 0.6 >= 0.5 and is skipped.
    let script = "workflow(\"demo\", \"declare a tight budget so the run stops early\", budget_usd = 0.5)\nagent(\"a\")\nagent(\"b\")\n";
    let outcome = run_starlark_with_budget(script, "demo", None, &driver, None, None)
        .expect("run ok")
        .outcome;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "only the first step runs");
    assert!(!outcome.steps[1].ok);
}
