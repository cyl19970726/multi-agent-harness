use thiserror::Error;

use crate::{
    project_team_activity, project_team_session, read_jsonl_text_page, read_transcript_page,
    DecodeContext, DecodeOutcome, ProjectionAccessError, ProjectionAuthority, ProjectionReadScope,
    ProviderEventFold, ProviderEventFoldError, SessionEventProjection, TeamRuntimeActivity,
    TranscriptReadBoundary, TranscriptReadError,
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
    page_has_more: bool,
    next_before_position: Option<u64>,
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
            page_has_more: false,
            next_before_position: None,
        }
    }

    pub fn refresh_page(
        &mut self,
        boundary: &TranscriptReadBoundary,
        before_position: Option<u64>,
        limit: usize,
    ) -> Result<usize, ProviderProjectionServiceError> {
        let page = read_transcript_page(&self.context, boundary, before_position, limit)?;
        self.replace_with_page(page.outcomes, page.has_more, page.next_before_position)
    }

    pub fn refresh_jsonl_page(
        &mut self,
        content: &str,
        before_position: Option<u64>,
        limit: usize,
    ) -> Result<usize, ProviderProjectionServiceError> {
        let page = read_jsonl_text_page(&self.context, content, before_position, limit)?;
        self.replace_with_page(page.outcomes, page.has_more, page.next_before_position)
    }

    fn replace_with_page(
        &mut self,
        outcomes: Vec<DecodeOutcome>,
        has_more: bool,
        next_before_position: Option<u64>,
    ) -> Result<usize, ProviderProjectionServiceError> {
        self.fold = ProviderEventFold::new(
            &self.context.agent_session_id,
            self.context.agent_session_generation,
            &self.context.node_daemon_id,
            self.context.node_daemon_generation,
        );
        let read_count = outcomes.len();
        for outcome in outcomes {
            let DecodeOutcome::Record(record) = outcome;
            self.fold.ingest(*record)?;
        }
        self.page_has_more = has_more;
        self.next_before_position = next_before_position;
        Ok(read_count)
    }

    pub fn page_metadata(&self, limit: usize) -> serde_json::Value {
        serde_json::json!({
            "limit": limit,
            "has_more": self.page_has_more,
            "next_before_position": self.next_before_position,
        })
    }

    pub fn team_session(
        &self,
        authority: &ProjectionAuthority,
        scope: &ProjectionReadScope,
        limit: usize,
    ) -> Result<SessionEventProjection, ProviderProjectionServiceError> {
        Ok(project_team_session(&self.fold, authority, scope, limit)?)
    }

    pub fn team_activity(
        &self,
        authority: &ProjectionAuthority,
        scope: &ProjectionReadScope,
    ) -> Result<Vec<TeamRuntimeActivity>, ProviderProjectionServiceError> {
        Ok(project_team_activity(&self.fold, authority, scope)?)
    }
}
