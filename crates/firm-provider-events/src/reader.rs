use std::{
    collections::VecDeque,
    fs,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{decode_native_json_line, DecodeContext, DecodeError, DecodeOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptReadBoundary {
    pub allowed_root: PathBuf,
    pub transcript_path: PathBuf,
}

#[derive(Debug, PartialEq)]
pub struct TranscriptPage {
    pub outcomes: Vec<DecodeOutcome>,
    pub has_more: bool,
    pub next_before_position: Option<u64>,
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
    #[error("provider transcript changed behind the disposable read position")]
    SourceChanged,
    #[error("provider transcript is not UTF-8")]
    InvalidUtf8,
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

/// Reads one response-local page without publishing the provider filesystem
/// path. Complete native rows are preserved exactly; an incomplete final row
/// remains provider-owned and is retried by a later request.
pub fn read_transcript_page(
    context: &DecodeContext,
    boundary: &TranscriptReadBoundary,
    before_position: Option<u64>,
    limit: usize,
) -> Result<TranscriptPage, TranscriptReadError> {
    let allowed_root = boundary.allowed_root.canonicalize()?;
    let metadata = fs::symlink_metadata(&boundary.transcript_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TranscriptReadError::InvalidSourceType);
    }
    let path = boundary.transcript_path.canonicalize()?;
    if !path.starts_with(&allowed_root) {
        return Err(TranscriptReadError::SourceEscape);
    }
    let snapshot_len = metadata.len();
    let file = fs::File::open(path)?;
    scan_page_reader(
        context,
        BufReader::new(file.take(snapshot_len)),
        snapshot_len,
        before_position,
        limit,
    )
}

pub fn read_jsonl_text_page(
    context: &DecodeContext,
    content: &str,
    before_position: Option<u64>,
    limit: usize,
) -> Result<TranscriptPage, TranscriptReadError> {
    scan_page_reader(
        context,
        BufReader::new(std::io::Cursor::new(content.as_bytes())),
        content.len() as u64,
        before_position,
        limit,
    )
}

fn scan_page_reader(
    context: &DecodeContext,
    mut reader: impl BufRead,
    snapshot_len: u64,
    before_position: Option<u64>,
    limit: usize,
) -> Result<TranscriptPage, TranscriptReadError> {
    let before = before_position.unwrap_or(u64::MAX);
    let limit = limit.max(1);
    let mut segment = Vec::new();
    let mut tail = VecDeque::<DecodeOutcome>::new();
    let mut byte_offset = 0u64;
    let mut ordering_position = 1u64;
    let mut active_turn_id = None;
    let mut incomplete_tail = false;
    loop {
        segment.clear();
        let read = reader.read_until(b'\n', &mut segment)?;
        if read == 0 {
            break;
        }
        let segment_offset = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);
        if !segment.ends_with(b"\n") {
            incomplete_tail = true;
            break;
        }
        let line = std::str::from_utf8(&segment[..segment.len() - 1])
            .map_err(|_| TranscriptReadError::InvalidUtf8)?;
        if line.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<serde_json::Value>(line).ok();
        if let Some(turn_id) = parsed.as_ref().and_then(provider_turn_id) {
            active_turn_id = Some(turn_id.to_owned());
        }
        let outcome = decode_native_json_line(
            context,
            Some(format!("offset-{segment_offset}")),
            active_turn_id.clone(),
            ordering_position,
            None,
            line,
        )?;
        if ordering_position < before {
            tail.push_back(outcome);
            while tail.len() > limit.saturating_add(1) {
                tail.pop_front();
            }
        }
        if parsed.as_ref().is_some_and(is_turn_terminal) {
            active_turn_id = None;
        }
        ordering_position = ordering_position.saturating_add(1);
    }
    if byte_offset != snapshot_len && !incomplete_tail {
        return Err(TranscriptReadError::SourceChanged);
    }
    let has_more = tail.len() > limit;
    if has_more {
        tail.pop_front();
    }
    let outcomes = tail.into_iter().collect::<Vec<_>>();
    let next_before_position = has_more
        .then(|| outcomes.first().and_then(outcome_position))
        .flatten();
    Ok(TranscriptPage {
        outcomes,
        has_more,
        next_before_position,
        incomplete_tail,
    })
}

fn outcome_position(outcome: &DecodeOutcome) -> Option<u64> {
    match outcome {
        DecodeOutcome::Observation(observation) => Some(observation.ordering_position),
    }
}

fn provider_turn_id(value: &serde_json::Value) -> Option<&str> {
    value
        .pointer("/payload/turn_id")
        .or_else(|| value.pointer("/turn_id"))
        .or_else(|| value.pointer("/turnId"))
        .or_else(|| value.pointer("/event/turn_id"))
        .or_else(|| value.pointer("/event/turnId"))
        .and_then(|value| value.as_str())
}

fn is_turn_terminal(value: &serde_json::Value) -> bool {
    if value.get("type").and_then(|value| value.as_str()) == Some("turn/end") {
        return true;
    }
    if value.get("type").and_then(|value| value.as_str()) == Some("context.append_loop_event")
        && value
            .pointer("/event/type")
            .and_then(|value| value.as_str())
            == Some("step.end")
        && value
            .pointer("/event/finishReason")
            .and_then(|value| value.as_str())
            == Some("end_turn")
    {
        return true;
    }
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
                | "turn.cancel"
                | "turn_cancelled"
                | "turn/cancelled"
        )
    )
}

pub fn is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}
