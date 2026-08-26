//! Typed ViewerContext RoleView application policy.
//!
//! Adapters authenticate transport credentials and collect canonical facts.
//! This module decides which Teams that principal may navigate and selects the
//! deterministic latest TeamRun/current MemberRun references. It owns no
//! persistence, transport, clock, provider, or JSON representation.

use std::fmt;

use firm_core::agentfirm_api::{ActorKind, ActorRef};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleViewReadPrincipal {
    pub actor: ActorRef,
    pub authority_actors: Vec<ActorRef>,
    pub local_operator: bool,
}

impl RoleViewReadPrincipal {
    fn has_agent_member(&self, agent_member_id: &str) -> bool {
        let matches =
            |actor: &ActorRef| actor.kind == ActorKind::AgentMember && actor.id == agent_member_id;
        matches(&self.actor) || self.authority_actors.iter().any(matches)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerContextTeamFact {
    pub team_id: String,
    pub display_name: String,
    pub host_agent_member_id: String,
    pub member_agent_member_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerContextRunFact {
    pub team_run_id: String,
    pub team_id: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerContextMemberRunFact {
    pub member_run_id: String,
    pub team_run_id: String,
    pub agent_member_id: String,
    pub active: bool,
    pub runtime_generation: u64,
    /// Canonical adapter order used only to preserve last-row tie behavior for
    /// historical duplicate generations.
    pub source_order: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewerContextFacts {
    pub teams: Vec<ViewerContextTeamFact>,
    pub runs: Vec<ViewerContextRunFact>,
    pub member_runs: Vec<ViewerContextMemberRunFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerContextTeamRole {
    Operator,
    Host,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewerContextTeamProjection {
    pub team_id: String,
    pub display_name: String,
    pub viewer_role: ViewerContextTeamRole,
    pub viewer_agent_member_id: String,
    pub default_conversation: String,
    pub latest_run_id: Option<String>,
    pub team_run_ids: Vec<String>,
    pub current_member_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewerContextProjection {
    pub viewer_actor_ref: ActorRef,
    pub teams: Vec<ViewerContextTeamProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerContextQueryError {
    UnsupportedPrincipal,
}

impl fmt::Display for ViewerContextQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "ViewerContext requires local Operator or authenticated AgentMember authority",
        )
    }
}

impl std::error::Error for ViewerContextQueryError {}

pub fn validate_viewer_context_principal(
    principal: &RoleViewReadPrincipal,
) -> Result<(), ViewerContextQueryError> {
    if principal.local_operator || principal.actor.kind == ActorKind::AgentMember {
        Ok(())
    } else {
        Err(ViewerContextQueryError::UnsupportedPrincipal)
    }
}

/// Project the minimal authenticated Dashboard navigation context.
pub fn project_viewer_context(
    principal: &RoleViewReadPrincipal,
    facts: &ViewerContextFacts,
) -> Result<ViewerContextProjection, ViewerContextQueryError> {
    validate_viewer_context_principal(principal)?;

    let mut teams = facts
        .teams
        .iter()
        .filter_map(|team| {
            let (viewer_role, viewer_agent_member_id) =
                if principal.has_agent_member(&team.host_agent_member_id) {
                    (
                        ViewerContextTeamRole::Host,
                        team.host_agent_member_id.clone(),
                    )
                } else if let Some(member_id) = team
                    .member_agent_member_ids
                    .iter()
                    .find(|member_id| principal.has_agent_member(member_id))
                {
                    (ViewerContextTeamRole::Member, member_id.clone())
                } else if principal.local_operator {
                    (
                        ViewerContextTeamRole::Operator,
                        team.host_agent_member_id.clone(),
                    )
                } else {
                    return None;
                };

            let latest_run = facts
                .runs
                .iter()
                .filter(|run| run.team_id == team.team_id)
                .max_by(|left, right| {
                    left.updated_at
                        .cmp(&right.updated_at)
                        .then(left.team_run_id.cmp(&right.team_run_id))
                });
            let mut team_run_ids = facts
                .runs
                .iter()
                .filter(|run| run.team_id == team.team_id)
                .map(|run| run.team_run_id.clone())
                .collect::<Vec<_>>();
            team_run_ids.sort_unstable();
            let current_member_run_id = latest_run.and_then(|run| {
                facts
                    .member_runs
                    .iter()
                    .filter(|member_run| {
                        member_run.team_run_id == run.team_run_id
                            && member_run.agent_member_id == viewer_agent_member_id
                            && member_run.active
                    })
                    .max_by(|left, right| {
                        left.runtime_generation
                            .cmp(&right.runtime_generation)
                            .then(left.source_order.cmp(&right.source_order))
                    })
                    .map(|member_run| member_run.member_run_id.clone())
            });
            let default_conversation = match viewer_role {
                ViewerContextTeamRole::Operator | ViewerContextTeamRole::Host => "host".into(),
                ViewerContextTeamRole::Member => viewer_agent_member_id.clone(),
            };
            Some(ViewerContextTeamProjection {
                team_id: team.team_id.clone(),
                display_name: team.display_name.clone(),
                viewer_role,
                viewer_agent_member_id,
                default_conversation,
                latest_run_id: latest_run.map(|run| run.team_run_id.clone()),
                team_run_ids,
                current_member_run_id,
            })
        })
        .collect::<Vec<_>>();
    teams.sort_by(|left, right| left.team_id.cmp(&right.team_id));

    Ok(ViewerContextProjection {
        viewer_actor_ref: principal.actor.clone(),
        teams,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(kind: ActorKind, id: &str) -> RoleViewReadPrincipal {
        RoleViewReadPrincipal {
            actor: ActorRef {
                kind,
                id: id.into(),
            },
            authority_actors: Vec::new(),
            local_operator: false,
        }
    }

    fn facts() -> ViewerContextFacts {
        ViewerContextFacts {
            teams: vec![
                ViewerContextTeamFact {
                    team_id: "team-b".into(),
                    display_name: "Team B".into(),
                    host_agent_member_id: "host-b".into(),
                    member_agent_member_ids: vec!["member-b".into()],
                },
                ViewerContextTeamFact {
                    team_id: "team-a".into(),
                    display_name: "Team A".into(),
                    host_agent_member_id: "host-a".into(),
                    member_agent_member_ids: vec!["member-a".into()],
                },
            ],
            runs: vec![
                ViewerContextRunFact {
                    team_run_id: "run-old".into(),
                    team_id: "team-a".into(),
                    updated_at: "t1".into(),
                },
                ViewerContextRunFact {
                    team_run_id: "run-new".into(),
                    team_id: "team-a".into(),
                    updated_at: "t2".into(),
                },
            ],
            member_runs: vec![
                ViewerContextMemberRunFact {
                    member_run_id: "member-run-1".into(),
                    team_run_id: "run-new".into(),
                    agent_member_id: "member-a".into(),
                    active: true,
                    runtime_generation: 1,
                    source_order: 0,
                },
                ViewerContextMemberRunFact {
                    member_run_id: "member-run-2".into(),
                    team_run_id: "run-new".into(),
                    agent_member_id: "member-a".into(),
                    active: true,
                    runtime_generation: 2,
                    source_order: 1,
                },
            ],
        }
    }

    #[test]
    fn foreign_member_gets_an_authenticated_zero_match_projection() {
        let projection =
            project_viewer_context(&principal(ActorKind::AgentMember, "foreign"), &facts())
                .unwrap();
        assert!(projection.teams.is_empty());
    }

    #[test]
    fn host_wins_over_member_and_delegated_authority_is_exact() {
        let mut principal = principal(ActorKind::AgentMember, "member-a");
        principal.authority_actors.push(ActorRef {
            kind: ActorKind::AgentMember,
            id: "host-a".into(),
        });
        let projection = project_viewer_context(&principal, &facts()).unwrap();
        assert_eq!(projection.teams.len(), 1);
        assert_eq!(projection.teams[0].viewer_role, ViewerContextTeamRole::Host);
        assert_eq!(projection.teams[0].viewer_agent_member_id, "host-a");
    }

    #[test]
    fn member_projection_selects_latest_run_and_highest_active_generation() {
        let projection =
            project_viewer_context(&principal(ActorKind::AgentMember, "member-a"), &facts())
                .unwrap();
        let team = &projection.teams[0];
        assert_eq!(team.viewer_role, ViewerContextTeamRole::Member);
        assert_eq!(team.latest_run_id.as_deref(), Some("run-new"));
        assert_eq!(team.team_run_ids, ["run-new", "run-old"]);
        assert_eq!(team.current_member_run_id.as_deref(), Some("member-run-2"));
        assert_eq!(team.default_conversation, "member-a");
    }

    #[test]
    fn local_operator_sees_every_team_in_deterministic_order() {
        let mut principal = principal(ActorKind::Service, "local-dashboard-operator");
        principal.local_operator = true;
        let projection = project_viewer_context(&principal, &facts()).unwrap();
        assert_eq!(
            projection
                .teams
                .iter()
                .map(|team| team.team_id.as_str())
                .collect::<Vec<_>>(),
            ["team-a", "team-b"]
        );
        assert!(projection
            .teams
            .iter()
            .all(|team| team.viewer_role == ViewerContextTeamRole::Operator));
    }

    #[test]
    fn remote_service_cannot_turn_delegated_identity_into_viewer_authority() {
        let mut principal = principal(ActorKind::Service, "remote-service");
        principal.authority_actors.push(ActorRef {
            kind: ActorKind::AgentMember,
            id: "member-a".into(),
        });
        assert_eq!(
            project_viewer_context(&principal, &facts()),
            Err(ViewerContextQueryError::UnsupportedPrincipal)
        );
    }
}
