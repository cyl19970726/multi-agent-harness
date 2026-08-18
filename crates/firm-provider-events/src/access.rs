use thiserror::Error;

use crate::{ProviderEventFold, SessionEventProjection, TeamRuntimeActivity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionAuthority {
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub team_id: String,
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionViewer {
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub team_id: String,
    pub agent_member_id: String,
    pub is_team_host: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectionAccessError {
    #[error("provider event projection belongs to another Execution Space")]
    CrossExecutionSpace,
    #[error("provider event projection belongs to another Project Binding")]
    CrossProjectBinding,
    #[error("provider event projection belongs to another Team")]
    CrossTeam,
    #[error("private Session events require the exact AgentMember owner")]
    NotSessionOwner,
    #[error("projection authority does not match the folded AgentSession generation")]
    StaleSessionAuthority,
}

/// Hosts intentionally do not bypass this boundary. They consume the bounded
/// Team activity projection rather than another Member's private Session view.
pub fn project_private_session(
    fold: &ProviderEventFold,
    authority: &ProjectionAuthority,
    viewer: &ProjectionViewer,
    limit: usize,
) -> Result<SessionEventProjection, ProjectionAccessError> {
    verify_shared_scope(authority, viewer)?;
    if viewer.agent_member_id != authority.agent_member_id {
        return Err(ProjectionAccessError::NotSessionOwner);
    }
    verify_fold(authority, fold)?;
    Ok(fold.session_projection(limit))
}

pub fn project_team_activity(
    fold: &ProviderEventFold,
    authority: &ProjectionAuthority,
    viewer: &ProjectionViewer,
) -> Result<Vec<TeamRuntimeActivity>, ProjectionAccessError> {
    verify_shared_scope(authority, viewer)?;
    verify_fold(authority, fold)?;
    Ok(fold.team_public_projection())
}

fn verify_shared_scope(
    authority: &ProjectionAuthority,
    viewer: &ProjectionViewer,
) -> Result<(), ProjectionAccessError> {
    if viewer.execution_space_id != authority.execution_space_id {
        return Err(ProjectionAccessError::CrossExecutionSpace);
    }
    if viewer.project_binding_id != authority.project_binding_id {
        return Err(ProjectionAccessError::CrossProjectBinding);
    }
    if viewer.team_id != authority.team_id {
        return Err(ProjectionAccessError::CrossTeam);
    }
    Ok(())
}

fn verify_fold(
    authority: &ProjectionAuthority,
    fold: &ProviderEventFold,
) -> Result<(), ProjectionAccessError> {
    if fold.agent_session_id != authority.agent_session_id
        || fold.agent_session_generation != authority.agent_session_generation
    {
        return Err(ProjectionAccessError::StaleSessionAuthority);
    }
    Ok(())
}
