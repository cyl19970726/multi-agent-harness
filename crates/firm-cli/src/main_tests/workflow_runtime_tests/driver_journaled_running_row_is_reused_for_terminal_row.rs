use super::*;

    /// LIVE PROGRESS contract: when a driver journals a `running` step row at
    /// step start (carrying its `step_id` + real `started_at`), the runtime
    /// REUSES that identity for the terminal row. The append log then holds two
    /// rows per step (running -> completed), but the latest-wins projection
    /// collapses to one terminal row whose `started_at` is the driver's real
    /// start time — never overwritten by the journal time. This is what lets the
    /// SSE watcher stream a `running` frame as each step starts.
    #[test]
    fn driver_journaled_running_row_is_reused_for_terminal_row() {
        let store = temp_store("live-progress");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        let run_id = generated_id("wfrun");

        // A driver that mimics the real path: journal a `running` row up front,
        // then return a StepResult carrying that same id + start time.
        let driver = |spec: &workflow::AgentStepSpec| {
            let step_id = generated_id("wfstep");
            let started_at = format!("unix-ms:{}", 1_000 + spec.label.len());
            let running = WorkflowStep {
                id: step_id.clone(),
                run_id: run_id.clone(),
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                native_session: None,
                status: WorkflowStepStatus::Running,
                output_summary: None,
                result: None,
                started_at: started_at.clone(),
                ended_at: None,
                terminal_reason: None,
                partial: false,
            };
            store
                .append_workflow_step(&running)
                .expect("journal running");
            let result = workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                output_summary: format!("ok: {}", spec.label),
                step_id: Some(step_id.clone()),
                started_at: Some(started_at.clone()),
                details: None,
                structured: None,
                ordinal: None,
            };
            // Mirror the real driver under the live-per-step contract: also
            // journal the TERMINAL row at completion, reusing the same step_id +
            // start time. `run_workflow_with_driver` must then NOT re-journal it.
            store
                .append_workflow_step(&build_terminal_step(&run_id, step_id, started_at, &result))
                .expect("journal terminal");
            result
        };

        let result = run_workflow_with_driver(&store, &run_id, def, "topic", false, None, &driver)
            .expect("run workflow");
        assert_eq!(
            result
                .get("run")
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str()),
            Some("completed")
        );

        // Raw append log: the driver journaled a `running` row at start AND the
        // terminal row at completion (2 rows x 3 steps = 6). run_workflow_with_driver
        // recognises the driver-journaled terminal (step_id is Some) and does NOT
        // re-journal — so the count stays 6, not 9.
        let appended = store.workflow_steps().expect("read step log");
        assert_eq!(
            appended.len(),
            6,
            "driver journals running + terminal per step; finalize does not re-journal"
        );
        assert_eq!(
            appended
                .iter()
                .filter(|s| s.status == WorkflowStepStatus::Running)
                .count(),
            3,
            "a running row was journaled at the start of each step (live progress)"
        );

        // Latest-wins projection: exactly 3 terminal rows, each reusing the
        // driver's start time rather than the journal-time stamp.
        let steps = latest_workflow_steps_in_append_order(&store).expect("project steps");
        assert_eq!(
            steps.len(),
            3,
            "running+terminal collapse to one row per step"
        );
        for step in &steps {
            assert_eq!(step.status, WorkflowStepStatus::Completed);
            assert!(
                step.started_at.starts_with("unix-ms:1"),
                "terminal row kept the driver's real start time: {}",
                step.started_at
            );
            assert!(step.ended_at.is_some());
        }
    }
