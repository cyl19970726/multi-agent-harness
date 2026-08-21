use super::*;

#[test]
fn resume_reuses_cached_leaves_and_skips_driver() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // A 3-serial-agent program where leaf 2 chains leaf 1's output into its
    // prompt — so we can prove the CACHED result flows back into the script.
    let script = r#"
a = agent("scan the code")
b = agent("step two: " + a)
c = agent("step three: " + b)
"#;
    // First run: capture every StepResult's ordinal (0,1,2) and outputs.
    let calls = AtomicUsize::new(0);
    let first = {
        let driver = counting_driver(&calls);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok")
    };
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "first run dispatches all 3"
    );
    assert_eq!(first.outcome.steps.len(), 3);
    assert_eq!(first.outcome.steps[0].ordinal, Some(0));
    assert_eq!(first.outcome.steps[1].ordinal, Some(1));
    assert_eq!(first.outcome.steps[2].ordinal, Some(2));

    // Build a replay map covering ordinals {0, 1} from the first run.
    let mut replay = HashMap::new();
    replay.insert(0u64, first.outcome.steps[0].clone());
    replay.insert(1u64, first.outcome.steps[1].clone());

    // Second run: resume. The driver must run EXACTLY ONCE (only leaf 2).
    let calls2 = AtomicUsize::new(0);
    let seen2 = Mutex::new(Vec::new());
    let second = {
        let inner = counting_driver(&calls2);
        let driver = |spec: &AgentStepSpec| {
            seen2.lock().unwrap().push(spec.prompt.clone());
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
        1,
        "only the uncached leaf (ordinal 2) is dispatched"
    );
    let steps = &second.outcome.steps;
    assert_eq!(steps.len(), 3);
    // Cached leaves carry the [replayed] marker + details flag.
    for i in [0usize, 1] {
        assert_eq!(steps[i].ordinal, Some(i as u64));
        assert!(
            steps[i].output_summary.starts_with("[replayed] "),
            "cached leaf {i} output: {}",
            steps[i].output_summary
        );
        assert_eq!(steps[i].details.as_ref().unwrap()["replayed"], true);
    }
    // Leaf 2 was freshly dispatched (no marker).
    assert_eq!(steps[2].ordinal, Some(2));
    assert!(!steps[2].output_summary.starts_with("[replayed] "));
    // The cached result flowed back into the script WITHOUT the [replayed]
    // marker: the script-visible value must be the prior run's ORIGINAL summary,
    // so downstream prompts are byte-identical to the first run (no corruption,
    // no control-flow divergence). The marker lives only on the journaled copy.
    let seen2 = seen2.into_inner().unwrap();
    assert_eq!(seen2.len(), 1);
    assert_eq!(
        seen2[0], "step three: ok: step two: ok: scan the code",
        "leaf 2 prompt must chain the cached leaf-1 ORIGINAL summary, got: {}",
        seen2[0]
    );
    assert!(
        !seen2[0].contains("[replayed]"),
        "the replay marker must NOT leak into the script-visible value, got: {}",
        seen2[0]
    );
}
