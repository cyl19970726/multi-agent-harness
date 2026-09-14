//! The one reader for Work.
//!
//! A Work's version chain is one chain, written to one journal and read from
//! two files — because a store that predates the writer cutover still holds
//! half its history in the other one:
//!
//! * `agentfirm_trust_operations.jsonl` is the Work journal. Every Work
//!   transition is a `work` aggregate envelope carrying its complete
//!   [`WorkOperation`] as an immutable side record. A pre-cutover envelope
//!   carries Submitted, Accepted, Cancelled or DependenciesChanged with a bare
//!   `WorkEvent`, and a `work_report/created` envelope carries a pre-cutover
//!   Review snapshot.
//! * `work_operations.jsonl` is legacy read-only input: the [`WorkOperation`]
//!   rows a pre-cutover binary appended. Nothing writes it, and a store created
//!   after the cutover never has it. The crash-atomic delegation composite this
//!   fold used to merge retired with the Work-ledger delegation stack; a store
//!   that still holds `work_delegation_operations.jsonl` keeps the file and no
//!   reader folds it.
//!
//! Every current-phase, event, record, count and cursor reader goes through
//! this module so no consumer can see half the chain. The module offers
//! exactly four shapes:
//!
//! 1. [`HarnessStore::latest_works`] / [`HarnessStore::current_work`] — the
//!    latest Work per id, plus any higher revision persisted only as an atomic
//!    side projection of another aggregate's envelope.
//! 2. [`HarnessStore::work_history`] — one Work's versions from both files,
//!    strictly in version order, as event + snapshot records.
//! 3. [`HarnessStore::work_journal_records`] / [`HarnessStore::work_events`] —
//!    every Work record in the store in one deterministic total order.
//! 4. [`HarnessStore::work_journal_position`] and
//!    [`HarnessStore::work_journal_cursors_for_team_run`] — a monotonic
//!    journal position whose trust component advances on every new write and
//!    whose ledger component is frozen at the pre-cutover row count.
//!
//! **Total order.** The two files share no comparable clock: ledger rows are
//! stamped with an ISO `created_at` and trust events with `unix-ms:`, and both
//! resolutions tie under scripted writes. The store-wide order is therefore
//! source-major and honest about it — every ledger row in its durable append
//! order, then every trust Work transition in `store_sequence` order. Only
//! *within one Work* is a real order available, and that one is the version
//! chain, which [`HarnessStore::work_history`] uses.
//!
//! **What is still ledger-only.** Two reads deliberately answer about the
//! legacy file alone: [`HarnessStore::legacy_work_operation_rows`], which is
//! what "read the pre-cutover rows" means, and the raw fold
//! `reconcile_work_projection_provenance` repairs from — that verb exists to
//! repair a sparse row *in that file*, so the sparse row is the thing it is
//! asked about. Everything else reads the journal.
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
    /// The Execution Space the canonical trust envelope was written in, or
    /// `None` for a ledger row. `work_operations.jsonl` is the store's own
    /// file and carries no space of its own; the trust journal is explicitly
    /// scoped, and a physical store may temporarily hold more than one space
    /// during recovery or import, so a space-scoped reader must never fold
    /// another scope's trust truth.
    pub execution_space_id: Option<String>,
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

    /// Render the position as the single integer the `--since` transport
    /// carries.
    ///
    /// A component that does not fit is a refusal, never a clamp: a silently
    /// clamped ledger component would freeze at the radix and a Host loop would
    /// stop seeing its own ledger rows without any error to act on. A store
    /// large enough to reach either bound needs a wider cursor, and must say so.
    pub fn packed(&self) -> StoreResult<u64> {
        if self.ledger >= Self::PACKED_LEDGER_RADIX {
            return Err(StoreError::Conflict(format!(
                "WORK_JOURNAL_CURSOR_OVERFLOW: ledger position {} does not fit the {} packing radix; \
                 the single-integer cursor cannot name this position",
                self.ledger,
                Self::PACKED_LEDGER_RADIX
            )));
        }
        self.trust
            .checked_mul(Self::PACKED_LEDGER_RADIX)
            .and_then(|high| high.checked_add(self.ledger))
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "WORK_JOURNAL_CURSOR_OVERFLOW: trust position {} overflows the packed cursor",
                    self.trust
                ))
            })
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

    /// The merged current Work for one id inside one Execution Space: this
    /// store's legacy ledger rows, plus only the trust transitions written in
    /// `execution_space_id`. Use this wherever the caller holds a space; the
    /// unscoped form above exists for store-wide reads that genuinely have no
    /// space to narrow with.
    pub fn current_work_in_space(
        &self,
        execution_space_id: &ExecutionSpaceId,
        work_id: &str,
    ) -> StoreResult<Option<Work>> {
        Ok(self
            .latest_works_in_space_unlocked(execution_space_id)?
            .remove(work_id))
    }

    pub(super) fn latest_works_in_space_unlocked(
        &self,
        execution_space_id: &ExecutionSpaceId,
    ) -> StoreResult<BTreeMap<String, Work>> {
        Ok(fold_latest_works(
            self.work_journal_unlocked()?
                .records
                .iter()
                .filter(|record| {
                    record
                        .execution_space_id
                        .as_deref()
                        .is_none_or(|space| space == execution_space_id.as_str())
                }),
        ))
    }

    /// Latest Work per id across both journals: the highest revision of each
    /// Work the store holds, from the one provenance-recovered journal, then
    /// any higher revision persisted only as an atomic side projection of
    /// another aggregate's envelope.
    ///
    /// That second source is not a second journal. It is a persisted shape
    /// this store already contains — a Work revision committed atomically
    /// beside the record that produced it, with no `work` transition of its
    /// own — and dropping it would make a real revision invisible.
    pub(super) fn latest_works_unlocked(&self) -> StoreResult<BTreeMap<String, Work>> {
        let mut latest = fold_latest_works(self.work_journal_unlocked()?.records.iter());
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
    /// version order. History hides no persisted row, so if one version is
    /// somehow present in both journals both records are returned, the ledger
    /// one first — the same precedence [`Self::latest_works_unlocked`] applies
    /// when it folds a tie.
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

    /// Every Work record this store's ledger holds, plus only the trust Work
    /// transitions written in `execution_space_id`.
    pub fn work_journal_records_for_space(
        &self,
        execution_space_id: &ExecutionSpaceId,
    ) -> StoreResult<Vec<WorkJournalRecord>> {
        Ok(self
            .work_journal_unlocked()?
            .records
            .iter()
            .filter(|record| {
                record
                    .execution_space_id
                    .as_deref()
                    .is_none_or(|space| space == execution_space_id.as_str())
            })
            .cloned()
            .collect())
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

    /// Work events for these ids, narrowed to one Execution Space.
    pub(crate) fn work_journal_events_for_ids_in_space_unlocked(
        &self,
        execution_space_id: &ExecutionSpaceId,
        work_ids: &HashSet<String>,
    ) -> StoreResult<Vec<WorkEvent>> {
        Ok(self
            .work_journal_records_for_space(execution_space_id)?
            .into_iter()
            .filter(|record| work_ids.contains(&record.work.id))
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
                        execution_space_id: None,
                        event: operation.event,
                        work: operation.work,
                    })
                    .collect::<Vec<_>>();
                let ledger = records.len() as u64;
                let trust_records = trust.work_journal_records()?;
                let trust = trust_records.len() as u64;
                records.extend(trust_records);
                refuse_divergent_duplicate_revisions(&records)?;
                recover_journal_provenance(&mut records)?;
                Ok(WorkJournal {
                    records,
                    position: WorkJournalPosition { ledger, trust },
                })
            },
        )
    }
}

