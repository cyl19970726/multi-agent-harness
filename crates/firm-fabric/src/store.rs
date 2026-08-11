use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::protocol::*;
use crate::{canonical_digest, FabricError, FabricErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FabricStoreLimits {
    pub max_queued_operations_per_node: usize,
    pub max_queued_bytes_per_node: u64,
    pub max_artifact_bytes: u64,
    pub max_operation_artifact_bytes: u64,
    pub max_operations_per_minute_per_source_actor: u32,
}

impl Default for FabricStoreLimits {
    fn default() -> Self {
        Self {
            max_queued_operations_per_node: 10_000,
            max_queued_bytes_per_node: 1024 * 1024 * 1024,
            max_artifact_bytes: 64 * 1024 * 1024,
            max_operation_artifact_bytes: 256 * 1024 * 1024,
            max_operations_per_minute_per_source_actor: 600,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricState {
    pub revision: u64,
    pub authority_company_id: Option<String>,
    pub control_plane_leases: BTreeMap<String, CompanyControlPlaneLease>,
    pub enrollments: BTreeMap<String, NodeEnrollment>,
    pub nodes: BTreeMap<String, CompanyNode>,
    pub certificates: BTreeMap<String, NodeCertificate>,
    pub revoked_certificate_serials: BTreeSet<String>,
    pub gateway_leases: BTreeMap<String, NodeGatewayLease>,
    pub operations: BTreeMap<String, RoutedOperation>,
    pub operation_idempotency: BTreeMap<String, String>,
    pub attempts: BTreeMap<String, RouteAttempt>,
    pub receipts: BTreeMap<String, RouteReceipt>,
    pub route_sequences: BTreeMap<String, u64>,
    pub ordering_sequences: BTreeMap<String, u64>,
    pub rate_windows: BTreeMap<String, FabricRateWindow>,
    pub artifacts: BTreeMap<String, RemoteArtifactManifest>,
    pub encrypted_artifacts: BTreeMap<String, EncryptedArtifact>,
    pub consumed_capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedArtifact {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalFrameCore {
    transaction_sequence: u64,
    previous_digest: String,
    state: FabricState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalFrame {
    transaction_sequence: u64,
    previous_digest: String,
    state: FabricState,
    frame_digest: String,
}

struct StoreInner {
    state: FabricState,
    last_frame_digest: String,
}

pub struct FabricStore {
    root: PathBuf,
    journal: PathBuf,
    inner: Mutex<StoreInner>,
    limits: FabricStoreLimits,
    available: AtomicBool,
    fail_next_commit: AtomicBool,
    fail_after_append: AtomicBool,
}

impl FabricStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, FabricError> {
        Self::open_with_limits(root, FabricStoreLimits::default())
    }

    pub fn open_with_limits(
        root: impl AsRef<Path>,
        limits: FabricStoreLimits,
    ) -> Result<Self, FabricError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(store_error)?;
        if fs::symlink_metadata(&root)
            .map_err(store_error)?
            .file_type()
            .is_symlink()
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore root may not be a symlink",
            ));
        }
        let canonical_root = fs::canonicalize(&root).map_err(store_error)?;
        let journal = canonical_root.join("fabric-transactions.jsonl");
        let (state, last_frame_digest, valid_length) = load_journal(&journal)?;
        if journal
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            > valid_length
        {
            let file = OpenOptions::new()
                .write(true)
                .open(&journal)
                .map_err(store_error)?;
            file.set_len(valid_length).map_err(store_error)?;
            file.sync_all().map_err(store_error)?;
        }
        Ok(Self {
            root: canonical_root,
            journal,
            inner: Mutex::new(StoreInner {
                state,
                last_frame_digest,
            }),
            limits,
            available: AtomicBool::new(true),
            fail_next_commit: AtomicBool::new(false),
            fail_after_append: AtomicBool::new(false),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal
    }

    pub fn limits(&self) -> FabricStoreLimits {
        self.limits
    }

    pub fn snapshot(&self) -> Result<FabricState, FabricError> {
        self.require_available()?;
        Ok(self
            .inner
            .lock()
            .map_err(|_| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "FabricStore lock poisoned",
                )
            })?
            .state
            .clone())
    }

    pub fn set_available_for_test(&self, available: bool) {
        self.available.store(available, Ordering::SeqCst);
    }

    pub fn fail_next_commit_for_test(&self) {
        self.fail_next_commit.store(true, Ordering::SeqCst);
    }

    pub fn fail_after_append_for_test(&self) {
        self.fail_after_append.store(true, Ordering::SeqCst);
    }

    pub(crate) fn transact<T>(
        &self,
        operation: impl FnOnce(&mut FabricState) -> Result<T, FabricError>,
    ) -> Result<T, FabricError> {
        self.require_available()?;
        let mut inner = self.inner.lock().map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore lock poisoned",
            )
        })?;
        let mut next = inner.state.clone();
        let result = operation(&mut next)?;
        next.revision = inner.state.revision.saturating_add(1);
        if self.fail_next_commit.swap(false, Ordering::SeqCst) {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "forced failure before durable transaction append",
            ));
        }
        let core = JournalFrameCore {
            transaction_sequence: next.revision,
            previous_digest: inner.last_frame_digest.clone(),
            state: next.clone(),
        };
        let frame_digest = canonical_digest(&core)?;
        let frame = JournalFrame {
            transaction_sequence: core.transaction_sequence,
            previous_digest: core.previous_digest,
            state: core.state,
            frame_digest: frame_digest.clone(),
        };
        append_frame(&self.journal, &frame)?;
        if self.fail_after_append.swap(false, Ordering::SeqCst) {
            self.available.store(false, Ordering::SeqCst);
            return Err(FabricError::unknown(
                "fabric-store-commit",
                "durable append may have committed but its acknowledgement was lost; reopen and reconcile",
            ));
        }
        inner.state = next;
        inner.last_frame_digest = frame_digest;
        Ok(result)
    }

    fn require_available(&self) -> Result<(), FabricError> {
        if !self.available.load(Ordering::SeqCst) {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore is unavailable",
            ));
        }
        Ok(())
    }
}

