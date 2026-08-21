use super::*;

#[test]
fn resume_with_empty_map_dispatches_all() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let script = "\nagent(\"a\")\nagent(\"b\")\n";
    let calls = AtomicUsize::new(0);
    {
        let driver = counting_driver(&calls);
        run_starlark_with_budget(
            &format!("{HEADER}{script}"),
            "demo",
            None,
            &driver,
            None,
            Some(HashMap::new()),
        )
        .expect("run ok");
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "an empty replay map dispatches every leaf, exactly like None"
    );
}
