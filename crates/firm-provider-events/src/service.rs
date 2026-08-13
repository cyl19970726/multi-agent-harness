use thiserror::Error;

use crate::{
    project_private_session, project_team_activity, read_transcript_batch, DecodeContext,
    ProjectionAccessError, ProjectionAuthority, ProjectionStore, ProjectionStoreError,
    ProjectionViewer, ProviderEventFold, ProviderProjectionState, SessionEventProjection,
    TeamRuntimeActivity, TranscriptReadBoundary, TranscriptReadError,
};

#[derive(Debug, Error)]
pub enum ProviderProjectionServiceError {
    #[error("persisted projection belongs to a stale Session or NodeDaemon generation")]
    StaleAuthority,
    #[error(transparent)]
    Read(#[from] TranscriptReadError),
    #[error(transparent)]
    Store(#[from] ProjectionStoreError),
    #[error(transparent)]
    Access(#[from] ProjectionAccessError),
}

/// Thin application service for RoleView/API integration. Callers resolve the
/// exact AgentSession, NodeDaemon and viewer from canonical Stores; this layer
/// owns only provider-source normalization and projection state.
pub struct ProviderProjectionService {
    store: ProjectionStore,
    context: DecodeContext,
    state: ProviderProjectionState,
}

impl ProviderProjectionService {
    pub fn open(
        store: ProjectionStore,
        context: DecodeContext,
    ) -> Result<Self, ProviderProjectionServiceError> {
        let state = match store.load_state()? {
            Some(state) => {
                if state.fold.agent_session_id != context.agent_session_id
                    || state.fold.agent_session_generation != context.agent_session_generation
                    || state.fold.node_daemon_id != context.node_daemon_id
                    || state.fold.node_daemon_generation != context.node_daemon_generation
                {
                    return Err(ProviderProjectionServiceError::StaleAuthority);
                }
                state
            }
            None => ProviderProjectionState::new(ProviderEventFold::new(
                &context.agent_session_id,
                context.agent_session_generation,
                &context.node_daemon_id,
                context.node_daemon_generation,
            )),
        };
        Ok(Self {
            store,
            context,
            state,
        })
    }

    pub fn refresh(
        &mut self,
        boundary: &TranscriptReadBoundary,
        max_events: usize,
    ) -> Result<usize, ProviderProjectionServiceError> {
        let batch = read_transcript_batch(
            &self.context,
            boundary,
            self.state.transcript_cursor.clone(),
            max_events,
        )?;
        let read_count = batch.outcomes.len();
        if read_count > 0 || batch.cursor != self.state.transcript_cursor {
            self.store.apply_batch(&mut self.state, batch)?;
        }
        Ok(read_count)
    }

    pub fn private_session(
        &self,
        authority: &ProjectionAuthority,
        viewer: &ProjectionViewer,
        limit: usize,
    ) -> Result<SessionEventProjection, ProviderProjectionServiceError> {
        Ok(project_private_session(
            &self.state.fold,
            authority,
            viewer,
            limit,
        )?)
    }

    pub fn team_activity(
        &self,
        authority: &ProjectionAuthority,
        viewer: &ProjectionViewer,
    ) -> Result<Vec<TeamRuntimeActivity>, ProviderProjectionServiceError> {
        Ok(project_team_activity(&self.state.fold, authority, viewer)?)
    }

    pub fn cursor(&self) -> &crate::TranscriptCursor {
        &self.state.transcript_cursor
    }
}
