use super::*;
use harness_core::agentfirm_api::{
    MemberRun as AgentMemberRun, NativeSessionRef as AgentNativeSessionRef,
};

#[derive(Clone, Copy)]
pub(super) struct CurrentWorkRuntime<'a> {
    pub(super) binding: &'a Value,
    pub(super) session: &'a Value,
    pub(super) member_run: &'a Value,
    pub(super) workspace: Option<&'a Value>,
}

/// Resolve current execution only through the canonical stable-responsibility
/// join. Historical `Work.active_member_run_id` is a conflict signal, never a
/// fallback. Every cardinality or generation mismatch leaves the Work without
/// a projected runtime rather than guessing an authority.
pub(super) fn current_work_runtime<'a>(
    facts: &'a Facts,
    work: &Work,
) -> Option<CurrentWorkRuntime<'a>> {
    if work.active_member_run_id.is_some() {
        return None;
    }
    let membership_id = work.assignee_membership_id.as_deref()?;
    let agent_member_id = work.owner_member_id.as_deref()?;
    let responsibility_changed = facts.work_events.iter().any(|event| {
        event["work_id"] == work.id
            && event["resulting_version"].as_u64().is_some_and(|version| {
                let binding_revision = facts
                    .work_execution_bindings
                    .iter()
                    .find(|binding| binding["work_id"] == work.id && binding["status"] == "active")
                    .and_then(|binding| binding["work_revision"].as_u64())
                    .unwrap_or(u64::MAX);
                version > binding_revision
            })
            && matches!(
                event["kind"].as_str(),
                Some("assigned" | "claimed" | "released" | "rebound" | "execution_retargeted")
            )
    });
    if responsibility_changed {
        return None;
    }
    let active_bindings = facts
        .work_execution_bindings
        .iter()
        .filter(|binding| {
            binding["work_id"] == work.id
                && binding["status"] == "active"
                && binding["team_membership_id"] == membership_id
                && binding["agent_member_id"] == agent_member_id
                && binding["work_revision"]
                    .as_u64()
                    .is_some_and(|revision| revision <= work.version)
        })
        .collect::<Vec<_>>();
    let [binding] = active_bindings.as_slice() else {
        return None;
    };
    let sessions = facts
        .agent_sessions
        .iter()
        .filter(|session| {
            session["id"] == binding["agent_session_id"]
                && session["agent_member_id"] == agent_member_id
                && session["runtime_generation"] == binding["agent_session_generation"]
                && session["lifecycle"] != "closed"
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return None;
    };
    let admissions = facts
        .work_execution_runtime_bindings
        .iter()
        .filter(|admission| admission["binding_id"] == binding["id"])
        .collect::<Vec<_>>();
    let [admission] = admissions.as_slice() else {
        return None;
    };
    let runtime_binding = &admission["runtime_binding"];
    let current_runs = facts
        .member_runs
        .iter()
        .filter(|member_run| {
            let Ok(current_run) = serde_json::from_value::<AgentMemberRun>((*member_run).clone())
            else {
                return false;
            };
            member_run["team_run_id"] == work.team_run_id
                && member_run["agent_member_id"] == agent_member_id
                && current_run.has_live_runtime_authority()
                && native_session_identity_matches(
                    &member_run["native_session"],
                    &session["native_session_ref"],
                )
                && runtime_binding["target_member_run_id"] == member_run["id"]
                && runtime_binding["target_member_run_generation"]
                    == member_run["runtime_generation"]
                && runtime_binding["target_session_id"] == session["id"]
                && runtime_binding["target_runtime_generation"] == session["runtime_generation"]
        })
        .collect::<Vec<_>>();
    let [member_run] = current_runs.as_slice() else {
        return None;
    };
    let workspace = current_workspace(facts, member_run["id"].as_str().unwrap_or_default()).filter(
        |workspace| workspace["attached_member_generation"] == member_run["runtime_generation"],
    );
    Some(CurrentWorkRuntime {
        binding,
        session,
        member_run,
        workspace,
    })
}

