use super::*;

/// Managed members whose Team-scoped runtime has not been explicitly closed.
///
/// TeamRun completion is coordination state, not provider-runtime teardown.
/// External-interactive members have no daemon-owned adapter, while Closed and
/// Retired members have already left the managed lane.
pub(super) fn unclosed_managed_members(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Vec<ProviderRuntimeProjection>> {
    Ok(latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| {
            member.team_run_id == team_run_id
                && !member.is_external_interactive()
                && member.coordination_is_active()
        })
        .collect())
}

pub(super) fn completed_serving_label(unclosed_members: usize) -> String {
    format!("completed ({unclosed_members} unclosed member(s))")
}
