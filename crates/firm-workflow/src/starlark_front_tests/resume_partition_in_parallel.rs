use super::*;

#[test]
fn resume_partition_in_parallel() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Fan out 4 specs (ordinals 0..3), then chain the joined results into a
    // downstream leaf (ordinal 4) so we can prove the script-visible parallel
    // values are the prior run's ORIGINAL summaries (no [replayed] leak).
    let script = r#"
rs = parallel([{"prompt": "fix " + x} for x in ["a", "b", "c", "d"]])
agent("join: " + " | ".join(rs))
"#;
    // First run to mint ordinals 0..4.
    let calls = AtomicUsize::new(0);
    let first = {
        let driver = counting_driver(&calls);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
    };
    assert_eq!(first.outcome.steps.len(), 5);
    let ords: Vec<Option<u64>> = first.outcome.steps.iter().map(|s| s.ordinal).collect();
    assert_eq!(ords, vec![Some(0), Some(1), Some(2), Some(3), Some(4)]);

    // Replay covers ordinals {0, 2} — two of the four fan-out specs.
    let mut replay = HashMap::new();
    replay.insert(0u64, first.outcome.steps[0].clone());
    replay.insert(2u64, first.outcome.steps[2].clone());

    let calls2 = AtomicUsize::new(0);
    let seen2 = Mutex::new(Vec::new());
    let second = {
        let inner = counting_driver(&calls2);
        let driver = |spec: &AgentStepSpec| {
            if spec.prompt.starts_with("join:") {
                seen2.lock().unwrap().push(spec.prompt.clone());
            }
            inner(spec)
        };
        run_starlark_with_budget(
            &format!("{HEADER}{script}"),
            "demo",
            None,
            &driver,
            None,
            Some(replay),
        )
        .expect("run ok")
    };
    assert_eq!(
        calls2.load(Ordering::SeqCst),
        3,
        "the two uncached fan-out specs plus the downstream join leaf are dispatched"
    );
    let steps = &second.outcome.steps;
    assert_eq!(steps.len(), 5);
    // Fan-out results journaled in INPUT order with ordinals 0..3, join is 4.
    let ords2: Vec<Option<u64>> = steps.iter().map(|s| s.ordinal).collect();
    assert_eq!(ords2, vec![Some(0), Some(1), Some(2), Some(3), Some(4)]);
    // The two replayed slots carry the marker on the JOURNALED copy; the two
    // dispatched do not (and neither does the downstream join leaf).
    assert!(steps[0].output_summary.starts_with("[replayed] "));
    assert!(!steps[1].output_summary.starts_with("[replayed] "));
    assert!(steps[2].output_summary.starts_with("[replayed] "));
    assert!(!steps[3].output_summary.starts_with("[replayed] "));
    assert!(!steps[4].output_summary.starts_with("[replayed] "));
    // The SCRIPT-VISIBLE parallel values are the prior run's ORIGINAL summaries:
    // the downstream join leaf's prompt is byte-identical to a non-resumed run,
    // with NO [replayed] marker leaking into the paid worker's prompt.
    let seen2 = seen2.into_inner().unwrap();
    assert_eq!(seen2.len(), 1);
    assert_eq!(
        seen2[0], "join: ok: fix a | ok: fix b | ok: fix c | ok: fix d",
        "the chained parallel results must be the original summaries, got: {}",
        seen2[0]
    );
}
