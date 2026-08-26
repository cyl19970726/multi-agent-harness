use super::*;

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
    let principal = harness_application::RoleViewReadPrincipal {
        actor: identity.actor.clone(),
        authority_actors: identity.authority_actors.clone(),
        local_operator: identity.local_operator,
    };
    harness_application::validate_viewer_context_principal(&principal)
        .map_err(|error| ("403 Forbidden", "NOT_AUTHORIZED", error.to_string()))?;
    let facts = Facts::read(space_id, store)
        .map_err(|error| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", error))?;
    let query_facts = harness_application::ViewerContextFacts {
        teams: facts
            .teams
            .iter()
            .map(|team| harness_application::ViewerContextTeamFact {
                team_id: team.id.clone(),
                display_name: team.name.clone(),
                host_agent_member_id: team.host_agent_id.clone(),
                member_agent_member_ids: team.member_ids.clone(),
            })
            .collect(),
        runs: facts
            .runs
            .iter()
            .map(|run| harness_application::ViewerContextRunFact {
                team_run_id: run.id.clone(),
                team_id: run.agent_team_id.clone(),
                updated_at: run.updated_at.clone(),
            })
            .collect(),
        member_runs: facts
            .member_runs
            .iter()
            .enumerate()
            .filter_map(|(source_order, member_run)| {
                Some(harness_application::ViewerContextMemberRunFact {
                    member_run_id: member_run["id"].as_str()?.to_string(),
                    team_run_id: member_run["team_run_id"].as_str()?.to_string(),
                    agent_member_id: member_run["agent_member_id"].as_str()?.to_string(),
                    active: member_run["coordination_status"] == "active",
                    runtime_generation: member_run["runtime_generation"].as_u64()?,
                    source_order,
                })
            })
            .collect(),
    };
    let projection = harness_application::project_viewer_context(&principal, &query_facts)
        .map_err(|error| ("403 Forbidden", "NOT_AUTHORIZED", error.to_string()))?;
    let data = serde_json::to_value(projection).map_err(|error| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            error.to_string(),
        )
    })?;
    Ok(envelope("viewer_context", &facts, data, vec![], vec![]))
}
