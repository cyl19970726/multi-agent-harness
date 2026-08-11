use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::protocol::*;
use crate::transport::FabricSessionFence;
use crate::{canonical_digest, FabricError, FabricErrorCode, FABRIC_SCHEMA_VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalApplicationResult {
    pub operation_id: String,
    pub result_schema: String,
    pub result: serde_json::Value,
    pub result_digest: String,
    pub effect: EffectCertainty,
    pub gateway_generation: u64,
    pub completed_at_unix_ms: u64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLocalFabricState {
    pub revision: u64,
    pub authority_company_id: Option<String>,
    pub authority_node_id: Option<String>,
    pub outboxes: BTreeMap<String, LocalRemoteOutbox>,
    pub inboxes: BTreeMap<String, LocalRemoteInbox>,
    pub persisted_ordering_sequences: BTreeMap<String, u64>,
    pub results: BTreeMap<String, LocalApplicationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalJournalCore {
    transaction_sequence: u64,
    previous_digest: String,
    state: NodeLocalFabricState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalJournalFrame {
    transaction_sequence: u64,
    previous_digest: String,
    state: NodeLocalFabricState,
    frame_digest: String,
}

struct LocalInner {
    state: NodeLocalFabricState,
    last_frame_digest: String,
}

pub struct NodeLocalFabricStore {
    company_id: String,
    node_id: String,
    root: PathBuf,
    journal: PathBuf,
    lock_path: PathBuf,
    inner: Mutex<LocalInner>,
    available: AtomicBool,
    fail_next_commit: AtomicBool,
    fail_after_append: AtomicBool,
}

impl NodeLocalFabricStore {
    pub fn open(
        root: impl AsRef<Path>,
        company_id: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Result<Self, FabricError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(local_store_error)?;
        if fs::symlink_metadata(&root)
            .map_err(local_store_error)?
            .file_type()
            .is_symlink()
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Node local Fabric root may not be a symlink",
            ));
        }
        let root = fs::canonicalize(root).map_err(local_store_error)?;
        let journal = root.join("node-fabric-transactions.jsonl");
        let lock_path = root.join("node-fabric.lock");
        let lock = open_local_lock(&lock_path)?;
        lock.lock_exclusive().map_err(local_store_error)?;
        let (state, last_frame_digest, valid_length) = load_local_journal(&journal)?;
        let requested_company_id = company_id.into();
        let requested_node_id = node_id.into();
        if state
            .authority_company_id
            .as_deref()
            .is_some_and(|authority| authority != requested_company_id)
            || state
                .authority_node_id
                .as_deref()
                .is_some_and(|authority| authority != requested_node_id)
        {
            return Err(FabricError::none(
                FabricErrorCode::WrongCompany,
                "Node-local FabricStore is durably bound to another Company or Node",
            ));
        }
        if journal
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            > valid_length
        {
            let file = OpenOptions::new()
                .write(true)
                .open(&journal)
                .map_err(local_store_error)?;
            file.set_len(valid_length).map_err(local_store_error)?;
            file.sync_all().map_err(local_store_error)?;
        }
        Ok(Self {
            company_id: requested_company_id,
            node_id: requested_node_id,
            root,
            journal,
            lock_path,
            inner: Mutex::new(LocalInner {
                state,
                last_frame_digest,
            }),
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

    pub fn snapshot(&self) -> Result<NodeLocalFabricState, FabricError> {
        self.require_available()?;
        let lock = open_local_lock(&self.lock_path)?;
        lock.lock_exclusive().map_err(local_store_error)?;
        let (state, last_frame_digest, _) = load_local_journal(&self.journal)?;
        let mut inner = self.inner.lock().map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Node local FabricStore lock poisoned",
            )
        })?;
        inner.state = state;
        inner.last_frame_digest = last_frame_digest;
        Ok(inner.state.clone())
    }

    pub fn fail_next_commit_for_test(&self) {
        self.fail_next_commit.store(true, Ordering::SeqCst);
    }

    pub fn fail_after_append_for_test(&self) {
        self.fail_after_append.store(true, Ordering::SeqCst);
    }

    pub fn prepare_outbox(
        &self,
        session: &FabricSessionFence,
        authenticated_actor: &AuthenticatedActor,
        operation: &RoutedOperation,
        _now_unix_ms: u64,
    ) -> Result<(LocalRemoteOutbox, bool), FabricError> {
        self.require_session(session)?;
        if operation.source_authority != OperationSourceAuthority::Node
            || operation.company_id != self.company_id
            || operation.source_node_id.as_deref() != Some(self.node_id.as_str())
        {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "source outbox operation does not match this Node authority",
            ));
        }
        if authenticated_actor != &operation.actor
            || authenticated_actor.company_id != self.company_id
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "source outbox actor must be resolved by the authenticated Node session",
            ));
        }
        if operation.source_gateway_generation != Some(session.gateway_generation)
            || operation.source_node_daemon_id.as_deref() != Some(session.node_daemon_id.as_str())
            || operation.source_node_daemon_generation != Some(session.node_daemon_generation)
            || operation.control_plane_generation != session.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "source outbox operation does not match the authenticated Fabric generation",
            ));
        }
        operation.validate_digest()?;
        operation.closed_body()?;
        let request_digest = canonical_digest(operation)?;
        self.transact(|state| {
            if let Some(existing) = state.outboxes.get(&operation.id) {
                if existing.request_digest != request_digest
                    || Some(existing.gateway_generation) != operation.source_gateway_generation
                    || existing.control_plane_generation != operation.control_plane_generation
                {
                    return Err(FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "source outbox replay changed its operation fingerprint",
                    ));
                }
                return Ok((existing.clone(), true));
            }
            let outbox = LocalRemoteOutbox {
                company_id: self.company_id.clone(),
                node_id: self.node_id.clone(),
                operation_id: operation.id.clone(),
                request_digest,
                local_state: LocalOutboxState::QueuedForControlPlane,
                gateway_generation: operation
                    .source_gateway_generation
                    .expect("validated Node source has a gateway generation"),
                control_plane_generation: operation.control_plane_generation,
                attempt_count: 0,
                last_attempt_at_unix_ms: None,
                terminal_receipt_ref: None,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            state.outboxes.insert(operation.id.clone(), outbox.clone());
            Ok((outbox, false))
        })
    }

    /// Record that a live authenticated gateway is about to submit the exact
    /// durable operation. Merely preparing an outbox while the Control Plane
    /// is offline remains visibly `queued_for_control_plane`.
    pub fn mark_outbox_submitted(
        &self,
        session: &FabricSessionFence,
        authenticated_actor: &AuthenticatedActor,
        operation: &RoutedOperation,
        now_unix_ms: u64,
    ) -> Result<LocalRemoteOutbox, FabricError> {
        self.require_session(session)?;
        if authenticated_actor != &operation.actor
            || authenticated_actor.company_id != self.company_id
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "outbox submit actor does not match the authenticated source identity",
            ));
        }
        if operation.company_id != self.company_id
            || operation.source_authority != OperationSourceAuthority::Node
            || operation.source_node_id.as_deref() != Some(self.node_id.as_str())
            || operation.source_gateway_generation != Some(session.gateway_generation)
            || operation.source_node_daemon_id.as_deref() != Some(session.node_daemon_id.as_str())
            || operation.source_node_daemon_generation != Some(session.node_daemon_generation)
            || operation.control_plane_generation != session.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "outbox submission does not match this authenticated source generation",
            ));
        }
        let request_digest = canonical_digest(operation)?;
        self.transact(|state| {
            let mut outbox = state.outboxes.get(&operation.id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "operation must be durably queued before submission",
                )
            })?;
            if outbox.request_digest != request_digest
                || outbox.gateway_generation != session.gateway_generation
                || outbox.control_plane_generation != session.control_plane_generation
            {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "outbox submission changed its durable request fingerprint",
                ));
            }
            if outbox.local_state == LocalOutboxState::Terminal {
                return Ok(outbox);
            }
            outbox.local_state = LocalOutboxState::Submitted;
            outbox.attempt_count = outbox.attempt_count.saturating_add(1);
            outbox.last_attempt_at_unix_ms = Some(now_unix_ms);
            state.outboxes.insert(operation.id.clone(), outbox.clone());
            Ok(outbox)
        })
    }

    pub fn mark_outbox_receipt(
        &self,
        receipt: &RouteReceipt,
    ) -> Result<LocalRemoteOutbox, FabricError> {
        if receipt.company_id != self.company_id {
            return Err(FabricError::none(
                FabricErrorCode::WrongCompany,
                "route receipt belongs to another Company",
            ));
        }
        self.transact(|state| {
            let outbox = state
                .outboxes
                .get(&receipt.operation_id)
                .cloned()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::OperationUnknown,
                        "route receipt has no source outbox",
                    )
                })?;
            let mut next = outbox;
            match receipt.kind {
                ReceiptKind::ControlPlaneAccepted | ReceiptKind::TargetPersisted => {
                    if next.local_state != LocalOutboxState::Terminal {
                        next.local_state = LocalOutboxState::Accepted;
                    }
                }
                ReceiptKind::OperationApplied | ReceiptKind::OperationRejected => {
                    next.local_state = LocalOutboxState::Terminal;
                    next.terminal_receipt_ref = Some(receipt.id.clone());
                }
            }
            state
                .outboxes
                .insert(receipt.operation_id.clone(), next.clone());
            Ok(next)
        })
    }

    pub fn persist_inbox(
        &self,
        session: &FabricSessionFence,
        operation: &RoutedOperation,
        attempt: &RouteAttempt,
    ) -> Result<(LocalRemoteInbox, bool), FabricError> {
        self.require_session(session)?;
        operation.validate_digest()?;
        operation.closed_body()?;
        if operation.company_id != self.company_id
            || operation.target_node_id != self.node_id
            || attempt.company_id != self.company_id
            || attempt.target_node_id != self.node_id
            || attempt.operation_id != operation.id
            || !matches!(
                attempt.state,
                RouteAttemptState::Queued | RouteAttemptState::Sent
            )
        {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "target inbox operation or attempt does not match this Node authority",
            ));
        }
        if operation.control_plane_generation != session.control_plane_generation
            || attempt.target_gateway_generation != session.gateway_generation
            || attempt.control_plane_generation != session.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "target inbox operation or attempt does not match the authenticated Fabric generation",
            ));
        }
        let request_digest = canonical_digest(operation)?;
        let ordering_index = operation.ordering_key.clone();
        self.transact(|state| {
            if let Some(existing) = state.inboxes.get(&operation.id) {
                if existing.request_digest != request_digest
                    || existing.route_seq != attempt.route_seq
                    || existing.gateway_generation != attempt.target_gateway_generation
                    || existing.control_plane_generation != attempt.control_plane_generation
                {
                    return Err(FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "target inbox replay changed its operation fingerprint",
                    ));
                }
                return Ok((existing.clone(), true));
            }
            let prior = state
                .persisted_ordering_sequences
                .get(&ordering_index)
                .copied()
                .unwrap_or(0);
            if attempt.ordering_seq != prior.saturating_add(1) {
                return Err(FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "target inbox received an out-of-order operation for its ordering key",
                ));
            }
            let inbox = LocalRemoteInbox {
                company_id: self.company_id.clone(),
                node_id: self.node_id.clone(),
                operation_id: operation.id.clone(),
                route_seq: attempt.route_seq,
                request_digest,
                state: LocalInboxState::Persisted,
                gateway_generation: attempt.target_gateway_generation,
                control_plane_generation: attempt.control_plane_generation,
                attempt_count: attempt.attempt_no,
                claim_generation: None,
                result_digest: None,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            state
                .persisted_ordering_sequences
                .insert(ordering_index, attempt.ordering_seq);
            state.inboxes.insert(operation.id.clone(), inbox.clone());
            Ok((inbox, false))
        })
    }

    pub fn claim_inbox(
        &self,
        session: &FabricSessionFence,
        operation_id: &str,
    ) -> Result<LocalRemoteInbox, FabricError> {
        self.require_session(session)?;
        self.transact(|state| {
            let inbox = state.inboxes.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "operation was not durably persisted in this Node inbox",
                )
            })?;
            if inbox.gateway_generation != session.gateway_generation
                || inbox.control_plane_generation != session.control_plane_generation
            {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    "inbox claim does not own the persisted Fabric generation",
                ));
            }
            match inbox.state {
                LocalInboxState::Persisted => {
                    let mut claimed = inbox;
                    claimed.state = LocalInboxState::Claimed;
                    claimed.claim_generation = Some(session.gateway_generation);
                    state
                        .inboxes
                        .insert(operation_id.into(), claimed.clone());
                    Ok(claimed)
                }
                LocalInboxState::Claimed | LocalInboxState::RecoveryRequired => {
                    Err(FabricError::unknown(
                        operation_id,
                        "operation was already claimed; native effect must be reconciled before retry",
                    ))
                }
                LocalInboxState::Applied | LocalInboxState::Rejected => Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "terminal inbox cannot be claimed again",
                )),
            }
        })
    }

    pub fn unresolved_operation_ids(&self) -> Result<BTreeSet<String>, FabricError> {
        Ok(self
            .snapshot()?
            .inboxes
            .values()
            .filter(|inbox| {
                matches!(
                    inbox.state,
                    LocalInboxState::Claimed | LocalInboxState::RecoveryRequired
                )
            })
            .map(|inbox| inbox.operation_id.clone())
            .collect())
    }

    pub fn record_application_result(
        &self,
        session: &FabricSessionFence,
        operation_id: &str,
        result_schema: &str,
        result: serde_json::Value,
        effect: EffectCertainty,
        now_unix_ms: u64,
    ) -> Result<(LocalRemoteInbox, LocalApplicationResult, bool), FabricError> {
        self.require_session(session)?;
        let result_digest = canonical_digest(&result)?;
        self.transact(|state| {
            if let Some(existing) = state.results.get(operation_id) {
                if existing.result_digest != result_digest
                    || existing.result_schema != result_schema
                    || existing.effect != effect
                    || existing.gateway_generation != session.gateway_generation
                {
                    return Err(FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "application result replay changed its fingerprint",
                    ));
                }
                let inbox = state.inboxes.get(operation_id).cloned().ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::StoreUnavailable,
                        "local application result has no inbox",
                    )
                })?;
                return Ok((inbox, existing.clone(), true));
            }
            let inbox = state.inboxes.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "operation was not durably persisted in this Node inbox",
                )
            })?;
            if inbox.gateway_generation != session.gateway_generation
                || inbox.control_plane_generation != session.control_plane_generation
                || inbox.state != LocalInboxState::Claimed
            {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    "application result requires the exact durably claimed inbox generation",
                ));
            }
            let mut next_inbox = inbox;
            next_inbox.state = match effect {
                EffectCertainty::Applied => LocalInboxState::Applied,
                EffectCertainty::NotApplied => LocalInboxState::Rejected,
                EffectCertainty::Unknown => LocalInboxState::RecoveryRequired,
                EffectCertainty::None => {
                    return Err(FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "a terminal application result must prove applied, not_applied, or unknown",
                    ));
                }
            };
            next_inbox.result_digest = Some(result_digest.clone());
            let local_result = LocalApplicationResult {
                operation_id: operation_id.into(),
                result_schema: result_schema.into(),
                result,
                result_digest,
                effect,
                gateway_generation: session.gateway_generation,
                completed_at_unix_ms: now_unix_ms,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            state
                .inboxes
                .insert(operation_id.into(), next_inbox.clone());
            state
                .results
                .insert(operation_id.into(), local_result.clone());
            Ok((next_inbox, local_result, false))
        })
    }

    fn transact<T>(
        &self,
        operation: impl FnOnce(&mut NodeLocalFabricState) -> Result<T, FabricError>,
    ) -> Result<T, FabricError> {
        self.require_available()?;
        let lock = open_local_lock(&self.lock_path)?;
        lock.lock_exclusive().map_err(local_store_error)?;
        let mut inner = self.inner.lock().map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Node local FabricStore lock poisoned",
            )
        })?;
        let (durable_state, durable_digest, _) = load_local_journal(&self.journal)?;
        inner.state = durable_state;
        inner.last_frame_digest = durable_digest;
        let mut next = inner.state.clone();
        match (
            next.authority_company_id.as_deref(),
            next.authority_node_id.as_deref(),
        ) {
            (None, None) => {
                next.authority_company_id = Some(self.company_id.clone());
                next.authority_node_id = Some(self.node_id.clone());
            }
            (Some(company_id), Some(node_id))
                if company_id == self.company_id && node_id == self.node_id => {}
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::WrongCompany,
                    "Node-local FabricStore authority binding is incomplete or mismatched",
                ));
            }
        }
        let result = operation(&mut next)?;
        next.revision = inner.state.revision.saturating_add(1);
        if self.fail_next_commit.swap(false, Ordering::SeqCst) {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "forced failure before Node-local durable transaction append",
            ));
        }
        let core = LocalJournalCore {
            transaction_sequence: next.revision,
            previous_digest: inner.last_frame_digest.clone(),
            state: next.clone(),
        };
        let frame_digest = canonical_digest(&core)?;
        let frame = LocalJournalFrame {
            transaction_sequence: core.transaction_sequence,
            previous_digest: core.previous_digest,
            state: core.state,
            frame_digest: frame_digest.clone(),
        };
        append_local_frame(&self.journal, &frame)?;
        if self.fail_after_append.swap(false, Ordering::SeqCst) {
            self.available.store(false, Ordering::SeqCst);
            return Err(FabricError::unknown(
                "node-local-fabric-commit",
                "Node-local durable append may have committed but its acknowledgement was lost; reopen and reconcile",
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
                "Node-local FabricStore is unavailable",
            ));
        }
        Ok(())
    }

    fn require_session(&self, session: &FabricSessionFence) -> Result<(), FabricError> {
        if session.company_id != self.company_id || session.node_id != self.node_id {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "authenticated Fabric session does not own this Node-local Store",
            ));
        }
        if session.gateway_generation == 0
            || session.node_daemon_id.trim().is_empty()
            || session.node_daemon_generation == 0
            || session.control_plane_generation == 0
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "authenticated Fabric session generations must be non-zero",
            ));
        }
        Ok(())
    }
}

