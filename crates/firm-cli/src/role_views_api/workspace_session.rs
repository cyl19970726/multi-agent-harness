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
        // The AgentSession owns this pointer and the MemberRun row projects it
        // (ADR 0072), so the session's ref decides the match and the MemberRun
        // copy is only a guard: it can disqualify a session by naming a
        // different conversation, never by merely lagging behind one.
        match (&native, &session_native) {
            (Some(native), Some(session_native)) => {
                session["provider_kind"] == native.provider
                    && native.same_identity_as(session_native)
            }
            // The authority has bound and the projection has not caught up.
            // Before the reader redirect this dropped the session entirely,
            // which is exactly deciding from a projection.
            //
            // Scoped to the SAME runtime generation on purpose. A lagging
            // projection is a within-generation race: the bind is generation
            // fenced, so the authority and the MemberRun always share one. A
            // member with no pointer must still not borrow an older
            // generation's session, which is what `unbound_external_host_does_
            // not_fabricate_a_session` pins.
            //
            // `member_run` is a canonical `MemberRun`, which has no `provider`
            // field — only `provider_profile_snapshot`, whose real values are
            // `"<provider>/<execution_mode>"` ("kimi/kimi_acp",
            // "claude/claude_agent_sdk"). Comparing against a key that does not
            // exist is how the first version of this arm compared a String to
            // Null and never fired.
            (None, Some(_)) => {
                session["runtime_generation"] == member_run["runtime_generation"]
                    && member_run["provider_profile_snapshot"]
                        .as_str()
                        .and_then(|snapshot| snapshot.split('/').next())
                        .is_none_or(|provider| session["provider_kind"] == provider)
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
            // The MemberRun names a conversation no session owns. The authority
            // has not bound it, so this session is not its lane.
            (Some(_), None) => false,
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

    /// The authority has bound and the MemberRun projection has not caught up.
    /// The first version of this arm compared `member_run["provider"]`, a key
    /// canonical MemberRuns do not have, so it was `String == Null` — always
    /// false, and the session was still dropped.
    #[test]
    fn a_bound_session_is_selected_while_the_member_run_projection_still_lags() {
        let mut lagging = member();
        lagging["native_session"] = Value::Null;
        lagging["provider_profile_snapshot"] = json!("codex/codex_app_server");
        lagging["runtime_generation"] = json!(2);
        let sessions = vec![session()];
        let selected = workspace_session("space-a", &run(), &lagging, &sessions);
        assert_eq!(
            selected.map(|s| &s["id"]),
            Some(&json!("session-a")),
            "a lagging projection must not drop a session the authority has bound"
        );
    }

    /// The snapshot is optional on real rows, and an absent one must not
    /// silently disqualify a lane the authority owns.
    #[test]
    fn a_lagging_projection_without_a_provider_snapshot_still_selects() {
        let mut lagging = member();
        lagging["native_session"] = Value::Null;
        lagging["runtime_generation"] = json!(2);
        let sessions = vec![session()];
        assert_eq!(
            workspace_session("space-a", &run(), &lagging, &sessions).map(|s| &s["id"]),
            Some(&json!("session-a"))
        );
    }

    /// Widening that arm must not widen the fences that precede it.
    #[test]
    fn a_lagging_projection_does_not_reach_across_member_node_or_space() {
        let mut lagging = member();
        lagging["native_session"] = Value::Null;
        lagging["provider_profile_snapshot"] = json!("codex/codex_app_server");
        lagging["runtime_generation"] = json!(2);

        let mut foreign_generation = session();
        foreign_generation["runtime_generation"] = json!(3);
        assert!(
            workspace_session("space-a", &run(), &lagging, &[foreign_generation]).is_none(),
            "a member with no pointer must not borrow another generation's session"
        );

        let mut foreign_member = session();
        foreign_member["agent_member_id"] = json!("agent-other");
        assert!(workspace_session("space-a", &run(), &lagging, &[foreign_member]).is_none());

        let mut foreign_node = session();
        foreign_node["node_id"] = json!("node-other");
        assert!(workspace_session("space-a", &run(), &lagging, &[foreign_node]).is_none());

        let foreign_space = session();
        assert!(workspace_session("space-other", &run(), &lagging, &[foreign_space]).is_none());

        let mut foreign_provider = session();
        foreign_provider["provider_kind"] = json!("kimi");
        assert!(
            workspace_session("space-a", &run(), &lagging, &[foreign_provider]).is_none(),
            "a snapshot that names another provider still disqualifies"
        );
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
