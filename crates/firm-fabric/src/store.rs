use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::protocol::*;
use crate::{
    bytes_to_hex, canonical_digest, sha256_hex, FabricError, FabricErrorCode, FABRIC_SCHEMA_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricBackupManifest {
    pub schema_version: String,
    pub transaction_sequence: u64,
    pub state_digest: String,
    pub journal_digest: String,
    pub journal_bytes: u64,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FabricCheckpointCore {
    schema_version: String,
    journal_offset: u64,
    journal_prefix_digest: String,
    transaction_sequence: u64,
    last_frame_digest: String,
    state: FabricState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FabricCheckpoint {
    #[serde(flatten)]
    core: FabricCheckpointCore,
    checkpoint_digest: String,
}

struct StoreInner {
    state: FabricState,
    last_frame_digest: String,
    journal_hasher: Sha256,
    journal_stamp: JournalStamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JournalStamp {
    length: u64,
    modified: Option<std::time::SystemTime>,
}

pub struct FabricStore {
    root: PathBuf,
    journal: PathBuf,
    checkpoint: PathBuf,
    lock_path: PathBuf,
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
        let checkpoint = canonical_root.join("fabric-checkpoint.json");
        let lock_path = canonical_root.join("fabric.lock");
        let lock = open_lock(&lock_path)?;
        lock.lock_exclusive().map_err(store_error)?;
        let (state, last_frame_digest, valid_length, checkpoint_current) =
            load_journal_from_checkpoint(&journal, &checkpoint)?;
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
        if !checkpoint_current {
            let journal_prefix_digest = journal_prefix_digest(&journal, valid_length)?;
            let _ = write_checkpoint(
                &checkpoint,
                &state,
                &last_frame_digest,
                valid_length,
                &journal_prefix_digest,
            );
        }
        let journal_hasher = journal_hasher(&journal, valid_length)?;
        let journal_stamp = journal_stamp(&journal)?;
        Ok(Self {
            root: canonical_root,
            journal,
            checkpoint,
            lock_path,
            inner: Mutex::new(StoreInner {
                state,
                last_frame_digest,
                journal_hasher,
                journal_stamp,
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
        let lock = open_lock(&self.lock_path)?;
        lock.lock_exclusive().map_err(store_error)?;
        let durable_stamp = journal_stamp(&self.journal)?;
        let mut inner = self.inner.lock().map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore lock poisoned",
            )
        })?;
        if durable_stamp == inner.journal_stamp {
            return Ok(inner.state.clone());
        }
        let (state, last_frame_digest, valid_length, checkpoint_current) =
            load_journal_from_checkpoint(&self.journal, &self.checkpoint)?;
        if !checkpoint_current {
            let journal_prefix_digest = journal_prefix_digest(&self.journal, valid_length)?;
            let _ = write_checkpoint(
                &self.checkpoint,
                &state,
                &last_frame_digest,
                valid_length,
                &journal_prefix_digest,
            );
        }
        inner.state = state;
        inner.last_frame_digest = last_frame_digest;
        inner.journal_hasher = journal_hasher(&self.journal, valid_length)?;
        inner.journal_stamp = journal_stamp(&self.journal)?;
        Ok(inner.state.clone())
    }

    /// Export one validated, transaction-boundary Control Plane backup. The
    /// Store lock prevents a concurrent append from splitting the snapshot.
    pub fn create_backup(
        &self,
        backup_root: impl AsRef<Path>,
    ) -> Result<FabricBackupManifest, FabricError> {
        self.require_available()?;
        let backup_root = backup_root.as_ref();
        if backup_root.exists() {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Fabric backup destination must not already exist",
            ));
        }
        let lock = open_lock(&self.lock_path)?;
        lock.lock_exclusive().map_err(store_error)?;
        let (state, _, valid_length) = load_journal(&self.journal)?;
        let mut journal_bytes = Vec::new();
        if valid_length > 0 {
            let mut journal = File::open(&self.journal).map_err(store_error)?;
            journal
                .read_to_end(&mut journal_bytes)
                .map_err(store_error)?;
            journal_bytes.truncate(valid_length as usize);
        }
        fs::create_dir(backup_root).map_err(store_error)?;
        if fs::symlink_metadata(backup_root)
            .map_err(store_error)?
            .file_type()
            .is_symlink()
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Fabric backup destination may not be a symlink",
            ));
        }
        let manifest = FabricBackupManifest {
            schema_version: FABRIC_SCHEMA_VERSION.into(),
            transaction_sequence: state.revision,
            state_digest: canonical_digest(&state)?,
            journal_digest: sha256_hex(&journal_bytes),
            journal_bytes: journal_bytes.len() as u64,
        };
        write_new_synced(
            &backup_root.join("fabric-transactions.jsonl"),
            &journal_bytes,
        )?;
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("failed to encode Fabric backup manifest: {error}"),
            )
        })?;
        write_new_synced(&backup_root.join("backup-manifest.json"), &manifest_bytes)?;
        File::open(backup_root)
            .and_then(|directory| directory.sync_all())
            .map_err(store_error)?;
        Ok(manifest)
    }

    /// Restore a validated backup only into a new empty Store root. Existing
    /// authority is never overwritten by this API.
    pub fn restore_backup(
        backup_root: impl AsRef<Path>,
        target_root: impl AsRef<Path>,
    ) -> Result<FabricBackupManifest, FabricError> {
        let backup_root = backup_root.as_ref();
        if !backup_root.is_dir()
            || fs::symlink_metadata(backup_root)
                .map_err(store_error)?
                .file_type()
                .is_symlink()
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Fabric backup source must be a real directory",
            ));
        }
        let manifest: FabricBackupManifest = serde_json::from_slice(
            &fs::read(backup_root.join("backup-manifest.json")).map_err(store_error)?,
        )
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("Fabric backup manifest is invalid: {error}"),
            )
        })?;
        if manifest.schema_version != FABRIC_SCHEMA_VERSION {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Fabric backup schema version is unsupported",
            ));
        }
        let journal_bytes =
            fs::read(backup_root.join("fabric-transactions.jsonl")).map_err(store_error)?;
        if journal_bytes.len() as u64 != manifest.journal_bytes
            || sha256_hex(&journal_bytes) != manifest.journal_digest
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Fabric backup journal digest or length changed",
            ));
        }
        let validation_root = backup_root;
        let (state, _, valid_length) =
            load_journal(&validation_root.join("fabric-transactions.jsonl"))?;
        if valid_length != journal_bytes.len() as u64
            || state.revision != manifest.transaction_sequence
            || canonical_digest(&state)? != manifest.state_digest
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Fabric backup does not end at its declared durable transaction boundary",
            ));
        }
        let target_root = target_root.as_ref();
        if target_root.exists() {
            if !target_root.is_dir()
                || fs::symlink_metadata(target_root)
                    .map_err(store_error)?
                    .file_type()
                    .is_symlink()
                || fs::read_dir(target_root)
                    .map_err(store_error)?
                    .next()
                    .is_some()
            {
                return Err(FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "Fabric restore target must be a new empty real directory",
                ));
            }
        } else {
            fs::create_dir_all(target_root).map_err(store_error)?;
        }
        write_new_synced(
            &target_root.join("fabric-transactions.jsonl"),
            &journal_bytes,
        )?;
        File::open(target_root)
            .and_then(|directory| directory.sync_all())
            .map_err(store_error)?;
        Ok(manifest)
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
        let lock = open_lock(&self.lock_path)?;
        lock.lock_exclusive().map_err(store_error)?;
        let mut inner = self.inner.lock().map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore lock poisoned",
            )
        })?;
        let durable_stamp = journal_stamp(&self.journal)?;
        if durable_stamp != inner.journal_stamp {
            let (durable_state, durable_digest, valid_length, _) =
                load_journal_from_checkpoint(&self.journal, &self.checkpoint)?;
            inner.state = durable_state;
            inner.last_frame_digest = durable_digest;
            inner.journal_hasher = journal_hasher(&self.journal, valid_length)?;
            inner.journal_stamp = durable_stamp;
        }
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
        let appended = append_frame(&self.journal, &frame)?;
        if self.fail_after_append.swap(false, Ordering::SeqCst) {
            self.available.store(false, Ordering::SeqCst);
            return Err(FabricError::unknown(
                "fabric-store-commit",
                "durable append may have committed but its acknowledgement was lost; reopen and reconcile",
            ));
        }
        inner.state = next;
        inner.last_frame_digest = frame_digest;
        inner.journal_hasher.update(&appended);
        let journal_length = self.journal.metadata().map_err(store_error)?.len();
        let journal_prefix_digest = bytes_to_hex(&inner.journal_hasher.clone().finalize());
        let _ = write_checkpoint(
            &self.checkpoint,
            &inner.state,
            &inner.last_frame_digest,
            journal_length,
            &journal_prefix_digest,
        );
        inner.journal_stamp = journal_stamp(&self.journal)?;
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