fn open_local_lock(path: &Path) -> Result<File, FabricError> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(local_store_error)
}

fn append_local_frame(path: &Path, frame: &LocalJournalFrame) -> Result<(), FabricError> {
    let mut encoded = serde_json::to_vec(frame).map_err(|error| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            format!("failed to encode Node local transaction: {error}"),
        )
    })?;
    encoded.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(local_store_error)?;
    file.write_all(&encoded).map_err(local_store_error)?;
    file.sync_all().map_err(local_store_error)?;
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(local_store_error)?;
    }
    Ok(())
}

fn load_local_journal(path: &Path) -> Result<(NodeLocalFabricState, String, u64), FabricError> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_end(&mut bytes).map_err(local_store_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((NodeLocalFabricState::default(), String::new(), 0));
        }
        Err(error) => return Err(local_store_error(error)),
    }
    let mut state = NodeLocalFabricState::default();
    let mut expected_previous = String::new();
    let mut expected_sequence = 1u64;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some(relative_end) = bytes[offset..].iter().position(|byte| *byte == b'\n') else {
            break;
        };
        let end = offset + relative_end;
        let frame: LocalJournalFrame =
            serde_json::from_slice(&bytes[offset..end]).map_err(|error| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    format!("Node local committed frame is invalid: {error}"),
                )
            })?;
        let core = LocalJournalCore {
            transaction_sequence: frame.transaction_sequence,
            previous_digest: frame.previous_digest.clone(),
            state: frame.state.clone(),
        };
        if frame.transaction_sequence != expected_sequence
            || frame.previous_digest != expected_previous
            || frame.frame_digest != canonical_digest(&core)?
            || frame.state.revision != frame.transaction_sequence
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "Node local journal sequence or digest chain is corrupt",
            ));
        }
        state = frame.state;
        expected_previous = frame.frame_digest;
        expected_sequence = expected_sequence.saturating_add(1);
        offset = end + 1;
    }
    Ok((state, expected_previous, offset as u64))
}

fn local_store_error(error: std::io::Error) -> FabricError {
    FabricError::none(
        FabricErrorCode::StoreUnavailable,
        format!("Node local FabricStore I/O failed: {error}"),
    )
}
