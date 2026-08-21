use super::*;

#[test]
fn workflow_run_script_journals_steps_and_snapshots_source() {
    let store = temp_store("run-script");
    // A two-agent Starlark program that chains output. `--dry-run` returns a
    // mock StepResult per node, so no provider is spawned (CI-safe).
    let script = r#"
workflow("triage", "scan first, then fix what the scan reported so the fix builds on it")
phase("scan")
a = agent("scan " + args["area"])
phase("fix")
agent("fix: " + a, provider = "claude", label = "fixer")
"#;
    let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("src")));
    fs::create_dir_all(&dir).expect("mkdir script dir");
    let path = dir.join("triage.star");
    fs::write(&path, script).expect("write script");

    let args = vec![
        path.display().to_string(),
        "--args".to_string(),
        r#"{"area":"checkout"}"#.to_string(),
        "--dry-run".to_string(),
    ];
    let result = workflow_run_script_value(&store, None, &args).expect("run script");

    // The run completed and references two steps.
    let run = result.get("run").expect("run key");
    assert_eq!(
        run.get("status").and_then(|s| s.as_str()),
        Some("completed")
    );
    // Workflow name defaults to the file stem.
    assert_eq!(
        run.get("workflow_name").and_then(|s| s.as_str()),
        Some("triage")
    );
    let step_ids = run
        .get("step_ids")
        .and_then(|s| s.as_array())
        .expect("step_ids");
    assert_eq!(step_ids.len(), 2);

    // The durable audit record snapshots the raw script text as a starlark spec.
    let runs = store.workflow_runs().expect("read runs");
    let final_run = runs.last().expect("a run row");
    let spec = final_run.spec.as_ref().expect("spec snapshot");
    assert_eq!(spec.get("lang").and_then(|v| v.as_str()), Some("starlark"));
    assert_eq!(spec.get("script").and_then(|v| v.as_str()), Some(script));
    // The mandatory design_intent from the `workflow(...)` header is persisted.
    assert_eq!(
        final_run.design_intent.as_deref(),
        Some("scan first, then fix what the scan reported so the fix builds on it")
    );
    // This was a `--dry-run`, so the journaled run is marked as such — a
    // validation run must be distinguishable from a real one (issue #89 item 2).
    assert!(final_run.dry_run, "dry-run runs are marked dry_run: true");
    // The parsed --args are carried opaquely onto the run.
    assert_eq!(
        final_run
            .args
            .as_ref()
            .and_then(|a| a.get("area"))
            .and_then(|v| v.as_str()),
        Some("checkout")
    );

    // The real driver journals a `running` row at step start and reuses its
    // id for the terminal row, so the append-only log holds running+terminal
    // rows per step. Project latest-wins by id: the two referenced steps must
    // each resolve to a completed terminal row across the distinct phases.
    let all_steps = store.workflow_steps().expect("read steps");
    let referenced: Vec<&str> = step_ids
        .iter()
        .map(|id| id.as_str().expect("step id string"))
        .collect();
    let mut terminal: BTreeMap<&str, &WorkflowStep> = BTreeMap::new();
    for step in &all_steps {
        if referenced.contains(&step.id.as_str()) {
            terminal.insert(step.id.as_str(), step);
        }
    }
    assert_eq!(terminal.len(), 2);
    let phases: BTreeSet<&str> = terminal.values().map(|s| s.phase.as_str()).collect();
    assert_eq!(
        phases,
        BTreeSet::from(["scan", "fix"]),
        "both phases journaled"
    );
    for step in terminal.values() {
        assert_eq!(step.status, WorkflowStepStatus::Completed);
    }
}