fn open_lock(path: &Path) -> Result<File, FabricError> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(store_error)
}

fn journal_stamp(path: &Path) -> Result<JournalStamp, FabricError> {
    match path.metadata() {
        Ok(metadata) => Ok(JournalStamp {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(JournalStamp {
            length: 0,
            modified: None,
        }),
        Err(error) => Err(store_error(error)),
    }
}

fn append_frame(path: &Path, frame: &JournalFrame) -> Result<Vec<u8>, FabricError> {
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
    Ok(encoded)
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), FabricError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(store_error)?;
    file.write_all(bytes).map_err(store_error)?;
    file.sync_all().map_err(store_error)
}

fn load_journal_from_checkpoint(
    journal: &Path,
    checkpoint: &Path,
) -> Result<(FabricState, String, u64, bool), FabricError> {
    let bytes = read_journal(journal)?;
    if let Some(checkpoint) = read_checkpoint(checkpoint)? {
        let offset = checkpoint.core.journal_offset as usize;
        let checkpoint_valid = checkpoint.core.schema_version == FABRIC_SCHEMA_VERSION
            && checkpoint.checkpoint_digest == canonical_digest(&checkpoint.core)?
            && offset <= bytes.len()
            && (offset == 0 || bytes.get(offset.saturating_sub(1)) == Some(&b'\n'))
            && sha256_hex(&bytes[..offset]) == checkpoint.core.journal_prefix_digest
            && checkpoint.core.state.revision == checkpoint.core.transaction_sequence
            && ((checkpoint.core.transaction_sequence == 0
                && checkpoint.core.last_frame_digest.is_empty())
                || (checkpoint.core.transaction_sequence > 0
                    && !checkpoint.core.last_frame_digest.is_empty()));
        if checkpoint_valid {
            let checkpoint_offset = checkpoint.core.journal_offset;
            let (state, digest, valid_length) = parse_journal(
                &bytes,
                offset,
                checkpoint.core.state,
                checkpoint.core.last_frame_digest,
                checkpoint.core.transaction_sequence.saturating_add(1),
            )?;
            return Ok((
                state,
                digest,
                valid_length,
                valid_length == checkpoint_offset,
            ));
        }
    }
    let (state, digest, valid_length) =
        parse_journal(&bytes, 0, FabricState::default(), String::new(), 1)?;
    Ok((state, digest, valid_length, false))
}

fn read_checkpoint(path: &Path) -> Result<Option<FabricCheckpoint>, FabricError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore checkpoint must be a regular non-symlink file",
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(store_error(error)),
    }
    let bytes = fs::read(path).map_err(store_error)?;
    Ok(serde_json::from_slice(&bytes).ok())
}