/// A Work revision may be persisted twice — the pre-cutover Review snapshot and
/// its paired `work`/`submitted` envelope are the same revision in two shapes —
/// but the two copies must say the same thing.
///
/// Before W4 both journals had live writers and the reader broke a tie by
/// preferring the ledger. Since the cutover only one writer produces new
/// revisions, so a version that appears twice with DIFFERENT content is not a
/// tie: it is two disagreeing accounts of one revision, and picking either
/// would make the store's answer depend on which file a reader looked at
/// first. The reader refuses instead.
fn refuse_divergent_duplicate_revisions(records: &[WorkJournalRecord]) -> StoreResult<()> {
    let mut seen = BTreeMap::<(&str, u64), &Work>::new();
    for record in records {
        let key = (record.work.id.as_str(), record.event.resulting_version);
        match seen.get(&key) {
            Some(existing) if **existing != record.work => {
                return Err(StoreError::Conflict(format!(
                    "WORK_JOURNAL_REVISION_CONFLICT: Work {} version {} is persisted twice \
                     with different content; one revision cannot have two projections",
                    record.work.id, record.event.resulting_version
                )));
            }
            Some(_) => {}
            None => {
                seen.insert(key, &record.work);
            }
        }
    }
    Ok(())
}

/// Highest revision per Work id. A version tie keeps the record already held,
/// and ledger records are folded before trust ones, so a revision present in
/// both journals still resolves to the ledger projection it always did.
fn fold_latest_works<'a>(
    records: impl Iterator<Item = &'a WorkJournalRecord>,
) -> BTreeMap<String, Work> {
    let mut latest = BTreeMap::<String, Work>::new();
    for record in records {
        match latest.get(&record.work.id) {
            Some(current) if current.version >= record.work.version => {}
            _ => {
                latest.insert(record.work.id.clone(), record.work.clone());
            }
        }
    }
    latest
}

