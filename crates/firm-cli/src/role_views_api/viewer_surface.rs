use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ViewerTeamRole {
    Operator,
    Host,
    Member,
}

fn viewer_team_role(identity: &ReadIdentity, team: &AgentTeam) -> Option<ViewerTeamRole> {
    let exact_host = identity.has_agent_member(&team.host_agent_id);
    if exact_host {
        return Some(ViewerTeamRole::Host);
    }
    if team
        .member_ids
        .iter()
        .any(|member_id| identity.has_agent_member(member_id))
    {
        return Some(ViewerTeamRole::Member);
    }
    identity.local_operator.then_some(ViewerTeamRole::Operator)
}

/// Minimal authenticated navigation context for the Dashboard.
///
/// This projection exists so an old bookmark cannot choose a Team before the
/// current local Operator or remote AgentMember context has established Team authority. It
/// exposes no Work, Message, runtime, workspace, or provider-session content.
pub(crate) fn viewer_context_view(
    space_id: &str,
    store: &HarnessStore,
    identity: Option<&ReadIdentity>,
) -> ViewResult {
    let identity = identity.ok_or((
        "401 Unauthorized",
        "NOT_AUTHORIZED",
        "ViewerContext requires local Operator or authenticated AgentMember authority".into(),
    ))?;
    if !identity.local_operator && identity.actor.kind != ActorKind::AgentMember {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "ViewerContext requires local Operator or authenticated AgentMember authority".into(),
        ));
    }
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let mut teams = facts
        .teams
        .iter()
        .filter_map(|team| {
            let viewer_role = viewer_team_role(identity, team)?;
            let latest_run = facts.latest_run(&team.id);
            let mut team_run_ids = facts
                .runs
                .iter()
                .filter(|run| run.agent_team_id == team.id)
                .map(|run| run.id.as_str())
                .collect::<Vec<_>>();
            team_run_ids.sort_unstable();
            let viewer_agent_id = if matches!(
                viewer_role,
                ViewerTeamRole::Operator | ViewerTeamRole::Host
            ) {
                team.host_agent_id.as_str()
            } else {
                team.member_ids
                    .iter()
                    .find(|member_id| identity.has_agent_member(member_id))
                    .map(String::as_str)
                    .expect("Member viewer role requires exact Team Member authority")
            };
            let current_member_run_id = latest_run.and_then(|run| {
                facts
                    .member_runs
                    .iter()
                    .filter(|member_run| {
                        member_run["team_run_id"] == run.id
                            && member_run["agent_member_id"] == viewer_agent_id
                            && member_run["coordination_status"] == "active"
                    })
                    .max_by_key(|member_run| {
                        member_run["runtime_generation"]
                            .as_u64()
                            .unwrap_or_default()
                    })
                    .and_then(|member_run| member_run["id"].as_str())
            });
            Some(json!({
                "team_id": team.id,
                "display_name": team.name,
                "viewer_role": match viewer_role { ViewerTeamRole::Operator => "operator", ViewerTeamRole::Host => "host", ViewerTeamRole::Member => "member" },
                "viewer_agent_member_id": viewer_agent_id,
                "default_conversation": if matches!(viewer_role, ViewerTeamRole::Operator | ViewerTeamRole::Host) { "host" } else { viewer_agent_id },
                "latest_run_id": latest_run.map(|run| run.id.as_str()),
                "team_run_ids": team_run_ids,
                "current_member_run_id": current_member_run_id,
            }))
        })
        .collect::<Vec<_>>();
    teams.sort_by(|left, right| left["team_id"].as_str().cmp(&right["team_id"].as_str()));
    let data = json!({
        "viewer_actor_ref": {
            "kind": enum_string(&identity.actor.kind),
            "id": identity.actor.id,
        },
        "teams": teams,
    });
    Ok(envelope("viewer_context", &facts, data, vec![], vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::agentfirm_api::ActorRef;
    use harness_core::AgentTeamStatus;

    fn team() -> AgentTeam {
        AgentTeam {
            id: "team-a".into(),
            name: "Team A".into(),
            description: String::new(),
            node_id: "node-a".into(),
            status: AgentTeamStatus::Active,
            revision: 1,
            legacy_mission_id: None,
            trashed_at: None,
            mission_id: "legacy-mission-a".into(),
            host_agent_id: "host-a".into(),
            member_ids: vec!["member-a".into()],
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    fn identity(id: &str) -> ReadIdentity {
        ReadIdentity {
            actor: ActorRef {
                kind: ActorKind::AgentMember,
                id: id.into(),
            },
            authority_actors: Vec::new(),
            local_operator: false,
        }
    }

    #[test]
    fn viewer_team_role_is_exact_and_team_scoped() {
        assert_eq!(
            viewer_team_role(&identity("host-a"), &team()),
            Some(ViewerTeamRole::Host)
        );
        assert_eq!(
            viewer_team_role(&identity("member-a"), &team()),
            Some(ViewerTeamRole::Member)
        );
        assert_eq!(viewer_team_role(&identity("foreign-member"), &team()), None);
        let delegated_member = ReadIdentity {
            actor: ActorRef {
                kind: ActorKind::Service,
                id: "remote-machine".into(),
            },
            authority_actors: vec![ActorRef {
                kind: ActorKind::AgentMember,
                id: "member-a".into(),
            }],
            local_operator: false,
        };
        assert_eq!(
            viewer_team_role(&delegated_member, &team()),
            Some(ViewerTeamRole::Member)
        );
        let mut local_operator = identity("local-dashboard-operator");
        local_operator.local_operator = true;
        assert_eq!(
            viewer_team_role(&local_operator, &team()),
            Some(ViewerTeamRole::Operator)
        );
    }
}
