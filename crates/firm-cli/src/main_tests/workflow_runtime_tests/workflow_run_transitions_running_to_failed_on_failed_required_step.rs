use super::*;

    #[test]
    fn workflow_run_transitions_running_to_failed_on_failed_required_step() {
        let store = temp_store("failed");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        // Mock driver: the required serial "scope" step fails; audits succeed.
        let driver = |spec: &workflow::AgentStepSpec| {
            let ok = spec.phase != "scope";
            workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok,
                output_summary: "mock".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        };

        let run_id = generated_id("wfrun");
        let result =
            run_workflow_with_driver(&store, &run_id, def, "failure Y", false, None, &driver)
                .expect("run workflow");
        let run = result.get("run").expect("run key");
        assert_eq!(run.get("status").and_then(|s| s.as_str()), Some("failed"));

        let runs = store.workflow_runs().expect("read runs");
        assert_eq!(runs[0].status, WorkflowRunStatus::Running);
        assert_eq!(runs.last().unwrap().status, WorkflowRunStatus::Failed);

        // All three steps are still journaled (parallel barrier collected nulls).
        let steps = store.workflow_steps().expect("read steps");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].status, WorkflowStepStatus::Failed);
        assert_eq!(steps[1].status, WorkflowStepStatus::Completed);
        assert_eq!(steps[2].status, WorkflowStepStatus::Completed);
    }