fn native_session_identity_matches(left: &Value, right: &Value) -> bool {
    match (
        serde_json::from_value::<AgentNativeSessionRef>(left.clone()),
        serde_json::from_value::<AgentNativeSessionRef>(right.clone()),
    ) {
        (Ok(left), Ok(right)) => {
            harness_core::agentfirm_api::native_session_identity_matches(&left, &right)
        }
        _ => left.is_null() && right.is_null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Work {
        let mut draft = harness_core::CurrentWorkDraft::new(
            "work-runtime-projection".into(),
            "team-run-runtime-projection".into(),
            "team-runtime-projection".into(),
            "Runtime projection".into(),
            String::new(),
            "Project only exact current execution".into(),
            harness_core::WorkClaimMode::HostAssign,
            harness_core::WorkPriority::Normal,
            harness_core::TeamActorRef {
                kind: harness_core::TeamActorKind::Host,
                id: "host".into(),
                display_name: None,
                authn_source: None,
            },
            "t1".into(),
        );
        draft.assignee_membership_id = Some("membership-worker".into());
        draft.owner_member_id = Some("worker".into());
        let mut work = Work::from_current_draft(draft);
        work.version = 2;
        work
    }

    fn facts(runtime_status: &str, workspace_generation: u64) -> Facts {
        Facts {
            space_id: "space".into(),
            store_identity: "store".into(),
            sequence: 0,
            work_sequence: 0,
            team_sequence: 0,
            run_sequence: 0,
            team_revisions: BTreeMap::new(),
            run_revisions: BTreeMap::new(),
            teams: Vec::new(),
            runs: Vec::new(),
            works: Vec::new(),
            members: Vec::new(),
            member_runs: vec![json!({
                "id":"member-run-worker",
                "team_run_id":"team-run-runtime-projection",
                "agent_member_id":"worker",
                "coordination_status":"active",
                "runtime_status":runtime_status,
                "runtime_generation":2,
                "native_session":null
            })],
            provider_runtime_projections: Vec::new(),
            messages: Vec::new(),
            message_deliveries: Vec::new(),
            agent_identities: Vec::new(),
            agent_sessions: vec![json!({
                "id":"session-worker",
                "agent_member_id":"worker",
                "runtime_generation":7,
                "lifecycle":"idle",
                "native_session_ref":null
            })],
            team_memberships: Vec::new(),
            message_subscriptions: Vec::new(),
            work_execution_bindings: vec![json!({
                "id":"binding-worker",
                "work_id":"work-runtime-projection",
                "work_revision":2,
                "team_membership_id":"membership-worker",
                "agent_member_id":"worker",
                "agent_session_id":"session-worker",
                "agent_session_generation":7,
                "status":"active"
            })],
            work_execution_runtime_bindings: vec![json!({
                "binding_id":"binding-worker",
                "runtime_binding":{
                    "target_member_run_id":"member-run-worker",
                    "target_member_run_generation":2,
                    "target_session_id":"session-worker",
                    "target_runtime_generation":7
                }
            })],
            canonical_messages: Vec::new(),
            canonical_message_deliveries: Vec::new(),
            runtime_commands: Vec::new(),
            work_deliveries: Vec::new(),
            work_events: Vec::new(),
            side: vec![json!({
                "id":"workspace-worker",
                "member_run_id":"member-run-worker",
                "attached_member_generation":workspace_generation,
                "canonical_root":"/tmp/workspace",
                "lifecycle":"attached",
                "version":1,
                "updated_at":"t2"
            })],
        }
    }

    #[test]
    fn terminal_runtime_and_stale_workspace_are_not_projected_as_current() {
        assert!(current_work_runtime(&facts("completed", 2), &work()).is_none());
        let stale_workspace_facts = facts("idle", 1);
        let stale_workspace = current_work_runtime(&stale_workspace_facts, &work())
            .expect("exact non-terminal runtime remains current");
        assert!(
            stale_workspace.workspace.is_none(),
            "workspace from another MemberRun generation is not current"
        );
        assert!(
            current_work_runtime(&facts("idle", 2), &work())
                .expect("exact current runtime")
                .workspace
                .is_some(),
            "exact MemberRun generation projects its workspace"
        );
    }
}