/// Fold immutable additive provenance forward through each Work's version
/// chain, across both journals.
///
/// A stale mixed-version writer may still append a `work_operations.jsonl` row
/// that dropped `accountable_team_id` or `created_by_member_id`. Once either
/// fact is established it is immutable, so a later sparse revision inherits
/// it and a later *conflicting* value stays corruption and is refused. Doing
/// this per journal was enough only while one journal held a whole chain;
/// after the W4 writer cutover a Work's creation lives in the trust journal
/// and the sparse row does not, so the fold must span both or the recovered
/// fact is lost exactly when it is needed.
fn recover_journal_provenance(records: &mut [WorkJournalRecord]) -> StoreResult<()> {
    let mut order = (0..records.len()).collect::<Vec<_>>();
    order.sort_by(|&left, &right| {
        records[left]
            .work
            .id
            .cmp(&records[right].work.id)
            .then(
                records[left]
                    .event
                    .resulting_version
                    .cmp(&records[right].event.resulting_version),
            )
            .then(records[left].source.cmp(&records[right].source))
    });
    let mut team_id: Option<(String, String)> = None;
    let mut creator_id: Option<(String, String)> = None;
    for index in order {
        let work_id = records[index].work.id.clone();
        let event_id = records[index].event.id.clone();
        for (established, actual, field) in [
            (
                &mut team_id,
                records[index].work.accountable_team_id.clone(),
                "accountable_team_id",
            ),
            (
                &mut creator_id,
                records[index].work.created_by_member_id.clone(),
                "created_by_member_id",
            ),
        ] {
            let known = established
                .as_ref()
                .filter(|(id, _)| *id == work_id)
                .map(|(_, value)| value.clone());
            match (known, actual) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed {field} from {expected} to {actual} in event {event_id}"
                    )));
                }
                (Some(expected), None) => match field {
                    "accountable_team_id" => {
                        records[index].work.accountable_team_id = Some(expected)
                    }
                    _ => records[index].work.created_by_member_id = Some(expected),
                },
                (_, Some(actual)) => *established = Some((work_id.clone(), actual)),
                (None, None) => {}
            }
        }
    }
    Ok(())
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
            position(7, 0).packed().unwrap(),
            7,
            "no trust rows: still a row count"
        );
    }

    #[test]
    fn a_trust_bearing_position_orders_after_every_legacy_value() {
        assert!(
            position(0, 1).packed().unwrap() > position(u32::MAX as u64 - 1, 0).packed().unwrap()
        );
        let packed = position(6, 3).packed().unwrap();
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
    fn a_position_the_single_integer_cursor_cannot_name_is_refused_not_clamped() {
        let overflowing = position(WorkJournalPosition::PACKED_LEDGER_RADIX, 0);
        let error = overflowing
            .packed()
            .expect_err("a ledger position at the radix cannot be packed");
        assert!(
            error.to_string().contains("WORK_JOURNAL_CURSOR_OVERFLOW"),
            "the refusal must name itself so a Host loop can act on it: {error}"
        );
        assert!(position(WorkJournalPosition::PACKED_LEDGER_RADIX - 1, 0)
            .packed()
            .is_ok());
        assert!(
            position(0, u64::MAX).packed().is_err(),
            "trust overflows too"
        );
    }

    #[test]
    fn the_watermark_is_the_componentwise_maximum() {
        assert_eq!(position(6, 1).merged_max(position(2, 3)), position(6, 3));
        assert_eq!(position(0, 0).total(), 0);
        assert_eq!(position(6, 3).total(), 9);
    }
}
