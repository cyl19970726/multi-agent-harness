use std::{
    collections::VecDeque,
    fs,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{decode_native_json_line, DecodeContext, DecodeError, DecodeOutcome, ProviderKind};

const MAX_NATIVE_LINE_BYTES: usize = 1024 * 1024;
const MAX_LATEST_TRANSCRIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TransientReadPosition {
    pub byte_offset: u64,
    pub next_ordering_position: u64,
    #[serde(default)]
    pub active_provider_turn_id: Option<String>,
    /// Process-local continuity for the reviewed Codex JSONL pair where one
    /// authored response is written first as `event_msg.agent_message` and
    /// immediately again as `response_item.message`. Only the exact next
    /// native row may consume this digest, so intentionally repeated replies
    /// from the same family remain distinct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_codex_message_mirror: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptReadBoundary {
    pub allowed_root: PathBuf,
    pub transcript_path: PathBuf,
}

#[derive(Debug, PartialEq)]
pub struct TranscriptBatch {
    pub outcomes: Vec<DecodeOutcome>,
    pub next_position: TransientReadPosition,
    pub incomplete_tail: bool,
}

#[derive(Debug, PartialEq)]
pub struct LatestTranscriptBatch {
    pub outcomes: Vec<DecodeOutcome>,
    pub source_truncated: bool,
    pub incomplete_tail: bool,
}

struct BufferedTranscriptOutcome {
    source_bytes: usize,
    outcome: DecodeOutcome,
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
    #[error("provider transcript line exceeds the bounded adapter limit")]
    LineTooLarge,
    #[error("provider transcript exceeds the bounded on-demand projection limit")]
    SourceTooLarge,
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
    position: TransientReadPosition,
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
    if position.byte_offset > metadata.len() {
        return Err(TranscriptReadError::SourceChanged);
    }
    if max_events == 0 {
        let incomplete_tail = metadata.len() > position.byte_offset;
        return Ok(TranscriptBatch {
            outcomes: vec![],
            next_position: position,
            incomplete_tail,
        });
    }

    let snapshot_len = metadata.len();
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(position.byte_offset))?;
    let remaining = snapshot_len - position.byte_offset;
    let mut reader = BufReader::new(file.take(remaining));
    let mut segment = Vec::new();
    let mut outcomes = Vec::new();
    let mut consumed = 0usize;
    let mut ordering = position.next_ordering_position.max(1);
    let mut active_turn_id = position.active_provider_turn_id.clone();
    let mut pending_codex_message_mirror = position.pending_codex_message_mirror.clone();
    let mut incomplete_tail = false;
    while outcomes.len() < max_events {
        let read = read_bounded_segment(&mut reader, &mut segment)?;
        if read == 0 {
            break;
        }
        if !segment.ends_with(b"\n") {
            incomplete_tail = true;
            break;
        }
        let line_bytes = &segment[..segment.len() - 1];
        let line = std::str::from_utf8(line_bytes).map_err(|_| TranscriptReadError::InvalidUtf8)?;
        if !line.trim().is_empty() {
            let parsed = serde_json::from_str::<serde_json::Value>(line).ok();
            if let Some(turn_id) = parsed.as_ref().and_then(provider_turn_id) {
                active_turn_id = Some(turn_id.to_owned());
            }
            let mut outcome = decode_native_json_line(
                context,
                Some(format!("offset-{}", position.byte_offset + consumed as u64)),
                active_turn_id.clone(),
                ordering,
                None,
                line,
            )?;
            if suppress_codex_mirrored_message(
                context.provider,
                parsed.as_ref(),
                active_turn_id.as_deref(),
                &mut pending_codex_message_mirror,
            ) {
                outcome = DecodeOutcome::Unsupported;
            }
            outcomes.push(outcome);
            if parsed.as_ref().is_some_and(is_turn_terminal) {
                active_turn_id = None;
            }
            ordering += 1;
        }
        consumed += read;
    }
    if !incomplete_tail && outcomes.len() < max_events && consumed as u64 != remaining {
        return Err(TranscriptReadError::SourceChanged);
    }
    Ok(TranscriptBatch {
        outcomes,
        next_position: TransientReadPosition {
            byte_offset: position.byte_offset + consumed as u64,
            next_ordering_position: ordering,
            active_provider_turn_id: active_turn_id,
            pending_codex_message_mirror,
        },
        incomplete_tail: incomplete_tail || (consumed as u64) < remaining,
    })
}

/// Reads the latest complete rows from a provider-owned transcript as one
/// disposable snapshot. The source is scanned in provider order so inherited
/// turn context stays correct, while only a bounded tail is retained in
/// memory. No position returned by this function can be persisted or resumed.
pub fn read_latest_transcript_batch(
    context: &DecodeContext,
    boundary: &TranscriptReadBoundary,
    max_events: usize,
) -> Result<LatestTranscriptBatch, TranscriptReadError> {
    let allowed_root = boundary.allowed_root.canonicalize()?;
    let metadata = fs::symlink_metadata(&boundary.transcript_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TranscriptReadError::InvalidSourceType);
    }
    let path = boundary.transcript_path.canonicalize()?;
    if !path.starts_with(&allowed_root) {
        return Err(TranscriptReadError::SourceEscape);
    }

    // Freeze this request at the length observed before opening. Appends after
    // that boundary belong to a later request, while a concurrent truncation is
    // rejected as a changed provider source.
    let snapshot_len = metadata.len();
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file.take(snapshot_len));
    scan_latest_reader(context, reader, snapshot_len, max_events)
}

