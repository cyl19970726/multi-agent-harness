use super::*;

    #[test]
    fn dashboard_snapshot_includes_workflow_keys() {
        let store = temp_store("snapshot");
        // Empty store: keys must still be present (additive, inspectable).
        let snapshot = dashboard_snapshot(&store).expect("snapshot");
        assert!(snapshot.get("workflow_runs").is_some());
        assert!(snapshot.get("workflow_steps").is_some());

        // After a run, the keys surface the journaled rows.
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("registered");
        let driver = |spec: &workflow::AgentStepSpec| ok_step(spec);
        let run_id = generated_id("wfrun");
        run_workflow_with_driver(&store, &run_id, def, "x", false, None, &driver).expect("run");

        let snapshot = dashboard_snapshot(&store).expect("snapshot");
        let runs = snapshot
            .get("workflow_runs")
            .and_then(|v| v.as_array())
            .expect("workflow_runs array");
        assert_eq!(runs.len(), 1, "latest-wins projection collapses to one run");
        assert_eq!(
            runs[0].get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let steps = snapshot
            .get("workflow_steps")
            .and_then(|v| v.as_array())
            .expect("workflow_steps array");
        assert_eq!(steps.len(), 3);
    }

