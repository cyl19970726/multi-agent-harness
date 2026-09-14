//! Store reads this suite asserts on directly.
//!
//! Kept out of the suite body because they are about which journal a fact
//! lives in, not about the HTTP surface under test.
use harness_store::HarnessStore;

/// The raw legacy `work_operations.jsonl` rows. These assertions are about
/// that one file — a refused writer appends nothing to it, and since the W4
/// writer cutover nothing appends to it at all.
pub(super) fn legacy_ledger_rows(store: &HarnessStore) -> Vec<harness_core::WorkOperation> {
    store
        .legacy_work_operation_rows()
        .expect("legacy Work ledger rows")
}

/// Every Work revision the store holds, through the one reader.
pub(super) fn work_journal(store: &HarnessStore) -> Vec<harness_store::WorkJournalRecord> {
    store.work_journal_records().expect("Work journal records")
}
