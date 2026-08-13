use thiserror::Error;

use crate::{
    project_private_session, project_team_activity, read_latest_transcript_batch,
    read_transcript_batch, DecodeContext, DecodeOutcome, ProjectionAccessError,
    ProjectionAuthority, ProjectionViewer, ProviderEventFold, ProviderEventFoldError,
    SessionEventProjection, TeamRuntimeActivity, TranscriptReadBoundary, TranscriptReadError,
    TransientReadPosition,
};

#[derive(Debug, Error)]
pub enum ProviderProjectionServiceError {
    #[error(transparent)]
    Read(#[from] TranscriptReadError),
    #[error(transparent)]
    Fold(#[from] ProviderEventFoldError),
    #[error(transparent)]
    Access(#[from] ProjectionAccessError),
}

/// Thin application service for RoleView/API integration. Callers resolve the
/// exact AgentSession, NodeDaemon and viewer from canonical Stores; this layer
/// owns only provider-source normalization and projection state.
pub struct ProviderProjectionService {
    context: DecodeContext,
    fold: ProviderEventFold,
    transient_position: TransientReadPosition,
    source_truncated: bool,
}

impl ProviderProjectionService {
    /// Opens a disposable, process-local projection. Provider-native storage is
    /// the sole history: no cursor, fold, observation, or snapshot is written
    /// by this service and a process restart intentionally starts from zero.
    pub fn open(context: DecodeContext) -> Self {
        let fold = ProviderEventFold::new(
            &context.agent_session_id,
            context.agent_session_generation,
            &context.node_daemon_id,
            context.node_daemon_generation,
        );
        Self {
            context,
            fold,
            transient_position: TransientReadPosition::default(),
            source_truncated: false,
        }
    }

    pub fn refresh(
        &mut self,
        boundary: &TranscriptReadBoundary,
        max_events: usize,
    ) -> Result<usize, ProviderProjectionServiceError> {
        let batch = read_transcript_batch(
            &self.context,
            boundary,
            self.transient_position.clone(),
            max_events,
        )?;
        let read_count = batch.outcomes.len();
        for outcome in batch.outcomes {
            if let DecodeOutcome::Observation(observation) = outcome {
                self.fold.ingest(*observation)?;
            }
        }
        self.transient_position = batch.next_position;
        Ok(read_count)
    }

    /// Replaces this disposable fold with the latest bounded snapshot. This is
    /// the historical RoleView path: it never exposes or persists a cursor.
    pub fn refresh_latest(
        &mut self,
        boundary: &TranscriptReadBoundary,
        max_events: usize,
    ) -> Result<usize, ProviderProjectionServiceError> {
        let batch = read_latest_transcript_batch(&self.context, boundary, max_events)?;
        self.fold = ProviderEventFold::new(
            &self.context.agent_session_id,
            self.context.agent_session_generation,
            &self.context.node_daemon_id,
            self.context.node_daemon_generation,
        );
        let read_count = batch.outcomes.len();
        for outcome in batch.outcomes {
            if let DecodeOutcome::Observation(observation) = outcome {
                self.fold.ingest(*observation)?;
            }
        }
        self.transient_position = TransientReadPosition::default();
        self.source_truncated = batch.source_truncated;
        Ok(read_count)
    }

    pub fn private_session(
        &self,
        authority: &ProjectionAuthority,
        viewer: &ProjectionViewer,
        limit: usize,
    ) -> Result<SessionEventProjection, ProviderProjectionServiceError> {
        let mut projection = project_private_session(&self.fold, authority, viewer, limit)?;
        projection.truncated |= self.source_truncated;
        Ok(projection)
    }

    pub fn team_activity(
        &self,
        authority: &ProjectionAuthority,
        viewer: &ProjectionViewer,
    ) -> Result<Vec<TeamRuntimeActivity>, ProviderProjectionServiceError> {
        Ok(project_team_activity(&self.fold, authority, viewer)?)
    }

    pub fn transient_position(&self) -> &TransientReadPosition {
        &self.transient_position
    }
}
