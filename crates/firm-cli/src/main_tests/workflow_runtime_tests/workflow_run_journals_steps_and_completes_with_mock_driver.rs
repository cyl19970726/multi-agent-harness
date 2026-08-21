use super::*;

    #[test]
    fn workflow_run_journals_steps_and_completes_with_mock_driver() {
        let store = temp_store("complete");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        // Mock driver: never spawns a provider; always succeeds.
        let driver = |spec: &workflow::AgentStepSpec| ok_step(spec);

        let run_id = generated_id("wfrun");
        let result =
            run_workflow_with_driver(&store, &run_id, def, "failure X", false, None, &driver)
                .expect("run workflow");

        // The returned run is completed and references 3 steps (serial + 2 parallel).
        let run = result.get("run").expect("run key");
        assert_eq!(
            run.get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let step_ids = run
            .get("step_ids")
            .and_then(|s| s.as_array())
            .expect("step_ids");
        assert_eq!(step_ids.len(), 3);

        // The journal holds two WorkflowRun rows (running -> completed) for one id.
        let runs = store.workflow_runs().expect("read runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].status, WorkflowRunStatus::Running);
        assert_eq!(runs[1].status, WorkflowRunStatus::Completed);
        assert_eq!(runs[0].id, runs[1].id);

        // Three dry-run steps journaled. No provider ran, so no native session
        // may be invented merely to make the dashboard look live.
        let steps = store.workflow_steps().expect("read steps");
        assert_eq!(steps.len(), 3);
        for step in &steps {
            assert_eq!(step.status, WorkflowStepStatus::Completed);
            assert_eq!(step.run_id, runs[0].id);
            assert!(step.native_session.is_none());
            assert!(step.ended_at.is_some());
        }
        // The serial step is first, in the "scope" phase.
        assert_eq!(steps[0].phase, "scope");
        assert_eq!(steps[1].phase, "audit");
        assert_eq!(steps[2].phase, "audit");
    }

