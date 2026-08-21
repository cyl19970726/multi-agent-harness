use super::*;

#[test]
fn team_run_materializes_canonical_member_runs_in_its_execution_space() {
    let (store, root) = temp_store("canonical-member-run-materialization");
    let created = create_two_member_team_run(&store);
    let canonical = store
        .trust_member_runs("unit-test-space")
        .expect("canonical MemberRuns");
    assert_eq!(canonical.len(), created.member_runs.len());
    for runtime in &created.member_runs {
        let projection = canonical
            .iter()
            .find(|candidate| candidate.id == runtime.id)
            .expect("runtime has canonical ProviderRuntimeProjection projection");
        assert_eq!(projection.agent_member_id, runtime.agent_member_id);
        assert_eq!(projection.team_run_id, created.team_run.id);
        assert_eq!(projection.runtime_generation, runtime.runtime_generation);
    }
    std::fs::remove_dir_all(root).expect("cleanup");
}
