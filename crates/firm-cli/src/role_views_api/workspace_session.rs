use super::*;

/// Reading provenance is independent of live Host control authority.
pub(super) fn workspace_session<'a>(
    space_id: &str,
    run: &AgentTeamRun,
    member_run: &Value,
    sessions: &'a [Value],
) -> Option<&'a Value> {
    use harness_core::agentfirm_api::NativeSessionRef;

    let member_run_id = member_run["id"].as_str()?;
    let agent_id = member_run["agent_member_id"].as_str()?;
    if member_run["team_run_id"] != run.id
        || !run.member_run_ids.iter().any(|id| id == member_run_id)
        || (run.host_control_mode == HostControlMode::ExternalInteractive
            && run
                .host_actor
                .as_ref()
                .is_some_and(|actor| actor.id == agent_id))
    {
        return None;
    }
    let native =
        serde_json::from_value::<Option<NativeSessionRef>>(member_run["native_session"].clone())
            .ok()?;
    let mut candidates = sessions.iter().filter(|session| {
        if session["execution_space_id"] != space_id
            || session["node_id"] != run.execution_node_id
            || session["agent_member_id"] != agent_id
        {
            return false;
        }
        let Ok(session_native) = serde_json::from_value::<Option<NativeSessionRef>>(
            session["native_session_ref"].clone(),
        ) else {
            return false;
        };
        match (&native, &session_native) {
            (Some(native), Some(session_native)) => {
                session["provider_kind"] == native.provider
                    && native.same_identity_as(session_native)
            }
            // A newly admitted managed Session may not have settled a native
            // id yet. Preserve its current projection only with exact driver
            // provenance; an external pull-only Host never gains a Session.
            (None, None) => {
                session["lifecycle"] != "closed"
                    && session["control_state"]["execution_driver"] == "host_driven"
                    && session["control_state"]["driver_ref"]["kind"] == "team_supervisor"
                    && session["control_state"]["driver_ref"]["team_run_id"] == run.id
            }
            _ => false,
        }
    });
    let selected = candidates.next()?;
    candidates.next().is_none().then_some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> AgentTeamRun {
        serde_json::from_value(json!({
            "id":"run-a", "agent_team_id":"team-a", "objective":"history",
            "status":"completed", "member_run_ids":["member-a"],
            "execution_node_id":"node-a", "project_binding_id":"project-a",
            "host_control_mode":"managed", "host_actor":{"kind":"host","id":"agent-a"},
            "host_surface":"codex", "created_at":"unix-ms:1", "updated_at":"unix-ms:2"
        }))
        .expect("TeamRun fixture")
    }

    fn native() -> Value {
        json!({"provider":"codex", "execution_mode":"codex_app_server",
            "native_session_id":"native-a", "native_locator_kind":"codex_rollout",
            "provider_version":"0.110.0", "adapter_contract_version":"codex-app-server-v1",
            "availability":"available", "supports_resume":true})
    }

    fn member() -> Value {
        json!({"id":"member-a", "team_run_id":"run-a", "agent_member_id":"agent-a",
            "coordination_status":"closed", "runtime_status":"stopped",
            "runtime_generation":7, "native_session":native()})
    }

    fn session() -> Value {
        json!({"id":"session-a", "execution_space_id":"space-a", "node_id":"node-a",
            "agent_member_id":"agent-a", "provider_kind":"codex", "runtime_generation":2,
            "lifecycle":"closed", "native_session_ref":native(),
            "control_state":{"runtime_residency":"detached", "activity":"idle"}})
    }

    #[test]
    fn closed_member_history_does_not_require_a_live_runtime_or_equal_process_epoch() {
        let sessions = vec![session()];
        let selected = workspace_session("space-a", &run(), &member(), &sessions);
        assert_eq!(selected.map(|s| &s["id"]), Some(&json!("session-a")));
    }

    #[test]
    fn another_run_cannot_borrow_the_same_agents_session() {
        let mut wrong_member = member();
        wrong_member["team_run_id"] = json!("run-b");
        assert!(workspace_session("space-a", &run(), &wrong_member, &[session()]).is_none());
    }

    #[test]
    fn mismatched_identity_or_placement_is_not_a_history_fallback() {
        for (field, value) in [
            ("execution_space_id", "space-b"),
            ("node_id", "node-b"),
            ("agent_member_id", "agent-b"),
            ("provider_kind", "claude"),
        ] {
            let mut wrong = session();
            wrong[field] = json!(value);
            assert!(
                workspace_session("space-a", &run(), &member(), &[wrong]).is_none(),
                "{field}"
            );
        }
        let mut wrong = session();
        wrong["native_session_ref"]["native_session_id"] = json!("native-b");
        assert!(workspace_session("space-a", &run(), &member(), &[wrong]).is_none());
    }

    #[test]
    fn ambiguous_native_binding_is_not_resolved_by_latest_timestamp() {
        let mut other = session();
        other["id"] = json!("session-b");
        assert!(workspace_session("space-a", &run(), &member(), &[session(), other]).is_none());
    }

    #[test]
    fn unbound_external_host_does_not_fabricate_a_session() {
        let mut unbound = member();
        unbound["native_session"] = Value::Null;
        assert!(workspace_session("space-a", &run(), &unbound, &[session()]).is_none());
    }

    #[test]
    fn external_host_cannot_borrow_an_existing_managed_native_session() {
        let mut external = run();
        external.host_control_mode = HostControlMode::ExternalInteractive;
        assert!(workspace_session("space-a", &external, &member(), &[session()]).is_none());
    }

    #[test]
    fn a_pending_managed_session_requires_exact_driver_provenance() {
        let mut pending_member = member();
        pending_member["native_session"] = Value::Null;
        let mut pending = session();
        pending["native_session_ref"] = Value::Null;
        pending["lifecycle"] = json!("cold");
        pending["control_state"]["execution_driver"] = json!("host_driven");
        pending["control_state"]["driver_ref"] =
            json!({"kind":"team_supervisor","team_run_id":"run-a"});
        let sessions = vec![pending.clone()];
        assert!(workspace_session("space-a", &run(), &pending_member, &sessions).is_some());
        pending["control_state"]["driver_ref"]["team_run_id"] = json!("run-b");
        assert!(workspace_session("space-a", &run(), &pending_member, &[pending]).is_none());
    }
}
