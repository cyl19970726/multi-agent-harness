use serde_json::Value;

/// Resolve the exact MemberRun accountable for one initial Work from stable
/// AgentMember responsibility. TeamRun response ordering is presentation only;
/// managed Hosts and requested members may appear in either position.
pub fn member_run_for_work_owner(result: &Value, work_index: usize) -> &Value {
    let work = result["works"]
        .as_array()
        .and_then(|works| works.get(work_index))
        .expect("initial Work response");
    let owner_member_id = work["owner_member_id"]
        .as_str()
        .expect("initial Work owner AgentMember");
    let matches = result["member_runs"]
        .as_array()
        .expect("TeamRun MemberRuns")
        .iter()
        .filter(|member| member["agent_member_id"].as_str() == Some(owner_member_id))
        .collect::<Vec<_>>();
    let [member] = matches.as_slice() else {
        panic!("initial Work owner must resolve exactly one MemberRun: {result}");
    };
    member
}
