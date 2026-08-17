//! INACTIVE HISTORICAL (DOC-108 Stage B): the `firm-store` docs_v2 module
//! this file covered was deleted with the legacy CompanyOS cutover (built-in
//! Docs retirement). Kept source-only per the inactive-historical convention.
#![cfg(any())]

//! Acceptance tests for the AI-first Docs slice (ADR 0054): revision ledger,
//! expected-revision optimistic concurrency, idempotent replay, and atomic
//! multi-block page writes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use firm_core::company_os::{ActorRef, ActorType, Document, DocumentKind, LifecycleStatus};
use firm_core::docs_v2::{BlockKindV2, BlockV2, ChangeMutation, ChangeMutationOp};
use firm_store::docs_v2::PageWriteRequest;
use firm_store::{HarnessStore, StoreError};
use serde_json::json;

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);
const NOW: &str = "2026-08-05T23:59:00Z";

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "harness-docs-v2-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("initialize test store");
        Self { root, store }
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn agent() -> ActorRef {
    ActorRef {
        actor_type: ActorType::Agent,
        actor_id: "agent-docs-v2".into(),
    }
}

fn document(id: &str, block_ids: Vec<&str>) -> Document {
    Document {
        id: id.into(),
        space_id: "company".into(),
        parent_document_id: None,
        title: "AI-first Docs page".into(),
        kind: DocumentKind::Page,
        lifecycle_status: LifecycleStatus::Active,
        block_ids: block_ids.into_iter().map(str::to_string).collect(),
        template_ref: None,
        permission_policy_refs: vec![],
        reference_refs: vec![],
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn paragraph(id: &str, document_id: &str, text: &str) -> BlockV2 {
    BlockV2 {
        id: id.into(),
        document_id: document_id.into(),
        kind: BlockKindV2::Paragraph,
        content: json!({ "text": text }),
        referenced_entities: vec![],
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn heading(id: &str, document_id: &str, level: u64, text: &str) -> BlockV2 {
    BlockV2 {
        id: id.into(),
        document_id: document_id.into(),
        kind: BlockKindV2::Heading,
        content: json!({ "level": level, "text": text }),
        referenced_entities: vec![],
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn request(
    doc: Document,
    blocks: Vec<BlockV2>,
    expected_revision: u64,
    action_command_id: &str,
) -> PageWriteRequest {
    PageWriteRequest {
        block_rows: blocks,
        mutations: vec![ChangeMutation {
            op: ChangeMutationOp::BlockAppend,
            anchor_block_id: None,
            target_block_id: None,
            source_block_ids: vec![],
            block: None,
        }],
        document: doc,
        expected_revision,
        change_summary: "test write".into(),
        authored_by: agent(),
        execution_ref: None,
        action_command_id: action_command_id.into(),
        created_at: NOW.into(),
    }
}

#[test]
fn page_create_commits_revision_one_with_ordered_blocks() {
    let fixture = TestStore::new("create");
    let blocks = vec![
        heading("blk-1", "doc-1", 1, "Overview"),
        paragraph("blk-2", "doc-1", "Agent-first docs."),
    ];
    let outcome = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1", "blk-2"]),
            blocks,
            0,
            "act-create-1",
        ))
        .expect("page create succeeds");

    assert_eq!(outcome.revision_number, 1);
    assert!(!outcome.replayed);
    assert_eq!(outcome.revision_id, "document-revision-doc-1-r1");
    assert_eq!(outcome.content_digest.len(), 64);

    let page = fixture
        .store
        .read_document_page("doc-1")
        .expect("read page")
        .expect("page exists");
    assert_eq!(page.blocks.len(), 2);
    assert_eq!(
        page.blocks[0].id, "blk-1",
        "order follows Document.block_ids"
    );
    assert_eq!(page.blocks[1].id, "blk-2");
    let revision = page.revision.expect("revision recorded");
    assert_eq!(revision.revision_number, 1);
    assert_eq!(revision.content_digest, outcome.content_digest);
    assert!(revision.parent_revision_id.is_none());
}

#[test]
fn append_from_current_revision_advances_and_links_parent() {
    let fixture = TestStore::new("append");
    fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1"]),
            vec![paragraph("blk-1", "doc-1", "first")],
            0,
            "act-1",
        ))
        .expect("create");

    let outcome = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1", "blk-2"]),
            vec![paragraph("blk-2", "doc-1", "second")],
            1,
            "act-2",
        ))
        .expect("append succeeds");

    assert_eq!(outcome.revision_number, 2);
    let history = fixture
        .store
        .document_revision_history("doc-1")
        .expect("history readable");
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[1].parent_revision_id.as_deref(),
        Some("document-revision-doc-1-r1")
    );
    // Revision snapshots reconstruct each historical page state.
    let first_snapshot = &history[0].content_snapshot;
    assert_eq!(first_snapshot["blocks"].as_array().unwrap().len(), 1);
    let second_snapshot = &history[1].content_snapshot;
    assert_eq!(second_snapshot["blocks"].as_array().unwrap().len(), 2);
}

