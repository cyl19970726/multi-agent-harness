use super::*;

/// Report the TeamRun's Works whose execution authority is provably gone: a
/// started Work whose binding a NodeDaemon settlement invalidated, or a
/// binding frozen on a MemberRun/AgentSession generation that can never pass
/// the runtime fence again. Reported, never repaired — `team-run work
/// recover-lost-execution` is the explicit Host verb — so the Host does not
/// learn this from member complaints (GitHub #799). A run whose Execution
/// Space cannot be resolved proves nothing about any Work, so it reports none.
pub(super) fn report_lost_execution_works(
    store: &HarnessStore,
    execution_space_id: Option<&str>,
    team_run_id: &str,
    json: bool,
) -> CliResult<Vec<harness_store::LostWorkExecution>> {
    let lost_execution_works = match execution_space_id {
        Some(space_id) => store.lost_work_executions(space_id, team_run_id)?,
        None => Vec::new(),
    };
    if !json {
        for lost in &lost_execution_works {
            println!(
                "  work {} (v{}, {}): execution lost [{}] - run `team-run work recover-lost-execution --work-id {} --expected-version {}`",
                lost.work_id,
                lost.work_version,
                serde_snake_label(&lost.phase),
                lost.causes.join(", "),
                lost.work_id,
                lost.work_version
            );
        }
    }
    Ok(lost_execution_works)
}
