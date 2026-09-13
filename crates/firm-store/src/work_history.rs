//! The one reader for Work.
//!
//! A Work's version chain is one chain, but until the W4 writer cutover its
//! rows live in two journals:
//!
//! * `work_operations.jsonl` (plus the crash-atomic
//!   `work_delegation_operations.jsonl` composite) holds [`WorkOperation`]
//!   rows — Created, Assigned, Claimed, Started, Released, Blocked, Resumed,
//!   ChangesRequested, Updated, Rebound, ExecutionRetargeted,
//!   ExecutionRecovered.
//! * `agentfirm_trust_operations.jsonl` holds the canonical trust envelopes
//!   whose `work` aggregate carries Submitted, Accepted, Cancelled and
//!   DependenciesChanged, and whose `work_report/created` envelope carries a
//!   pre-cutover Review snapshot as an immutable side record.
//!
//! Every current-phase, event, count and cursor reader goes through this
//! module so no consumer can see half the chain. The module offers exactly
//! four shapes:
//!
//! 1. [`HarnessStore::latest_works`] / [`HarnessStore::current_work`] — the
//!    merged latest Work per id (ledger + delegation fold, overlaid by the
//!    trust fold on greater version; the ledger wins an exact tie).
//! 2. [`HarnessStore::work_history`] — one Work's versions from both
//!    journals, strictly in version order, as event + snapshot records.
//! 3. [`HarnessStore::work_journal_records`] / [`HarnessStore::work_events`] —
//!    every Work record in the store in one deterministic total order.
//! 4. [`HarnessStore::work_journal_position`] and
//!    [`HarnessStore::work_journal_cursors_for_team_run`] — a monotonic
//!    journal position that advances on BOTH a ledger row and a trust Work
//!    transition.
//!
//! **Total order.** The two files share no comparable clock: ledger rows are
//! stamped with an ISO `created_at` and trust events with `unix-ms:`, and both
//! resolutions tie under scripted writes. The store-wide order is therefore
//! source-major and honest about it — every ledger row in its durable append
//! order, then every trust Work transition in `store_sequence` order. Only
//! *within one Work* is a real order available, and that one is the version
//! chain, which [`HarnessStore::work_history`] uses.
//!
//! **What is still ledger-only.** Ledger writers keep private ledger reads for
//! their own append-time concerns (idempotency lookup, record/event id
//! availability, next ledger sequence, provenance recovery, and the
//! ledger-shaped submission provenance proof). Those are writer internals of
//! one file, not readers of Work state; W4 retires them with the file.
use super::*;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// Which journal a Work record was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkJournalSource {
    /// `work_operations.jsonl` (or its crash-atomic delegation composite).
    Ledger,
    /// A `work`-aggregate trust envelope, or the pre-cutover Review snapshot
    /// carried by a `work_report/created` envelope.
    Trust,
}

/// One Work revision as both journals can express it: the event that produced
/// it and the Work projection it produced.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkJournalRecord {
    pub source: WorkJournalSource,
    pub event: WorkEvent,
    pub work: Work,
}

/// A monotonic position in the Work journal.
///
/// The two components count the two files independently because neither can
/// be interleaved into the other while both are still written: a new ledger
/// row must not sort before an already-issued trust position, and vice versa.
/// A position therefore advances a Work in the product order — `ledger`
/// strictly greater, or `trust` strictly greater — never by comparing one
/// component against the other.
///
/// [`WorkJournalPosition::packed`] renders it as one `u64` for transports
/// that carry a single integer (the `--since` cursor). The packing is
/// `trust * 2^32 + ledger`, so:
///
/// * every integer cursor written by the pre-W3 code — a bare ledger row
///   count — decodes unchanged to `{ledger: n, trust: 0}`, which is exactly
///   what it meant, and
/// * every position that has seen a trust transition is at least `2^32`, i.e.
///   it orders after every legacy value.
///
/// The packed form is only a transport. Comparison always decodes first and
/// uses [`WorkJournalPosition::advanced_past`]; comparing packed integers
/// directly would be trust-major lexicographic and would skip a Work whose
/// only new row is a ledger row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkJournalPosition {
    pub ledger: u64,
    pub trust: u64,
}

