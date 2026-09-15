//! One row per generation transition, beside the machine lease (ADR 0075).
//!
//! The lease document is deliberately not a history: it answers "who owns this
//! machine now". Exactly one reader needs more than that. `predecessor_was_released`
//! asks whether a **named past generation** reached `Released`, because lease
//! expiry is not a provider-drain receipt — a session whose predecessor merely
//! timed out may still have a live provider process behind it, and reattaching
//! to that is how two runtime generations end up driving one native session.
//!
//! In the Space-row world that proof survived only as a side effect of the
//! heartbeat compactor, which kept first-of-group plus last-of-each-status per
//! generation for exactly this reason. Moving to a latest-state document would
//! have deleted it on the successor's very first acquire, so the proof gets its
//! own file rather than an implicit dependency on a compaction rule.
//!
//! It stays small because it grows per **generation**, not per renewal: in the
//! live evidence the busiest node took 14 generations in 3 days and 79 in 12 —
//! about 28 KB — against the 87,354 rows and 28 MB that heartbeat appends
//! produced before they were compacted. There is nothing here to compact.

use super::*;
use crate::node_lease_document::{
    read_lease_document, NodeDaemonLeaseDocument, MACHINE_LEASE_DOCUMENT_INVALID,
};
use crate::node_lease_lock::NodeLeaseLock;

/// `<node_home>/node-daemon-lease-history.jsonl`.
pub(crate) fn lease_history_path(node_home: &Path) -> PathBuf {
    node_home.join("node-daemon-lease-history.jsonl")
}

/// One generation transition.
///
/// The row carries the lease exactly as the document held it at that moment,
/// so a reader answers questions about a past generation with the same fields
/// it would use for the current one — no second vocabulary, no projection to
/// keep in step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDaemonLeaseHistoryRow {
    pub schema_version: u32,
    pub recorded_unix_ms: u64,
    #[serde(flatten)]
    pub lease: NodeDaemonLease,
}

/// Append one transition. Called under the lease lock, inside the same write
/// that publishes the document, so the history can never describe a transition
/// that did not land.
///
/// A single `write_all` of one line is the unit of atomicity a lock-free reader
/// relies on; the reader below tolerates the one short window where the
/// trailing newline is not yet visible.
pub(crate) fn append_lease_history(
    _lock: &NodeLeaseLock,
    node_home: &Path,
    document: &NodeDaemonLeaseDocument,
    recorded_unix_ms: u64,
) -> StoreResult<()> {
    let row = NodeDaemonLeaseHistoryRow {
        schema_version: document.schema_version,
        recorded_unix_ms,
        lease: document.lease.clone(),
    };
    let mut bytes = serde_json::to_vec(&row)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(lease_history_path(node_home))?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// Read the history without taking any lock.
///
/// A concurrent single-line append can leave the final row without its newline
/// for a moment. Rather than invent a new tolerance, this drops exactly that
/// unmistakably incomplete final line — a row that is not yet fully visible is
/// a row that has not yet happened, and the next read will see it. A line that
/// is complete but undecodable is a real corruption and is reported.
pub(crate) fn read_lease_history(node_home: &Path) -> StoreResult<Vec<NodeDaemonLeaseHistoryRow>> {
    let path = lease_history_path(node_home);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StoreError::Io(error)),
    };
    let text = String::from_utf8_lossy(&bytes);
    let complete = text.ends_with('\n');
    let mut lines = text.lines().collect::<Vec<_>>();
    if !complete {
        lines.pop();
    }
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(
            serde_json::from_str::<NodeDaemonLeaseHistoryRow>(line).map_err(|error| {
                StoreError::Conflict(format!(
                "{MACHINE_LEASE_DOCUMENT_INVALID}: {} holds an undecodable history row: {error}",
                path.display()
            ))
            })?,
        );
    }
    Ok(rows)
}

/// Did this exact past generation publish `Released`?
///
/// `false` is the fail-closed answer and covers both "it did not" and "there is
/// no record either way". A caller must keep treating that as "no drain
/// receipt" — the refusal it already writes — rather than reading silence as
/// permission. The current document is consulted too, because the generation in
/// question may still be the one on disk.
pub(crate) fn generation_was_released(
    node_home: &Path,
    node_id: &str,
    daemon_id: &str,
    generation: u64,
) -> StoreResult<bool> {
    let matches_generation = |lease: &NodeDaemonLease| {
        lease.node_id == node_id && lease.daemon_id == daemon_id && lease.generation == generation
    };
    if let Some(document) = read_lease_document(node_home, node_id)? {
        if matches_generation(&document.lease) {
            return Ok(document.lease.status == NodeDaemonLeaseStatus::Released);
        }
    }
    Ok(read_lease_history(node_home)?
        .iter()
        .rev()
        .find(|row| matches_generation(&row.lease))
        .is_some_and(|row| row.lease.status == NodeDaemonLeaseStatus::Released))
}
