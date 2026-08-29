use std::{
    collections::VecDeque,
    fs,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{OrderingKeyKind, PersistedNativeRow, PersistedOrderingKey, ProviderKind};

use super::projector::{
    PersistedProjectionContext, PersistedProjectionError, PersistedProjectionSeed,
    PersistedSessionProjector,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedReaderSource {
    pub provider: ProviderKind,
    pub native_session_id: String,
    pub source_family: String,
    pub format_version_fence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedFileBoundary {
    pub allowed_root: PathBuf,
    pub transcript_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedRowPage {
    pub native_source_ref: String,
    pub source_generation: String,
    pub rows: Vec<PersistedNativeRow>,
    pub snapshot_watermark: Option<PersistedOrderingKey>,
    pub has_more: bool,
    pub next_before: Option<PersistedOrderingKey>,
    pub incomplete_tail: bool,
    projection_seed: PersistedProjectionSeed,
}

impl PersistedRowPage {
    /// Builds a projector primed by the exact rows preceding this page. This
    /// keeps call-id → tool-name joins stable across backward pagination
    /// without copying provider payloads into the cursor or Harness state.
    pub fn projector(
        &self,
        context: PersistedProjectionContext,
    ) -> Result<PersistedSessionProjector, PersistedProjectionError> {
        if context.native_source_ref != self.native_source_ref {
            return Err(PersistedProjectionError::InvalidContext);
        }
        PersistedSessionProjector::with_seed(context, self.projection_seed.clone())
    }
}

#[derive(Debug, Error)]
pub enum PersistedReaderError {
    #[error("persisted provider source I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("persisted provider source is outside its provider-owned root")]
    SourceEscape,
    #[error("persisted provider source must be a regular non-symlink file")]
    InvalidSourceType,
    #[error("persisted provider source changed during the bounded read")]
    SourceChanged,
    #[error("persisted provider source is not UTF-8")]
    InvalidUtf8,
    #[error("persisted provider reader source contract is incomplete")]
    InvalidSourceContract,
    #[error("persisted provider source family or format version is unsupported")]
    UnsupportedSourceContract,
}

pub fn read_persisted_file_page(
    source: &PersistedReaderSource,
    boundary: &PersistedFileBoundary,
    before: Option<PersistedOrderingKey>,
    limit: usize,
) -> Result<PersistedRowPage, PersistedReaderError> {
    validate_source(source)?;
    let allowed_root = boundary.allowed_root.canonicalize()?;
    let link_metadata = fs::symlink_metadata(&boundary.transcript_path)?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(PersistedReaderError::InvalidSourceType);
    }
    let path = boundary.transcript_path.canonicalize()?;
    if !path.starts_with(&allowed_root) {
        return Err(PersistedReaderError::SourceEscape);
    }
    let file = fs::File::open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(PersistedReaderError::InvalidSourceType);
    }
    let snapshot_len = metadata.len();
    let source_generation = file_source_generation(source, &path, &metadata)?;
    scan_jsonl(
        source,
        &source_generation,
        BufReader::new(file.take(snapshot_len)),
        snapshot_len,
        PageWindow::Before(before),
        limit,
    )
}

/// Reads the first bounded group of complete rows strictly after a snapshot
/// watermark. This is the incremental half of the snapshot-first protocol;
/// the same physical reader and source generation are used for both halves.
pub fn read_persisted_file_page_after(
    source: &PersistedReaderSource,
    boundary: &PersistedFileBoundary,
    after: PersistedOrderingKey,
    limit: usize,
) -> Result<PersistedRowPage, PersistedReaderError> {
    validate_source(source)?;
    let allowed_root = boundary.allowed_root.canonicalize()?;
    let link_metadata = fs::symlink_metadata(&boundary.transcript_path)?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(PersistedReaderError::InvalidSourceType);
    }
    let path = boundary.transcript_path.canonicalize()?;
    if !path.starts_with(&allowed_root) {
        return Err(PersistedReaderError::SourceEscape);
    }
    let file = fs::File::open(&path)?;
    let metadata = file.metadata()?;
    let snapshot_len = metadata.len();
    let source_generation = file_source_generation(source, &path, &metadata)?;
    scan_jsonl(
        source,
        &source_generation,
        BufReader::new(file.take(snapshot_len)),
        snapshot_len,
        PageWindow::After(after),
        limit,
    )
}

/// DeepSeek's official bounded reader returns JSONL rather than a path. Its
/// source generation is the exact provider Session plus reviewed reader format;
/// content changes do not rename existing rows.
pub fn read_persisted_jsonl_snapshot(
    source: &PersistedReaderSource,
    content: &str,
    before: Option<PersistedOrderingKey>,
    limit: usize,
) -> Result<PersistedRowPage, PersistedReaderError> {
    validate_source(source)?;
    let source_generation = snapshot_source_generation(source);
    scan_jsonl(
        source,
        &source_generation,
        BufReader::new(std::io::Cursor::new(content.as_bytes())),
        content.len() as u64,
        PageWindow::Before(before),
        limit,
    )
}

pub fn read_persisted_jsonl_snapshot_after(
    source: &PersistedReaderSource,
    content: &str,
    after: PersistedOrderingKey,
    limit: usize,
) -> Result<PersistedRowPage, PersistedReaderError> {
    validate_source(source)?;
    let source_generation = snapshot_source_generation(source);
    scan_jsonl(
        source,
        &source_generation,
        BufReader::new(std::io::Cursor::new(content.as_bytes())),
        content.len() as u64,
        PageWindow::After(after),
        limit,
    )
}

#[derive(Clone, Copy)]
enum PageWindow {
    Before(Option<PersistedOrderingKey>),
    After(PersistedOrderingKey),
}

fn scan_jsonl(
    source: &PersistedReaderSource,
    source_generation: &str,
    mut reader: impl BufRead,
    snapshot_len: u64,
    window: PageWindow,
    limit: usize,
) -> Result<PersistedRowPage, PersistedReaderError> {
    let cursor = match window {
        PageWindow::Before(cursor) => cursor,
        PageWindow::After(cursor) => Some(cursor),
    };
    if cursor.is_some_and(|cursor| cursor.kind != OrderingKeyKind::CompleteRowEndOffset) {
        return Err(PersistedReaderError::InvalidSourceContract);
    }
    let before_value = match window {
        PageWindow::Before(cursor) => cursor.map(|cursor| cursor.value).unwrap_or(u64::MAX),
        PageWindow::After(_) => u64::MAX,
    };
    let after_value = match window {
        PageWindow::After(cursor) => Some(cursor.value),
        PageWindow::Before(_) => None,
    };
    let limit = limit.max(1);
    let mut row_bytes = Vec::new();
    let mut tail = VecDeque::new();
    let mut byte_offset = 0u64;
    let mut incomplete_tail = false;
    let mut watermark = None;
    let mut projection_seed = PersistedProjectionSeed::default();
    loop {
        row_bytes.clear();
        let read = reader.read_until(b'\n', &mut row_bytes)?;
        if read == 0 {
            break;
        }
        let row_start = byte_offset;
        byte_offset = byte_offset.saturating_add(read as u64);
        if !row_bytes.ends_with(b"\n") {
            incomplete_tail = true;
            break;
        }
        row_bytes.pop();
        if row_bytes.last() == Some(&b'\r') {
            row_bytes.pop();
        }
        if row_bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let raw_text =
            std::str::from_utf8(&row_bytes).map_err(|_| PersistedReaderError::InvalidUtf8)?;
        let content_fingerprint = sha256_label(&row_bytes);
        let native_event = serde_json::from_str(raw_text)
            .unwrap_or_else(|_| serde_json::Value::String(raw_text.to_owned()));
        let ordering_key = PersistedOrderingKey {
            kind: OrderingKeyKind::CompleteRowEndOffset,
            value: byte_offset,
        };
        watermark = Some(ordering_key);
        let row_locator = provider_native_row_id(source.provider, &native_event)
            .map(|identity| {
                format!(
                    "row-locator:provider-id:{}",
                    sha256_hex(identity.as_bytes())
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "row-locator:offset-{row_start}-{}",
                    content_fingerprint.trim_start_matches("sha256:")
                )
            });
        let selected = after_value
            .map(|after| ordering_key.value > after)
            .unwrap_or(ordering_key.value < before_value);
        if selected {
            tail.push_back(PersistedNativeRow {
                provider: source.provider,
                source_generation: source_generation.to_owned(),
                row_locator,
                ordering_key,
                content_fingerprint,
                occurred_at: occurred_at(&native_event),
                native_event,
            });
            match window {
                PageWindow::Before(_) => {
                    while tail.len() > limit.saturating_add(1) {
                        if let Some(row) = tail.pop_front() {
                            projection_seed.observe(&row);
                        }
                    }
                }
                PageWindow::After(_) => {}
            }
        } else if after_value.is_some_and(|after| ordering_key.value <= after) {
            let seed_row = PersistedNativeRow {
                provider: source.provider,
                source_generation: source_generation.to_owned(),
                row_locator,
                ordering_key,
                content_fingerprint,
                occurred_at: occurred_at(&native_event),
                native_event,
            };
            projection_seed.observe(&seed_row);
        }
        if matches!(window, PageWindow::After(_)) && tail.len() > limit.saturating_add(1) {
            // Keep scanning only to compute the exact snapshot watermark and
            // incomplete-tail flag; the response remains bounded.
            while tail.len() > limit.saturating_add(1) {
                tail.pop_back();
            }
        }
    }
    if byte_offset != snapshot_len && !incomplete_tail {
        return Err(PersistedReaderError::SourceChanged);
    }
    let has_more = tail.len() > limit;
    if has_more && matches!(window, PageWindow::Before(_)) {
        if let Some(row) = tail.pop_front() {
            projection_seed.observe(&row);
        }
    } else if has_more {
        tail.pop_back();
    }
    let rows = tail.into_iter().collect::<Vec<_>>();
    let next_before =
        (has_more && matches!(window, PageWindow::Before(_))).then(|| rows[0].ordering_key);
    Ok(PersistedRowPage {
        native_source_ref: native_source_ref(source),
        source_generation: source_generation.to_owned(),
        rows,
        snapshot_watermark: watermark,
        has_more,
        next_before,
        incomplete_tail,
        projection_seed,
    })
}

fn validate_source(source: &PersistedReaderSource) -> Result<(), PersistedReaderError> {
    if source.native_session_id.trim().is_empty()
        || source.source_family.trim().is_empty()
        || source.format_version_fence.trim().is_empty()
    {
        return Err(PersistedReaderError::InvalidSourceContract);
    }
    let manifest = super::manifest::persisted_adapter_manifest(source.provider);
    if !manifest
        .persisted_source_families
        .contains(&source.source_family)
        || !manifest
            .format_version_fences
            .contains(&source.format_version_fence)
    {
        return Err(PersistedReaderError::UnsupportedSourceContract);
    }
    Ok(())
}

fn native_source_ref(source: &PersistedReaderSource) -> String {
    let identity = format!(
        "{:?}\0{}\0{}",
        source.provider, source.native_session_id, source.source_family
    );
    format!("provider-source:sha256:{}", sha256_hex(identity.as_bytes()))
}

fn snapshot_source_generation(source: &PersistedReaderSource) -> String {
    generation_hash(source, "official-reader-snapshot")
}

fn file_source_generation(
    source: &PersistedReaderSource,
    canonical_path: &Path,
    metadata: &fs::Metadata,
) -> Result<String, PersistedReaderError> {
    let created = metadata
        .created()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    #[cfg(unix)]
    let physical_identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{created}", metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let physical_identity = format!("{}:{created}", canonical_path.display());
    Ok(generation_hash(
        source,
        &format!("{}:{physical_identity}", canonical_path.display()),
    ))
}

fn generation_hash(source: &PersistedReaderSource, incarnation: &str) -> String {
    let identity = format!(
        "{:?}\0{}\0{}\0{}\0{incarnation}",
        source.provider,
        source.native_session_id,
        source.source_family,
        source.format_version_fence
    );
    format!(
        "source-generation:sha256:{}",
        sha256_hex(identity.as_bytes())
    )
}

fn provider_native_row_id(provider: ProviderKind, value: &serde_json::Value) -> Option<String> {
    let row_type = value.get("type").and_then(serde_json::Value::as_str)?;
    let (discriminator, id) = match provider {
        ProviderKind::Codex => (
            value
                .pointer("/payload/type")
                .and_then(serde_json::Value::as_str),
            value
                .pointer("/payload/id")
                .or_else(|| value.pointer("/payload/call_id"))
                .or_else(|| value.pointer("/payload/turn_id")),
        ),
        ProviderKind::Claude => (
            value.get("subtype").and_then(serde_json::Value::as_str),
            value
                .get("uuid")
                .or_else(|| value.pointer("/message/id"))
                .or_else(|| value.get("id")),
        ),
        ProviderKind::Kimi => (
            value
                .pointer("/event/type")
                .and_then(serde_json::Value::as_str),
            value
                .pointer("/event/uuid")
                .or_else(|| value.pointer("/event/id")),
        ),
        ProviderKind::Pi => (
            value
                .pointer("/message/role")
                .and_then(serde_json::Value::as_str),
            value.get("id").or_else(|| value.pointer("/message/id")),
        ),
        ProviderKind::DeepseekHarness => (None, value.get("seq").or_else(|| value.get("id"))),
    };
    let id = id?;
    let id = id
        .as_str()
        .map(str::to_owned)
        .or_else(|| id.as_u64().map(|value| value.to_string()))?;
    Some(format!(
        "{:?}\0{row_type}\0{}\0{id}",
        provider,
        discriminator.unwrap_or_default()
    ))
}

fn occurred_at(value: &serde_json::Value) -> Option<String> {
    value
        .get("timestamp")
        .or_else(|| value.get("created_at"))
        .or_else(|| value.get("time"))
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_u64().map(|value| value.to_string()))
        })
}

fn sha256_label(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