fn write_checkpoint(
    checkpoint: &Path,
    state: &FabricState,
    last_frame_digest: &str,
    journal_offset: u64,
    journal_prefix_digest: &str,
) -> Result<(), FabricError> {
    if let Ok(metadata) = fs::symlink_metadata(checkpoint) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FabricStore checkpoint must be a regular non-symlink file",
            ));
        }
    }
    let core = FabricCheckpointCore {
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        journal_offset,
        journal_prefix_digest: journal_prefix_digest.into(),
        transaction_sequence: state.revision,
        last_frame_digest: last_frame_digest.into(),
        state: state.clone(),
    };
    let record = FabricCheckpoint {
        checkpoint_digest: canonical_digest(&core)?,
        core,
    };
    let encoded = serde_json::to_vec(&record).map_err(|error| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            format!("failed to encode FabricStore checkpoint: {error}"),
        )
    })?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "system clock is before UNIX epoch",
            )
        })?
        .as_nanos();
    let temp = checkpoint.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    write_new_synced(&temp, &encoded)?;
    fs::rename(&temp, checkpoint).map_err(store_error)?;
    if let Some(parent) = checkpoint.parent() {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(store_error)?;
    }
    Ok(())
}

fn journal_hasher(path: &Path, valid_length: u64) -> Result<Sha256, FabricError> {
    let bytes = read_journal(path)?;
    let length = usize::try_from(valid_length).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "FabricStore journal length exceeds this platform",
        )
    })?;
    if length > bytes.len() || (length > 0 && bytes[length - 1] != b'\n') {
        return Err(FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "FabricStore journal does not end at the validated frame boundary",
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..length]);
    Ok(hasher)
}

fn journal_prefix_digest(path: &Path, valid_length: u64) -> Result<String, FabricError> {
    Ok(bytes_to_hex(
        &journal_hasher(path, valid_length)?.finalize(),
    ))
}

fn read_journal(path: &Path) -> Result<Vec<u8>, FabricError> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes).map_err(store_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(store_error(error)),
    }
    Ok(bytes)
}

fn load_journal(path: &Path) -> Result<(FabricState, String, u64), FabricError> {
    let bytes = read_journal(path)?;
    parse_journal(&bytes, 0, FabricState::default(), String::new(), 1)
}

fn parse_journal(
    bytes: &[u8],
    mut offset: usize,
    mut state: FabricState,
    mut expected_previous: String,
    mut expected_sequence: u64,
) -> Result<(FabricState, String, u64), FabricError> {
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