/// Decode a bounded JSONL snapshot returned directly by a reviewed provider
/// persistence API. DeepSeek Harness owns its zstd decoding; Harness receives
/// only this response-local logical JSONL and never stores it.
pub fn read_latest_jsonl_text(
    context: &DecodeContext,
    content: &str,
    max_events: usize,
) -> Result<LatestTranscriptBatch, TranscriptReadError> {
    if content.len() > MAX_LATEST_TRANSCRIPT_BYTES {
        return Err(TranscriptReadError::SourceTooLarge);
    }
    let snapshot_len = content.len() as u64;
    scan_latest_reader(
        context,
        BufReader::new(std::io::Cursor::new(content.as_bytes())),
        snapshot_len,
        max_events,
    )
}

fn scan_latest_reader(
    context: &DecodeContext,
    mut reader: impl BufRead,
    snapshot_len: u64,
    max_events: usize,
) -> Result<LatestTranscriptBatch, TranscriptReadError> {
    let mut segment = Vec::new();
    let mut tail = VecDeque::<BufferedTranscriptOutcome>::new();
    let mut tail_bytes = 0usize;
    let mut byte_offset = 0u64;
    let mut ordering_position = 1u64;
    let mut active_turn_id = None;
    let mut pending_codex_message_mirror = None;
    let mut source_truncated = false;
    let mut incomplete_tail = false;

    loop {
        let read = read_bounded_segment(&mut reader, &mut segment)?;
        if read == 0 {
            break;
        }
        let segment_offset = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);
        if !segment.ends_with(b"\n") {
            incomplete_tail = true;
            break;
        }
        let line_bytes = &segment[..segment.len() - 1];
        let line = std::str::from_utf8(line_bytes)
            .map_err(|_| TranscriptReadError::InvalidUtf8)?
            .to_owned();
        if line.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<serde_json::Value>(&line).ok();
        if let Some(turn_id) = parsed.as_ref().and_then(provider_turn_id) {
            active_turn_id = Some(turn_id.to_owned());
        }
        let mut outcome = decode_native_json_line(
            context,
            Some(format!("offset-{segment_offset}")),
            active_turn_id.clone(),
            ordering_position,
            None,
            &line,
        )?;
        if suppress_codex_mirrored_message(
            context.provider,
            parsed.as_ref(),
            active_turn_id.as_deref(),
            &mut pending_codex_message_mirror,
        ) {
            outcome = DecodeOutcome::Unsupported;
        }
        // Unsupported provider metadata and deliberately dropped private
        // reasoning do not consume the visible-history budget. Otherwise a
        // long run of non-projectable native rows could hide the actual latest
        // Message/Tool/Result observations.
        if matches!(&outcome, DecodeOutcome::Observation(_)) {
            tail_bytes = tail_bytes.saturating_add(line.len());
            tail.push_back(BufferedTranscriptOutcome {
                source_bytes: line.len(),
                outcome,
            });
            while tail.len() > max_events || tail_bytes > MAX_LATEST_TRANSCRIPT_BYTES {
                if let Some(discarded) = tail.pop_front() {
                    tail_bytes = tail_bytes.saturating_sub(discarded.source_bytes);
                    source_truncated = true;
                }
            }
        }
        if parsed.as_ref().is_some_and(is_turn_terminal) {
            active_turn_id = None;
        }
        ordering_position = ordering_position.saturating_add(1);
    }
    if byte_offset != snapshot_len {
        return Err(TranscriptReadError::SourceChanged);
    }

    let outcomes = tail.into_iter().map(|row| row.outcome).collect();
    Ok(LatestTranscriptBatch {
        outcomes,
        source_truncated,
        incomplete_tail,
    })
}

fn suppress_codex_mirrored_message(
    provider: ProviderKind,
    raw: Option<&Value>,
    active_turn_id: Option<&str>,
    pending: &mut Option<String>,
) -> bool {
    if provider != ProviderKind::Codex {
        *pending = None;
        return false;
    }
    let Some(raw) = raw else {
        *pending = None;
        return false;
    };
    let row_type = raw.get("type").and_then(Value::as_str);
    let payload_type = raw.pointer("/payload/type").and_then(Value::as_str);
    if row_type == Some("event_msg") && payload_type == Some("agent_message") {
        *pending = raw
            .pointer("/payload/message")
            .and_then(Value::as_str)
            .map(|text| codex_message_mirror_digest(active_turn_id, text));
        return false;
    }
    if row_type == Some("response_item")
        && payload_type == Some("message")
        && raw.pointer("/payload/role").and_then(Value::as_str) == Some("assistant")
    {
        let response_text = codex_response_item_text(raw);
        let mirrored = response_text
            .as_deref()
            .map(|text| codex_message_mirror_digest(active_turn_id, text))
            .is_some_and(|digest| pending.as_deref() == Some(digest.as_str()));
        *pending = None;
        return mirrored;
    }
    *pending = None;
    false
}

fn codex_response_item_text(raw: &Value) -> Option<String> {
    let content = raw.pointer("/payload/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let text = content
        .as_array()?
        .iter()
        .filter_map(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("text" | "output_text")
            )
            .then(|| part.get("text").and_then(Value::as_str))
            .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn codex_message_mirror_digest(active_turn_id: Option<&str>, text: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(active_turn_id.unwrap_or_default().as_bytes());
    digest.update([0]);
    digest.update(text.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn read_bounded_segment(
    reader: &mut impl BufRead,
    segment: &mut Vec<u8>,
) -> Result<usize, TranscriptReadError> {
    segment.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(segment.len());
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if segment.len().saturating_add(take) > MAX_NATIVE_LINE_BYTES {
            return Err(TranscriptReadError::LineTooLarge);
        }
        let terminal = available[take - 1] == b'\n';
        segment.extend_from_slice(&available[..take]);
        reader.consume(take);
        if terminal {
            return Ok(segment.len());
        }
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
