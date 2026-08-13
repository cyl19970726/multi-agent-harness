use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DecodeOutcome, FoldOutcome, ProviderEventFold, ProviderEventFoldError, ProviderObservation,
    TranscriptBatch, TranscriptCursor,
};

#[derive(Debug, Error)]
pub enum ProjectionStoreError {
    #[error("provider event projection I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("provider event projection snapshot is malformed: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error(transparent)]
    Fold(#[from] ProviderEventFoldError),
}

/// Durable bounded projection state. This stores only canonical redacted
/// observations and a resume cursor; provider transcripts remain provider-owned.
pub struct ProjectionStore {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProjectionState {
    pub fold: ProviderEventFold,
    pub transcript_cursor: TranscriptCursor,
}

impl ProviderProjectionState {
    pub fn new(fold: ProviderEventFold) -> Self {
        Self {
            fold,
            transcript_cursor: TranscriptCursor::default(),
        }
    }
}

impl ProjectionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Option<ProviderEventFold>, ProjectionStoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn load_state(&self) -> Result<Option<ProviderProjectionState>, ProjectionStoreError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn ingest(
        &self,
        fold: &mut ProviderEventFold,
        observation: ProviderObservation,
    ) -> Result<FoldOutcome, ProjectionStoreError> {
        // Stage in a clone so a failed durable write cannot advance the live
        // in-memory projection. The caller owns the enclosing Store lock.
        let mut staged = fold.clone();
        let outcome = staged.ingest(observation)?;
        if outcome == FoldOutcome::Inserted {
            self.persist(&staged)?;
            *fold = staged;
        }
        Ok(outcome)
    }

    pub fn persist(&self, fold: &ProviderEventFold) -> Result<(), ProjectionStoreError> {
        self.persist_bytes(&serde_json::to_vec(fold)?)
    }

    pub fn apply_batch(
        &self,
        state: &mut ProviderProjectionState,
        batch: TranscriptBatch,
    ) -> Result<(), ProjectionStoreError> {
        let mut staged = state.clone();
        for outcome in batch.outcomes {
            if let DecodeOutcome::Observation(observation) = outcome {
                staged.fold.ingest(*observation)?;
            }
        }
        staged.transcript_cursor = batch.cursor;
        self.persist_state(&staged)?;
        *state = staged;
        Ok(())
    }

    pub fn persist_state(
        &self,
        state: &ProviderProjectionState,
    ) -> Result<(), ProjectionStoreError> {
        self.persist_bytes(&serde_json::to_vec(state)?)
    }

    fn persist_bytes(&self, bytes: &[u8]) -> Result<(), ProjectionStoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("provider-events.json");
        let temp_path = parent.join(format!(".{file_name}.tmp"));
        let mut temp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(bytes)?;
        temp.sync_all()?;
        fs::rename(&temp_path, &self.path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