impl WorkJournalPosition {
    /// Ledger headroom in the packed form. Chosen so that a packed position
    /// stays exactly representable as an IEEE-754 double (the JSON number a
    /// Host loop reads back) until a single TeamRun has recorded more than
    /// two million trust Work transitions.
    pub const PACKED_LEDGER_RADIX: u64 = 1 << 32;

    pub fn packed(&self) -> u64 {
        self.trust
            .saturating_mul(Self::PACKED_LEDGER_RADIX)
            .saturating_add(self.ledger.min(Self::PACKED_LEDGER_RADIX - 1))
    }

    pub fn from_packed(value: u64) -> Self {
        Self {
            ledger: value % Self::PACKED_LEDGER_RADIX,
            trust: value / Self::PACKED_LEDGER_RADIX,
        }
    }

    /// True when `self` names at least one journal row that `cursor` does not.
    pub fn advanced_past(&self, cursor: &Self) -> bool {
        self.ledger > cursor.ledger || self.trust > cursor.trust
    }

    /// Componentwise maximum: the watermark of two observations.
    pub fn merged_max(self, other: Self) -> Self {
        Self {
            ledger: self.ledger.max(other.ledger),
            trust: self.trust.max(other.trust),
        }
    }

    /// How many journal rows this position has seen, for the single-number
    /// "how far has the log advanced" reporters.
    pub fn total(&self) -> u64 {
        self.ledger.saturating_add(self.trust)
    }
}

/// Per-Work delta cursors for one TeamRun, with the watermark a caller should
/// carry into its next read.
#[derive(Debug, Clone, Default)]
pub struct WorkJournalRunCursors {
    pub by_work: BTreeMap<String, WorkJournalPosition>,
    pub watermark: WorkJournalPosition,
}

impl WorkJournalRunCursors {
    /// True when this Work changed after `cursor`. An unknown Work is not a
    /// change: it has no row in this run at all.
    pub fn changed_after(&self, work_id: &str, cursor: &WorkJournalPosition) -> bool {
        self.by_work
            .get(work_id)
            .is_some_and(|position| position.advanced_past(cursor))
    }
}

pub(super) struct WorkJournal {
    pub records: Vec<WorkJournalRecord>,
    pub position: WorkJournalPosition,
}

impl HarnessStore {
    /// The merged current Work for one id, or `None` when no journal names it.
    pub fn current_work(&self, work_id: &str) -> StoreResult<Option<Work>> {
        Ok(self.latest_works_unlocked()?.remove(work_id))
    }

    /// Latest Work per id across both journals. The ledger + delegation fold
    /// is overlaid by the trust fold wherever trust holds a greater version;
    /// an exact version tie keeps the ledger projection.
    pub(super) fn latest_works_unlocked(&self) -> StoreResult<BTreeMap<String, Work>> {
        let mut latest = self
            .current_work_sources()?
            .latest
            .clone()
            .map_err(StoreError::Conflict)?;
        for work in self.cached_trust_work_latest_unlocked()? {
            match latest.get(&work.id) {
                Some(current) if current.version >= work.version => {}
                _ => {
                    latest.insert(work.id.clone(), work);
                }
            }
        }
        Ok(latest)
    }

