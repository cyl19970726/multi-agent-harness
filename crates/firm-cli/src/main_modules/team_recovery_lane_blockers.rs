use super::*;

/// The Blocked, untyped members `team-run recover` could not return to a
/// startable status because their lane does not prove its runtime gone, each
/// with the exact clause that blocks it. Reported, never repaired: the Host
/// reconciles the named condition (detach the handle, settle the ambiguous
/// RuntimeCommand) and runs recover again, instead of reading "skipped" or
/// a success line for a lane Close will refuse (GitHub #841).
pub(super) fn report_blocked_lanes_not_proven(
    store: &HarnessStore,
    execution_space_id: Option<&str>,
    members: &[ProviderRuntimeProjection],
    json: bool,
) -> Vec<serde_json::Value> {
    let Some(space_id) = execution_space_id else {
        return Vec::new();
    };
    let mut reported = Vec::new();
    for member in members {
        if !(member.coordination_is_active()
            && member.status == MemberRunStatus::Blocked
            && !member.is_external_interactive()
            && blocked_member_provenance(member) == BlockedMemberProvenance::Untyped)
        {
            continue;
        }
        let Some(blocker) = member_lane_blocker(store, space_id, member) else {
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
    reported
}
