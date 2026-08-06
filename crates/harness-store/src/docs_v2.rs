//! AI-first Docs slice store operations (ADR 0054).
//!
//! Adds three ledgers (`blocks_v2`, `document_revisions`,
//! `document_change_ops`) plus one atomic page-write boundary with
//! expected-revision optimistic concurrency and idempotent replay. Document
//! rows keep using the existing `company_os_documents.jsonl` ledger so the
//! current page metadata projections stay compatible.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{latest_by_id, HarnessStore, StoreError, StoreResult};
use harness_core::company_os::{Document, ValidateCompanyOs};
use harness_core::docs_v2::{BlockV2, ChangeMutation, DocumentChangeOperation, DocumentRevision};

const BLOCKS_V2: &str = "company_os_blocks_v2.jsonl";
const DOCUMENT_REVISIONS: &str = "company_os_document_revisions.jsonl";
const DOCUMENT_CHANGE_OPS: &str = "company_os_document_change_ops.jsonl";

/// Deterministic revision id: one identity per (document, revision number).
fn revision_id_for(document_id: &str, revision_number: u64) -> String {
    format!("document-revision-{document_id}-r{revision_number}")
}

fn sha256_hex(payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validation(message: impl std::fmt::Display) -> StoreError {
    StoreError::CompanyOsValidation(message.to_string())
}

/// Request for one atomic page write. `expected_revision = 0` creates the
/// first revision of the document.
#[derive(Debug, Clone)]
pub struct PageWriteRequest {
    /// The resulting latest Document row (metadata + resulting block order).
    pub document: Document,
    /// New or updated BlockV2 rows to append. Every row must appear in
    /// `document.block_ids`.
    pub block_rows: Vec<BlockV2>,
    /// Descriptive mutation log persisted in the change operation row.
    pub mutations: Vec<ChangeMutation>,
    pub expected_revision: u64,
    pub change_summary: String,
    pub authored_by: harness_core::company_os::ActorRef,
    pub execution_ref: Option<harness_core::company_os::EntityRef>,
    /// Canonical idempotency key (the governing ActionCommand id in the full
    /// Phase 2 dispatch path).
    pub action_command_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageWriteOutcome {
    pub revision_id: String,
    pub revision_number: u64,
    pub content_digest: String,
    /// True when the same `action_command_id` already committed this exact
    /// change and the original revision is returned instead of a new one.
    pub replayed: bool,
}

/// One page read bundle: latest document, ordered blocks, latest revision.
#[derive(Debug, Clone)]
pub struct DocumentPageState {
    pub document: Document,
    /// Blocks ordered exactly by `document.block_ids`.
    pub blocks: Vec<BlockV2>,
    /// Latest revision row, if any write has been committed.
    pub revision: Option<DocumentRevision>,
}

impl HarnessStore {
    /// Raw append-only BlockV2 ledger rows in append order.
    pub fn blocks_v2(&self) -> StoreResult<Vec<BlockV2>> {
        self.read_jsonl(BLOCKS_V2)
    }

    /// Latest-row-wins BlockV2 projection.
    pub fn latest_blocks_v2(&self) -> StoreResult<Vec<BlockV2>> {
        Ok(latest_by_id(self.blocks_v2()?, |row| row.id.clone())
            .into_values()
            .collect())
    }

    /// Raw append-only DocumentRevision rows in append order.
    pub fn document_revisions(&self) -> StoreResult<Vec<DocumentRevision>> {
        self.read_jsonl(DOCUMENT_REVISIONS)
    }

    /// Raw append-only DocumentChangeOperation rows in append order.
    pub fn document_change_ops(&self) -> StoreResult<Vec<DocumentChangeOperation>> {
        self.read_jsonl(DOCUMENT_CHANGE_OPS)
    }

    /// Revision history for one document in commit order.
    pub fn document_revision_history(
        &self,
        document_id: &str,
    ) -> StoreResult<Vec<DocumentRevision>> {
        Ok(self
            .document_revisions()?
            .into_iter()
            .filter(|revision| revision.document_id == document_id)
            .collect())
    }

    /// Latest committed revision number for a document (0 when none).
    fn latest_revision_row(&self, document_id: &str) -> StoreResult<Option<DocumentRevision>> {
        Ok(self
            .document_revision_history(document_id)?
            .into_iter()
            .max_by_key(|revision| revision.revision_number))
    }

    /// Read one document page: latest Document row, BlockV2 rows ordered by
    /// `Document.block_ids`, and the latest revision. Returns `Ok(None)` when
    /// the document itself does not exist.
    pub fn read_document_page(&self, document_id: &str) -> StoreResult<Option<DocumentPageState>> {
        let document = match latest_by_id(
            self.read_jsonl::<Document>("company_os_documents.jsonl")?,
            |row: &Document| row.id.clone(),
        )
        .remove(document_id)
        {
            Some(document) => document,
            None => return Ok(None),
        };
        let latest = latest_by_id(self.blocks_v2()?, |row| row.id.clone());
        let mut blocks = Vec::with_capacity(document.block_ids.len());
        for block_id in &document.block_ids {
            match latest.get(block_id) {
                Some(block) if block.document_id == document.id => blocks.push(block.clone()),
                Some(_) => {
                    return Err(validation(format!(
                        "document {} references block {block_id} owned by another document",
                        document.id
                    )))
                }
                None => {
                    return Err(validation(format!(
                        "document {} references missing block {block_id}",
                        document.id
                    )))
                }
            }
        }
        let revision = self.latest_revision_row(document_id)?;
        Ok(Some(DocumentPageState {
            document,
            blocks,
            revision,
        }))
    }

    /// Canonical snapshot for a page write: the resulting document plus its
    /// ordered blocks. Serialization is deterministic because serde_json map
    /// keys are ordered.
    fn page_snapshot(document: &Document, ordered_blocks: &[BlockV2]) -> StoreResult<Value> {
        Ok(json!({
            "document": serde_json::to_value(document).map_err(StoreError::Json)?,
            "blocks": serde_json::to_value(ordered_blocks).map_err(StoreError::Json)?,
        }))
    }

    /// Resolve the ordered block set for a write: new/updated rows take
    /// precedence over existing latest rows; every id in `block_ids` must
    /// resolve, and every supplied row must be referenced.
    fn resolve_ordered_blocks(&self, request: &PageWriteRequest) -> StoreResult<Vec<BlockV2>> {
        let existing = latest_by_id(self.blocks_v2()?, |row| row.id.clone());
        let supplied: std::collections::BTreeMap<String, &BlockV2> = request
            .block_rows
            .iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        for row in &request.block_rows {
            if !request.document.block_ids.contains(&row.id) {
                return Err(validation(format!(
                    "supplied block {} is not referenced by Document.block_ids",
                    row.id
                )));
            }
        }
        let mut ordered = Vec::with_capacity(request.document.block_ids.len());
        for block_id in &request.document.block_ids {
            let block = match supplied.get(block_id) {
                Some(row) => (*row).clone(),
                None => match existing.get(block_id) {
                    Some(row) if row.document_id == request.document.id => row.clone(),
                    Some(_) => {
                        return Err(validation(format!(
                            "block {block_id} belongs to another document and cannot be adopted"
                        )))
                    }
                    None => {
                        return Err(validation(format!(
                            "Document.block_ids references missing block {block_id}"
                        )))
                    }
                },
            };
            ordered.push(block);
        }
        Ok(ordered)
    }

    fn replay_outcome(&self, committed: &DocumentChangeOperation) -> StoreResult<PageWriteOutcome> {
        let revision = self
            .document_revisions()?
            .into_iter()
            .find(|revision| revision.id == committed.document_revision_id)
            .ok_or_else(|| {
                validation(format!(
                    "committed change op references missing revision {}",
                    committed.document_revision_id
                ))
            })?;
        Ok(PageWriteOutcome {
            revision_id: revision.id,
            revision_number: revision.revision_number,
            content_digest: revision.content_digest,
            replayed: true,
        })
    }

    /// The single atomic page-write boundary. Under one store write lock:
    /// validate everything, enforce idempotency by `action_command_id`,
    /// enforce `expected_revision` optimistic concurrency, then append block
    /// rows, the Document row, the change operation row, and the revision row
    /// together. Returns `REVISION_CONFLICT` when the document moved past the
    /// caller's base revision, and `IDEMPOTENCY_CONFLICT` when the same
    /// command id was committed with a different payload.
    pub fn write_document_page_atomic(
        &self,
        request: &PageWriteRequest,
    ) -> StoreResult<PageWriteOutcome> {
        request.document.validate().map_err(validation)?;
        for row in &request.block_rows {
            row.validate().map_err(validation)?;
        }
        for mutation in &request.mutations {
            if let Some(block) = &mutation.block {
                block.validate().map_err(validation)?;
            }
        }
        request.authored_by.validate().map_err(validation)?;
        if request.action_command_id.trim().is_empty() {
            return Err(validation("PageWriteRequest.action_command_id is required"));
        }
        if request.created_at.trim().is_empty() {
            return Err(validation("PageWriteRequest.created_at is required"));
        }

        let ordered_blocks = self.resolve_ordered_blocks(request)?;

        self.init()?;
        let _lock = self.acquire_write_lock()?;

        // Idempotency: an already-committed command id replays its original
        // result; a divergent payload under the same id is a conflict.
        for committed in self.document_change_ops()? {
            if committed.action_command_id == request.action_command_id {
                let same_payload = committed.document_id == request.document.id
                    && committed.expected_revision == request.expected_revision
                    && committed.resulting_block_ids == request.document.block_ids;
                if same_payload {
                    return self.replay_outcome(&committed);
                }
                return Err(StoreError::Conflict(format!(
                    "IDEMPOTENCY_CONFLICT: action command {} already committed to document {}",
                    request.action_command_id, committed.document_id
                )));
            }
        }

        // Optimistic concurrency: the document must still be at the revision
        // the change was prepared against.
        let current = self.latest_revision_row(&request.document.id)?;
        let current_number = current
            .as_ref()
            .map(|revision| revision.revision_number)
            .unwrap_or(0);
        if current_number != request.expected_revision {
            return Err(StoreError::Conflict(format!(
                "REVISION_CONFLICT: document {} is at revision {current_number}, expected {}",
                request.document.id, request.expected_revision
            )));
        }

        let revision_number = request.expected_revision + 1;
        let revision_id = revision_id_for(&request.document.id, revision_number);
        let snapshot = Self::page_snapshot(&request.document, &ordered_blocks)?;
        let canonical = serde_json::to_string(&snapshot).map_err(StoreError::Json)?;
        let revision = DocumentRevision {
            id: revision_id.clone(),
            document_id: request.document.id.clone(),
            revision_number,
            parent_revision_id: current.map(|row| row.id),
            content_snapshot: snapshot,
            content_digest: sha256_hex(&canonical),
            change_summary: request.change_summary.clone(),
            authored_by: request.authored_by.clone(),
            execution_ref: request.execution_ref.clone(),
            action_command_id: request.action_command_id.clone(),
            created_at: request.created_at.clone(),
        };
        revision.validate().map_err(validation)?;

        let change_op = DocumentChangeOperation {
            action_command_id: request.action_command_id.clone(),
            document_id: request.document.id.clone(),
            expected_revision: request.expected_revision,
            mutations: request.mutations.clone(),
            resulting_document_id: request.document.id.clone(),
            resulting_block_ids: request.document.block_ids.clone(),
            document_revision_id: revision_id.clone(),
            created_at: request.created_at.clone(),
        };
        change_op.validate().map_err(validation)?;

        // All appends share the single write lock; replay plus the revision
        // digest make any torn prefix detectable and re-drivable.
        for row in &request.block_rows {
            self.append_jsonl_unlocked(BLOCKS_V2, row)?;
        }
        self.append_jsonl_unlocked("company_os_documents.jsonl", &request.document)?;
        self.append_jsonl_unlocked(DOCUMENT_CHANGE_OPS, &change_op)?;
        self.append_jsonl_unlocked(DOCUMENT_REVISIONS, &revision)?;

        Ok(PageWriteOutcome {
            revision_id,
            revision_number,
            content_digest: revision.content_digest,
            replayed: false,
        })
    }
}