#[test]
fn stale_expected_revision_returns_revision_conflict() {
    let fixture = TestStore::new("conflict");
    fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1"]),
            vec![paragraph("blk-1", "doc-1", "first")],
            0,
            "act-1",
        ))
        .expect("create");

    let error = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1", "blk-2"]),
            vec![paragraph("blk-2", "doc-1", "late writer")],
            0,
            "act-late",
        ))
        .expect_err("stale writer is rejected");

    match error {
        StoreError::Conflict(message) => {
            assert!(
                message.starts_with("REVISION_CONFLICT: document doc-1 is at revision 1"),
                "unexpected conflict message: {message}"
            );
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
    // Nothing was appended by the rejected write.
    assert_eq!(
        fixture
            .store
            .document_revision_history("doc-1")
            .expect("history")
            .len(),
        1
    );
}

#[test]
fn identical_command_replays_original_revision() {
    let fixture = TestStore::new("replay");
    let make = || {
        request(
            document("doc-1", vec!["blk-1"]),
            vec![paragraph("blk-1", "doc-1", "first")],
            0,
            "act-replay",
        )
    };
    let first = fixture
        .store
        .write_document_page_atomic(&make())
        .expect("create");
    let second = fixture
        .store
        .write_document_page_atomic(&make())
        .expect("replay succeeds");

    assert!(second.replayed);
    assert_eq!(second.revision_id, first.revision_id);
    assert_eq!(second.content_digest, first.content_digest);
    assert_eq!(
        fixture
            .store
            .document_revision_history("doc-1")
            .expect("history")
            .len(),
        1,
        "replay must not append a second revision"
    );
}

#[test]
fn same_command_id_with_divergent_payload_conflicts() {
    let fixture = TestStore::new("idempotency-conflict");
    fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1"]),
            vec![paragraph("blk-1", "doc-1", "first")],
            0,
            "act-shared",
        ))
        .expect("create");

    let error = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-2", vec!["blk-9"]),
            vec![paragraph("blk-9", "doc-2", "other doc")],
            0,
            "act-shared",
        ))
        .expect_err("reused command id is rejected");

    match error {
        StoreError::Conflict(message) => {
            assert!(message.starts_with("IDEMPOTENCY_CONFLICT:"), "{message}");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn multi_block_write_is_visible_atomically() {
    let fixture = TestStore::new("atomic");
    let blocks = vec![
        heading("blk-1", "doc-1", 1, "Plan"),
        paragraph("blk-2", "doc-1", "Step one."),
        paragraph("blk-3", "doc-1", "Step two."),
    ];
    let outcome = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1", "blk-2", "blk-3"]),
            blocks,
            0,
            "act-multi",
        ))
        .expect("multi-block write succeeds");
    assert_eq!(outcome.revision_number, 1);

    let page = fixture
        .store
        .read_document_page("doc-1")
        .expect("read")
        .expect("page");
    let ids: Vec<&str> = page.blocks.iter().map(|block| block.id.as_str()).collect();
    assert_eq!(ids, vec!["blk-1", "blk-2", "blk-3"]);
    assert_eq!(
        fixture.store.blocks_v2().expect("raw ledger").len(),
        3,
        "all block rows land in one commit"
    );
}

#[test]
fn unreferenced_or_missing_blocks_are_rejected() {
    let fixture = TestStore::new("refs");

    let orphan = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1"]),
            vec![
                paragraph("blk-1", "doc-1", "kept"),
                paragraph("blk-orphan", "doc-1", "not referenced"),
            ],
            0,
            "act-orphan",
        ))
        .expect_err("unreferenced block row is rejected");
    assert!(
        matches!(orphan, StoreError::CompanyOsValidation(_)),
        "{orphan:?}"
    );

    let missing = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1", "blk-ghost"]),
            vec![paragraph("blk-1", "doc-1", "kept")],
            0,
            "act-missing",
        ))
        .expect_err("missing referenced block is rejected");
    assert!(
        matches!(missing, StoreError::CompanyOsValidation(_)),
        "{missing:?}"
    );

    assert!(
        fixture.store.blocks_v2().expect("ledger").is_empty(),
        "rejected writes append nothing"
    );
}

#[test]
fn blocks_owned_by_another_document_cannot_be_adopted() {
    let fixture = TestStore::new("cross-doc");
    fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-1", vec!["blk-1"]),
            vec![paragraph("blk-1", "doc-1", "mine")],
            0,
            "act-doc1",
        ))
        .expect("create doc-1");

    let error = fixture
        .store
        .write_document_page_atomic(&request(
            document("doc-2", vec!["blk-1"]),
            vec![],
            0,
            "act-doc2",
        ))
        .expect_err("cross-document adoption is rejected");
    assert!(
        matches!(error, StoreError::CompanyOsValidation(_)),
        "{error:?}"
    );
}
