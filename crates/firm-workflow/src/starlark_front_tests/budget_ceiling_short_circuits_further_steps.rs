use super::*;

#[test]
fn budget_ceiling_short_circuits_further_steps() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    let driver = spending_driver(&calls, 0.6);
    // CLI budget $1.00: step1 (spent 0 -> runs, 0.6), step2 (0.6 -> runs, 1.2),
    // step3 (1.2 >= 1.0 -> SKIPPED). The driver dispatches exactly twice.
    let script = "\nagent(\"a\")\nagent(\"b\")\nagent(\"c\")\n";
    let outcome = run_starlark_with_budget(
        &format!("{HEADER}{script}"),
        "demo",
        None,
        &driver,
        Some(1.0),
        None,
    )
    .expect("run ok")
    .outcome;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the third step must be skipped once the budget is reached"
    );
    assert_eq!(outcome.steps.len(), 3);
    assert!(outcome.steps[0].ok && outcome.steps[1].ok);
    assert!(!outcome.steps[2].ok, "third step is a budget skip");
    assert!(outcome.steps[2].output_summary.contains("budget"));
}
