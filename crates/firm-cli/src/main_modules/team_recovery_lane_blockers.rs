use super::*;

/// The Blocked, untyped members `team-run recover` could not return to a
/// startable status because their lane does not prove its runtime gone, each
/// with the exact clause that blocks it. Reported, never repaired: the Host
/// reconciles the named condition (detach the handle, settle the ambiguous
/// RuntimeCommand) and runs recover again, instead of reading "skipped" or a
/// success line for a lane Close will refuse (GitHub #841). Members the
/// repair loop already explained (`already_reported`) are not listed twice,
/// and the rows are re-read so the loop's own writes are not judged stale.
pub(super) fn report_blocked_lanes_not_proven(
    store: &HarnessStore,
    execution_space_id: Option<&str>,
    team_run_id: &str,
    already_reported: &[serde_json::Value],
    json: bool,
) -> CliResult<Vec<serde_json::Value>> {
    let Some(space_id) = execution_space_id else {
        return Ok(Vec::new());
    };
    let mut reported = Vec::new();
    for member in latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
    {
        if !(member.coordination_is_active()
            && member.status == MemberRunStatus::Blocked
            && !member.is_external_interactive()
            && blocked_member_provenance(&member) == BlockedMemberProvenance::Untyped)
        {
            continue;
        }
        if already_reported
            .iter()
            .any(|entry| entry["member_run_id"].as_str() == Some(member.id.as_str()))
        {
            continue;
        }
        let Some(blocker) = member_lane_blocker(store, space_id, &member) else {
            continue;
        };
        if !json {
            println!(
                "  {} ({}): blocked, not restarted — {}",
                member.name, member.provider, blocker
            );
        }
        reported.push(serde_json::json!({
            "member_run_id": member.id,
            "name": member.name,
            "blocker": blocker,
        }));
    }
    Ok(reported)
}