fn append_frame(path: &Path, frame: &JournalFrame) -> Result<(), FabricError> {
    let mut encoded = serde_json::to_vec(frame).map_err(|error| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            format!("failed to encode FabricStore transaction: {error}"),
        )
    })?;
    encoded.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(store_error)?;
    file.write_all(&encoded).map_err(store_error)?;
    file.sync_all().map_err(store_error)?;
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(store_error)?;
    }
    Ok(())
}

fn load_journal(path: &Path) -> Result<(FabricState, String, u64), FabricError> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes).map_err(store_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((FabricState::default(), String::new(), 0));
        }
        Err(error) => return Err(store_error(error)),
    }
    let mut state = FabricState::default();
    let mut expected_previous = String::new();
    let mut expected_sequence = 1u64;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some(relative_end) = bytes[offset..].iter().position(|byte| *byte == b'\n') else {
            // A crash may leave one torn final append. It was never ACKed and
            // is deliberately ignored. Mid-journal corruption is not ignored.
            break;
        };
        let end = offset + relative_end;
        if end == offset {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore journal contains an empty committed frame",
            ));
        }
        let frame: JournalFrame = serde_json::from_slice(&bytes[offset..end]).map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("FabricStore committed frame is invalid: {error}"),
            )
        })?;
        let core = JournalFrameCore {
            transaction_sequence: frame.transaction_sequence,
            previous_digest: frame.previous_digest.clone(),
            state: frame.state.clone(),
        };
        let actual_digest = canonical_digest(&core)?;
        if frame.transaction_sequence != expected_sequence
            || frame.previous_digest != expected_previous
            || frame.frame_digest != actual_digest
            || frame.state.revision != frame.transaction_sequence
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore journal sequence or digest chain is corrupt",
            ));
        }
        state = frame.state;
        expected_previous = frame.frame_digest;
        expected_sequence = expected_sequence.saturating_add(1);
        offset = end + 1;
    }
    Ok((state, expected_previous, offset as u64))
}

fn store_error(error: std::io::Error) -> FabricError {
    FabricError::none(
        FabricErrorCode::StoreUnavailable,
        format!("FabricStore I/O failed: {error}"),
    )
}
