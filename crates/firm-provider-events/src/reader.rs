use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{decode_native_json_line, DecodeContext, DecodeError, DecodeOutcome};

const MAX_NATIVE_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TranscriptCursor {
    pub byte_offset: u64,
    pub next_ordering_position: u64,
    #[serde(default)]
    pub active_provider_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptReadBoundary {
    pub allowed_root: PathBuf,
    pub transcript_path: PathBuf,
}

#[derive(Debug, PartialEq)]
pub struct TranscriptBatch {
    pub outcomes: Vec<DecodeOutcome>,
    pub cursor: TranscriptCursor,
    pub incomplete_tail: bool,
}

#[derive(Debug, Error)]
pub enum TranscriptReadError {
    #[error("provider transcript I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("provider transcript is outside its server-owned root")]
    SourceEscape,
    #[error("provider transcript must be a regular non-symlink file")]
    InvalidSourceType,
    #[error("provider transcript was truncated behind the durable cursor")]
    CursorBeyondEnd,
    #[error("provider transcript line exceeds the bounded adapter limit")]
    LineTooLarge,
    #[error("provider transcript is not UTF-8")]
    InvalidUtf8,
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

/// Incrementally reads an append-only provider source without publishing its
/// filesystem path. Incomplete final lines remain unconsumed for the next poll.
pub fn read_transcript_batch(
    context: &DecodeContext,
    boundary: &TranscriptReadBoundary,
    cursor: TranscriptCursor,
    max_events: usize,
) -> Result<TranscriptBatch, TranscriptReadError> {
    let allowed_root = boundary.allowed_root.canonicalize()?;
    let metadata = fs::symlink_metadata(&boundary.transcript_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TranscriptReadError::InvalidSourceType);
    }
    let path = boundary.transcript_path.canonicalize()?;
    if !path.starts_with(&allowed_root) {
        return Err(TranscriptReadError::SourceEscape);
    }
    if cursor.byte_offset > metadata.len() {
        return Err(TranscriptReadError::CursorBeyondEnd);
    }
    if max_events == 0 {
        let incomplete_tail = metadata.len() > cursor.byte_offset;
        return Ok(TranscriptBatch {
            outcomes: vec![],
            cursor,
            incomplete_tail,
        });
    }

    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(cursor.byte_offset))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut outcomes = Vec::new();
    let mut consumed = 0usize;
    let mut ordering = cursor.next_ordering_position.max(1);
    let mut active_turn_id = cursor.active_provider_turn_id.clone();
    for segment in bytes.split_inclusive(|byte| *byte == b'\n') {
        if outcomes.len() >= max_events || !segment.ends_with(b"\n") {
            break;
        }
        if segment.len() > MAX_NATIVE_LINE_BYTES {
            return Err(TranscriptReadError::LineTooLarge);
        }
        let line_bytes = &segment[..segment.len() - 1];
        let line = std::str::from_utf8(line_bytes).map_err(|_| TranscriptReadError::InvalidUtf8)?;
        if !line.trim().is_empty() {
            let parsed = serde_json::from_str::<serde_json::Value>(line).ok();
            if let Some(turn_id) = parsed.as_ref().and_then(provider_turn_id) {
                active_turn_id = Some(turn_id.to_owned());
            }
            outcomes.push(decode_native_json_line(
                context,
                Some(format!("offset-{}", cursor.byte_offset + consumed as u64)),
                active_turn_id.clone(),
                ordering,
                None,
                line,
            )?);
            if parsed.as_ref().is_some_and(is_turn_terminal) {
                active_turn_id = None;
            }
            ordering += 1;
        }
        consumed += segment.len();
    }
    Ok(TranscriptBatch {
        outcomes,
        cursor: TranscriptCursor {
            byte_offset: cursor.byte_offset + consumed as u64,
            next_ordering_position: ordering,
            active_provider_turn_id: active_turn_id,
        },
        incomplete_tail: consumed < bytes.len(),
    })
}

fn provider_turn_id(value: &serde_json::Value) -> Option<&str> {
    value
        .pointer("/payload/turn_id")
        .or_else(|| value.pointer("/turn_id"))
        .and_then(|value| value.as_str())
}

fn is_turn_terminal(value: &serde_json::Value) -> bool {
    matches!(
        value
            .pointer("/payload/type")
            .or_else(|| value.get("type"))
            .and_then(|value| value.as_str()),
        Some(
            "task_complete"
                | "turn_completed"
                | "turn/completed"
                | "turn_failed"
                | "turn/failed"
                | "turn_cancelled"
                | "turn/cancelled"
        )
    )
}

pub fn is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}
