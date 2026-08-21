use super::*;

    /// The same dangling reference with NO legacy `teams.jsonl` backing row is
    /// genuine corruption (or a post-cutover write bug): the snapshot keeps
    /// failing closed.
    #[test]
    fn dashboard_snapshot_still_fails_closed_on_unbacked_dangling_team_reference() {
        let store = temp_store("snapshot-dangling-team-fails");
        append_jsonl_value(
            &store.root().join("node_project_registrations.jsonl"),
            &serde_json::json!({
                "node_id": "node-dangling",
                "execution_space_id": "space-dangling",
                "project_binding_id": "project-dangling",
                "status": "active",
                "created_at": "unix-ms:1",
                "updated_at": "unix-ms:1"
            }),
        )
        .expect("append registration");
        append_jsonl_value(
            &store.root().join("team_runs.jsonl"),
            &serde_json::json!({
                "id": "team-run-dangling",
                "agent_team_id": "team-never-existed",
                "execution_node_id": "node-dangling",
                "project_binding_id": "project-dangling",
                "host_surface": "test",
                "objective": "dangling run",
                "status": "completed",
                "created_at": "unix-ms:1",
                "updated_at": "unix-ms:1"
            }),
        )
        .expect("append dangling TeamRun");

        let error = dashboard_snapshot(&store).expect_err("unbacked dangling ref fails closed");
        assert!(
            error
                .to_string()
                .contains("TeamRun references a missing AgentTeam"),
            "unexpected error: {error}"
        );
    }

