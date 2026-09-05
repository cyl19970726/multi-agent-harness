use super::*;

pub(super) const COMPLETED_RUN_SERVING_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(super) fn is_unclosed_managed_member(
    member: &ProviderRuntimeProjection,
    team_run_id: &str,
) -> bool {
    member.team_run_id == team_run_id
        && !member.is_external_interactive()
        && member.coordination_is_active()
}

pub(super) fn unclosed_managed_member_count(
    members: &[ProviderRuntimeProjection],
    team_run_id: &str,
) -> usize {
    members
        .iter()
        .filter(|member| is_unclosed_managed_member(member, team_run_id))
        .count()
}

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
        .filter(|member| is_unclosed_managed_member(member, team_run_id))
        .collect())
}

pub(super) fn completed_serving_label(unclosed_members: usize) -> String {
    format!("completed ({unclosed_members} unclosed member(s))")
}
