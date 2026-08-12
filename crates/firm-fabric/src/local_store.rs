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

/// Durable proof that one ordered operation was consumed without crossing
/// the target application boundary. A tombstone advances only its exact
/// ordering key under the current Gateway session fence and never creates an
/// inbox, native result, or applied-effect claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOrderingTombstone {
    pub operation_id: String,
    pub ordering_key: String,
    pub ordering_seq: u64,
    pub route_seq: u64,
    pub request_digest: String,
    pub reason: FabricErrorCode,
    pub gateway_generation: u64,
    pub control_plane_generation: u64,
    pub consumed_at_unix_ms: u64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLocalFabricState {
    pub revision: u64,
    pub authority_company_id: Option<String>,
    pub authority_node_id: Option<String>,
    #[serde(default)]
    pub active_session: Option<FabricSessionFence>,
    pub outboxes: BTreeMap<String, LocalRemoteOutbox>,
    pub inboxes: BTreeMap<String, LocalRemoteInbox>,
    pub persisted_ordering_sequences: BTreeMap<String, u64>,
    #[serde(default)]
    pub ordering_tombstones: BTreeMap<String, LocalOrderingTombstone>,
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
    pub fn bind_gateway_session(&self, session: &FabricSessionFence) -> Result<(), FabricError> {
        if session.company_id != self.company_id || session.node_id != self.node_id {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "gateway session belongs to another Company or Node",
            ));
        }
        self.transact(|state| {
            if state.active_session.as_ref() == Some(session) {
                return Ok(());
            }
            if state.active_session.as_ref().is_some_and(|current| {
                session.control_plane_generation < current.control_plane_generation
                    || session.node_daemon_generation < current.node_daemon_generation
                    || (session.control_plane_generation == current.control_plane_generation
                        && session.gateway_generation < current.gateway_generation)
                    || (session.gateway_generation == current.gateway_generation
                        && (session.node_daemon_id != current.node_daemon_id
                            || session.node_daemon_generation
                                != current.node_daemon_generation))
            }) {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    "Node-local gateway session cannot move authority backwards or alias a generation",
                ));
            }
            state.active_session = Some(session.clone());
            Ok(())
        })
    }

    pub fn active_session(&self) -> Result<Option<FabricSessionFence>, FabricError> {
        Ok(self.snapshot()?.active_session)
    }

    pub fn pending_outbox_operations(&self) -> Result<Vec<RoutedOperation>, FabricError> {
        let state = self.snapshot()?;
        let mut operations = state
            .outboxes
            .values()
            .filter(|outbox| {
                matches!(
                    outbox.local_state,
                    LocalOutboxState::QueuedForControlPlane
                        | LocalOutboxState::Submitted
                        | LocalOutboxState::Accepted
                        | LocalOutboxState::ReconcileRequired
                )
            })
            .filter_map(|outbox| outbox.operation.clone())
            .collect::<Vec<_>>();
        operations.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(operations)
    }

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
        self.transact_session(session, |state| {
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
                operation: Some(operation.clone()),
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
        self.transact_session(session, |state| {
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

    /// Rebind a durable source operation only after the current Control Plane
    /// returned an empty, generation-fenced reconciliation result for its id.
    /// Once FabricStore has accepted the operation, that Store remains the
    /// sole route truth and the original fingerprint must never be rewritten.
    pub fn rebind_unaccepted_outbox(
        &self,
        session: &FabricSessionFence,
        operation_id: &str,
        reconciled_receipts: &[RouteReceipt],
    ) -> Result<RoutedOperation, FabricError> {
        self.require_session(session)?;
        if reconciled_receipts
            .iter()
            .any(|receipt| receipt.operation_id == operation_id)
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "accepted FabricStore operation cannot be rebound to a successor generation",
            ));
        }
        self.transact_session(session, |state| {
            let mut outbox = state.outboxes.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "successor rebind requires a durable source outbox",
                )
            })?;
            if !matches!(
                outbox.local_state,
                LocalOutboxState::QueuedForControlPlane
                    | LocalOutboxState::Submitted
                    | LocalOutboxState::ReconcileRequired
            ) {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "only an unaccepted source outbox can be rebound",
                ));
            }
            let mut operation = outbox.operation.clone().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "source outbox lost its pre-acceptance operation envelope",
                )
            })?;
            operation.source_gateway_generation = Some(session.gateway_generation);
            operation.source_node_daemon_id = Some(session.node_daemon_id.clone());
            operation.source_node_daemon_generation = Some(session.node_daemon_generation);
            operation.control_plane_generation = session.control_plane_generation;
            operation.validate_digest()?;
            operation.closed_body()?;
            outbox.request_digest = canonical_digest(&operation)?;
            outbox.gateway_generation = session.gateway_generation;
            outbox.control_plane_generation = session.control_plane_generation;
            outbox.local_state = LocalOutboxState::QueuedForControlPlane;
            outbox.operation = Some(operation.clone());
            state.outboxes.insert(operation_id.into(), outbox);
            Ok(operation)
        })
    }

    /// Settle a source operation which expired before FabricStore accepted it.
    ///
    /// FabricStore remains the sole cross-node route truth, so this method does
    /// not invent a Control Plane receipt. It only closes the Node-local
    /// pre-acceptance outbox under the exact active session fence. This keeps a
    /// durable offline operation from poisoning every later gateway reconnect
    /// while preserving the truthful `not_applied` boundary: no route attempt
    /// or target/native effect ever existed.
    pub fn expire_unaccepted_outbox(
        &self,
        session: &FabricSessionFence,
        operation_id: &str,
        now_unix_ms: u64,
    ) -> Result<Option<LocalRemoteOutbox>, FabricError> {
        self.require_session(session)?;
        let current = self.snapshot()?;
        if current.active_session.as_ref() != Some(session) {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "Node-local expiry requires the exact current active gateway session",
            ));
        }
        if let Some(existing) = current.outboxes.get(operation_id) {
            if existing.local_state == LocalOutboxState::Terminal {
                return Ok(Some(existing.clone()));
            }
            if existing.local_state == LocalOutboxState::Accepted {
                return Ok(None);
            }
        }
        self.transact_session(session, |state| {
            let mut outbox = state.outboxes.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "source expiry requires a durable local outbox",
                )
            })?;
            // A concurrent same-session settlement may have happened after
            // the read-only fast path. Returning it is an idempotent replay;
            // `transact_session` still guarantees no successor can interleave.
            if outbox.local_state == LocalOutboxState::Terminal {
                return Ok(Some(outbox));
            }
            if outbox.local_state == LocalOutboxState::Accepted {
                return Ok(None);
            }
            if !matches!(
                outbox.local_state,
                LocalOutboxState::QueuedForControlPlane
                    | LocalOutboxState::Submitted
                    | LocalOutboxState::ReconcileRequired
            ) {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "a Control Plane-accepted outbox cannot be settled as locally expired",
                ));
            }
            let operation = outbox.operation.as_ref().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "source outbox lost its pre-acceptance operation envelope",
                )
            })?;
            if operation.expires_at_unix_ms > now_unix_ms {
                return Err(FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "unaccepted source outbox has not expired",
                ));
            }
            outbox.local_state = LocalOutboxState::Terminal;
            outbox.terminal_receipt_ref = Some(format!(
                "local:not_applied:operation_expired:{}",
                operation.id
            ));
            state.outboxes.insert(operation_id.into(), outbox.clone());
            Ok(Some(outbox))
        })
    }

    pub fn mark_outbox_receipt(
        &self,
        session: &FabricSessionFence,
        receipt: &RouteReceipt,
    ) -> Result<LocalRemoteOutbox, FabricError> {
        self.require_session(session)?;
        if receipt.company_id != self.company_id {
            return Err(FabricError::none(
                FabricErrorCode::WrongCompany,
                "route receipt belongs to another Company",
            ));
        }
        self.transact_session(session, |state| {
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
                ReceiptKind::RecoveryRequired => {
                    next.local_state = LocalOutboxState::ReconcileRequired;
                    next.terminal_receipt_ref = None;
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
        now_unix_ms: u64,
    ) -> Result<(LocalRemoteInbox, bool), FabricError> {
        self.require_session(session)?;
        operation.validate_digest()?;
        operation.closed_body()?;
        if operation.expires_at_unix_ms <= now_unix_ms {
            return Err(FabricError::none(
                FabricErrorCode::OperationExpired,
                "expired routed operation cannot be persisted by the target",
            ));
        }
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
        self.transact_session(session, |state| {
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

    /// Consume one expired ordered delivery without persisting an inbox or
    /// invoking application code. This prevents a terminal NotApplied #1 from
    /// permanently blocking valid #2 while retaining explicit durable truth
    /// that the skipped sequence never reached the native boundary.
    pub fn consume_expired_ordering_tombstone(
        &self,
        session: &FabricSessionFence,
        operation: &RoutedOperation,
        attempt: &RouteAttempt,
        now_unix_ms: u64,
    ) -> Result<(LocalOrderingTombstone, bool), FabricError> {
        self.require_session(session)?;
        operation.validate_digest()?;
        operation.closed_body()?;
        if operation.expires_at_unix_ms > now_unix_ms {
            return Err(FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "ordering tombstone requires an expired routed operation",
            ));
        }
        if operation.company_id != self.company_id
            || operation.target_node_id != self.node_id
            || attempt.company_id != self.company_id
            || attempt.target_node_id != self.node_id
            || attempt.operation_id != operation.id
            || !matches!(
                attempt.state,
                RouteAttemptState::Queued | RouteAttemptState::Sent
            )
            || operation.control_plane_generation != session.control_plane_generation
            || attempt.target_gateway_generation != session.gateway_generation
            || attempt.control_plane_generation != session.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "expired ordering tombstone does not match the authenticated target generation",
            ));
        }
        let request_digest = canonical_digest(operation)?;
        let ordering_key = operation.ordering_key.clone();
        self.transact_session(session, |state| {
            if state.inboxes.contains_key(&operation.id)
                || state.results.contains_key(&operation.id)
            {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "persisted or applied operation cannot become an ordering tombstone",
                ));
            }
            if let Some(existing) = state.ordering_tombstones.get(&operation.id) {
                if existing.request_digest != request_digest
                    || existing.ordering_key != ordering_key
                    || existing.ordering_seq != attempt.ordering_seq
                    || existing.route_seq != attempt.route_seq
                    || existing.gateway_generation != session.gateway_generation
                    || existing.control_plane_generation != session.control_plane_generation
                {
                    return Err(FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "expired ordering tombstone replay changed its fingerprint",
                    ));
                }
                return Ok((existing.clone(), true));
            }
            let prior = state
                .persisted_ordering_sequences
                .get(&ordering_key)
                .copied()
                .unwrap_or(0);
            if attempt.ordering_seq != prior.saturating_add(1) {
                return Err(FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "expired ordering tombstone is out of order for its ordering key",
                ));
            }
            let tombstone = LocalOrderingTombstone {
                operation_id: operation.id.clone(),
                ordering_key: ordering_key.clone(),
                ordering_seq: attempt.ordering_seq,
                route_seq: attempt.route_seq,
                request_digest,
                reason: FabricErrorCode::OperationExpired,
                gateway_generation: session.gateway_generation,
                control_plane_generation: session.control_plane_generation,
                consumed_at_unix_ms: now_unix_ms,
                schema_version: FABRIC_SCHEMA_VERSION.into(),
            };
            state
                .persisted_ordering_sequences
                .insert(ordering_key, attempt.ordering_seq);
            state
                .ordering_tombstones
                .insert(operation.id.clone(), tombstone.clone());
            Ok((tombstone, false))
        })
    }

    pub fn claim_inbox(
        &self,
        session: &FabricSessionFence,
        operation: &RoutedOperation,
        now_unix_ms: u64,
    ) -> Result<LocalRemoteInbox, FabricError> {
        self.require_session(session)?;
        operation.validate_digest()?;
        operation.closed_body()?;
        if operation.expires_at_unix_ms <= now_unix_ms {
            return Err(FabricError::none(
                FabricErrorCode::OperationExpired,
                "expired routed operation cannot cross the target application boundary",
            ));
        }
        let operation_id = operation.id.as_str();
        self.transact_session(session, |state| {
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
        self.transact_session(session, |state| {
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

    /// Run one Node-local mutation only while the exact authenticated gateway
    /// session is still the durable active session. The comparison occurs
    /// after the filesystem lock is held and the latest journal state has
    /// been reloaded, so a predecessor cannot pass an early check and mutate
    /// after a successor has bound.
    fn transact_session<T>(
        &self,
        session: &FabricSessionFence,
        operation: impl FnOnce(&mut NodeLocalFabricState) -> Result<T, FabricError>,
    ) -> Result<T, FabricError> {
        self.require_session(session)?;
        self.transact(|state| {
            if state.active_session.as_ref() != Some(session) {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    "Node-local mutation requires the exact current active gateway session",
                ));
            }
            operation(state)
        })
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
