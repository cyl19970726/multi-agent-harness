use super::*;

/// Report the TeamRun's Works whose execution authority is provably gone: a
/// started Work whose binding a NodeDaemon settlement invalidated, or a
/// binding frozen on a MemberRun/AgentSession generation that can never pass
/// the runtime fence again. Reported, never repaired — `team-run work
/// recover-lost-execution` is the explicit Host verb — so the Host does not
/// learn this from member complaints (GitHub #799). A Work whose durable
/// facts cannot be read is reported as a scan error and never fails the
/// recovery; a run whose Execution Space cannot be resolved proves nothing
/// about any Work, so it reports none.
pub(super) fn report_lost_execution_works(
    store: &HarnessStore,
    execution_space_id: Option<&str>,
    team_run_id: &str,
    json: bool,
) -> CliResult<harness_store::LostWorkExecutionScan> {
    let scan = match execution_space_id {
        Some(space_id) => store.lost_work_executions(space_id, team_run_id)?,
        None => harness_store::LostWorkExecutionScan::default(),
    };
    if !json {
        for lost in &scan.lost {
            let prerequisite = if lost.condition == harness_core::WorkCondition::Normal {
                String::new()
            } else {
                format!(
                    "resume it first (it is {}; `team-run work resume --work-id {} --expected-version {}`), then ",
                    serde_snake_label(&lost.condition),
                    lost.work_id,
                    lost.work_version
                )
            };
            println!(
                "  work {} (v{}, {}): execution lost [{}] - {}run `team-run work recover-lost-execution --work-id {} --expected-version {}`",
                lost.work_id,
                lost.work_version,
                serde_snake_label(&lost.phase),
                lost.causes.join(", "),
                prerequisite,
                lost.work_id,
                lost.work_version
            );
        }
        for error in &scan.errors {
            println!(
                "  work {}: lost-execution scan could not read its facts: {}",
                error.work_id, error.error
            );
        }
    }
    Ok(scan)
}