    /// One Work's complete version chain from both journals, strictly in
    /// version order. Ties (the same version present in both journals) keep
    /// the ledger record, matching [`Self::latest_works_unlocked`].
    pub fn work_history(&self, work_id: &str) -> StoreResult<Vec<WorkJournalRecord>> {
        let mut records = self
            .work_journal_unlocked()?
            .records
            .iter()
            .filter(|record| record.work.id == work_id)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.event
                .resulting_version
                .cmp(&right.event.resulting_version)
                .then(left.source.cmp(&right.source))
                .then_with(|| left.event.id.cmp(&right.event.id))
        });
        Ok(records)
    }

    /// Every Work record in the store, ledger rows first in durable append
    /// order, then trust Work transitions in `store_sequence` order.
    pub fn work_journal_records(&self) -> StoreResult<Vec<WorkJournalRecord>> {
        Ok(self.work_journal_unlocked()?.records.clone())
    }

    pub(crate) fn work_journal_records_for_ids_unlocked(
        &self,
        work_ids: &HashSet<String>,
    ) -> StoreResult<Vec<WorkJournalRecord>> {
        Ok(self
            .work_journal_unlocked()?
            .records
            .iter()
            .filter(|record| work_ids.contains(&record.work.id))
            .cloned()
            .collect())
    }

    pub(crate) fn work_journal_events_for_ids_unlocked(
        &self,
        work_ids: &HashSet<String>,
    ) -> StoreResult<Vec<WorkEvent>> {
        Ok(self
            .work_journal_records_for_ids_unlocked(work_ids)?
            .into_iter()
            .map(|record| record.event)
            .collect())
    }

    /// How far the Work journal has advanced store-wide.
    pub fn work_journal_position(&self) -> StoreResult<WorkJournalPosition> {
        Ok(self.work_journal_unlocked()?.position)
    }

    /// Per-run delta cursors. Positions are 1-based per-run indexes within
    /// each journal, so a row appended for another TeamRun never shifts this
    /// run's cursors, exactly as the pre-W3 ledger-only cursor behaved.
    pub fn work_journal_cursors_for_team_run(
        &self,
        team_run_id: &str,
    ) -> StoreResult<WorkJournalRunCursors> {
        let journal = self.work_journal_unlocked()?;
        let mut cursors = WorkJournalRunCursors::default();
        let mut ledger = 0u64;
        let mut trust = 0u64;
        for record in &journal.records {
            if record.event.team_run_id != team_run_id {
                continue;
            }
            let position = match record.source {
                WorkJournalSource::Ledger => {
                    ledger += 1;
                    WorkJournalPosition { ledger, trust: 0 }
                }
                WorkJournalSource::Trust => {
                    trust += 1;
                    WorkJournalPosition { ledger: 0, trust }
                }
            };
            let entry = cursors.by_work.entry(record.work.id.clone()).or_default();
            *entry = entry.merged_max(position);
        }
        cursors.watermark = WorkJournalPosition { ledger, trust };
        Ok(cursors)
    }

    pub(super) fn work_journal_unlocked(&self) -> StoreResult<Arc<WorkJournal>> {
        let sources = self.current_work_sources()?;
        let trust = self.trust_read_model()?;
        self.cached_combined_projection(
            "work-journal",
            vec![sources.clone(), trust.clone()],
            || {
                let mut records = sources
                    .recovered
                    .clone()
                    .map_err(StoreError::Conflict)?
                    .into_iter()
                    .map(|operation| WorkJournalRecord {
                        source: WorkJournalSource::Ledger,
                        event: operation.event,
                        work: operation.work,
                    })
                    .collect::<Vec<_>>();
                let ledger = records.len() as u64;
                let trust_records = trust.work_journal_records()?;
                let trust = trust_records.len() as u64;
                records.extend(trust_records);
                Ok(WorkJournal {
                    records,
                    position: WorkJournalPosition { ledger, trust },
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(ledger: u64, trust: u64) -> WorkJournalPosition {
        WorkJournalPosition { ledger, trust }
    }

    #[test]
    fn a_legacy_integer_cursor_decodes_to_the_ledger_position_it_meant() {
        assert_eq!(WorkJournalPosition::from_packed(0), position(0, 0));
        assert_eq!(WorkJournalPosition::from_packed(7), position(7, 0));
        assert_eq!(
            position(7, 0).packed(),
            7,
            "no trust rows: still a row count"
        );
    }

    #[test]
    fn a_trust_bearing_position_orders_after_every_legacy_value() {
        assert!(position(0, 1).packed() > position(u32::MAX as u64 - 1, 0).packed());
        let packed = position(6, 3).packed();
        assert_eq!(WorkJournalPosition::from_packed(packed), position(6, 3));
    }

    #[test]
    fn advancing_either_journal_advances_past_the_cursor() {
        let cursor = position(5, 2);
        assert!(position(6, 2).advanced_past(&cursor), "a new ledger row");
        assert!(position(5, 3).advanced_past(&cursor), "a new trust row");
        assert!(!position(5, 2).advanced_past(&cursor), "no new row");
        assert!(!position(4, 1).advanced_past(&cursor), "older on both");
        assert!(
            position(6, 0).advanced_past(&cursor),
            "a Work with no trust row is never hidden by another Work's trust row"
        );
    }

    #[test]
    fn the_watermark_is_the_componentwise_maximum() {
        assert_eq!(position(6, 1).merged_max(position(2, 3)), position(6, 3));
        assert_eq!(position(0, 0).total(), 0);
        assert_eq!(position(6, 3).total(), 9);
    }
}
