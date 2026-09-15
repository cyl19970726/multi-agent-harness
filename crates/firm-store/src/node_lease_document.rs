//! The one machine lease document (ADR 0075).
//!
//! `<FIRM_HOME>/nodes/<node_id>/node-daemon-lease.json` holds the current — and
//! only — record of who owns this machine. It replaces a lease row appended to
//! every Execution Space's `node_daemon_leases.jsonl` under that Space's
//! `.store.lock`, which is how a heartbeat came to queue 14,996 ms behind
//! ordinary data writes against a 14,994 ms budget and lose the machine by two
//! milliseconds of queueing.
//!
//! Three properties carry that weight, and each is structural rather than
//! conventional:
//!
//! - **Atomic replace.** A reader sees the whole previous document or the
//!   whole next one, never a mixture. That is strictly stronger than the
//!   torn-tail retry an append-only ledger needs, which is why readers here
//!   take no lock at all.
//! - **Time sampled inside the lock, immediately before the rename.** The
//!   writer samples the clock itself and hands it to the caller, so a queued
//!   writer structurally cannot carry a pre-lock timestamp past expiry.
//! - **Monotonic `generation` and `expires`.** A write that would move either
//!   backwards is refused by name, which bounds a backwards system-clock step
//!   and makes a resurrected stale document detectable instead of authoritative.

use super::*;
use crate::node_lease_lock::NodeLeaseLock;
use crate::store_node_runtime::current_store_unix_ms;

/// `MACHINE_LEASE_DOCUMENT_INVALID` — the document on disk cannot be trusted
/// to answer who owns this machine: wrong node, wrong schema, or a write that
/// would move authority backwards.
pub const MACHINE_LEASE_DOCUMENT_INVALID: &str = "MACHINE_LEASE_DOCUMENT_INVALID";

/// The document's own schema version, independent of the lease fields it
/// carries. A document from the future is refused rather than guessed at: an
/// unreadable authority record must never read as "nobody owns this machine".
pub const NODE_LEASE_DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// `<node_home>/node-daemon-lease.json`.
pub(crate) fn lease_document_path(node_home: &Path) -> PathBuf {
    node_home.join("node-daemon-lease.json")
}

/// The staging file for an atomic replace. Deliberately in the same directory
/// as its target: `rename` is only atomic within one filesystem.
fn lease_document_tmp_path(node_home: &Path) -> PathBuf {
    node_home.join("node-daemon-lease.json.tmp")
}

/// The machine lease as it lives on disk.
///
/// It carries today's `NodeDaemonLease` unchanged so the fences, projections
/// and legacy rows keep speaking one vocabulary, plus the two things a file —
/// as opposed to a ledger row — has to say for itself: which schema wrote it,
/// and which process owns it. The pid is what lets a successor prove a
/// predecessor is dead without opening a second source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDaemonLeaseDocument {
    pub schema_version: u32,
    pub owner_pid: u32,
    #[serde(flatten)]
    pub lease: NodeDaemonLease,
}

impl NodeDaemonLeaseDocument {
    pub fn new(lease: NodeDaemonLease, owner_pid: u32) -> Self {
        Self {
            schema_version: NODE_LEASE_DOCUMENT_SCHEMA_VERSION,
            owner_pid,
            lease,
        }
    }
}

/// Read the machine lease without taking any lock.
///
/// Lock-free is safe here precisely because writes are atomic replaces: there
/// is no partially written state to observe. `Ok(None)` means no daemon has
/// ever owned this node — the one honest "nobody", distinct from every failure
/// below, which is refused by name.
pub(crate) fn read_lease_document(
    node_home: &Path,
    node_id: &str,
) -> StoreResult<Option<NodeDaemonLeaseDocument>> {
    let path = lease_document_path(node_home);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::Io(error)),
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let document: NodeDaemonLeaseDocument = serde_json::from_slice(&bytes).map_err(|error| {
        StoreError::Conflict(format!(
            "{MACHINE_LEASE_DOCUMENT_INVALID}: {} does not decode as a machine lease document: {error}",
            path.display()
        ))
    })?;
    require_document_belongs_here(&document, node_id, &path)?;
    Ok(Some(document))
}

/// A document is only authority for the node whose directory it sits in, under
/// a schema this build understands. Both mismatches are refusals, never
/// repairs: silently adopting a foreign or newer record is how one machine
/// starts answering for another.
fn require_document_belongs_here(
    document: &NodeDaemonLeaseDocument,
    node_id: &str,
    path: &Path,
) -> StoreResult<()> {
    if document.schema_version > NODE_LEASE_DOCUMENT_SCHEMA_VERSION {
        return Err(StoreError::Conflict(format!(
            "{MACHINE_LEASE_DOCUMENT_INVALID}: {} was written by schema_version {} and this build understands at most {NODE_LEASE_DOCUMENT_SCHEMA_VERSION}",
            path.display(),
            document.schema_version
        )));
    }
    if document.lease.node_id != node_id {
        return Err(StoreError::Conflict(format!(
            "{MACHINE_LEASE_DOCUMENT_INVALID}: {} holds Node {} but sits in the directory of Node {node_id}",
            path.display(),
            document.lease.node_id
        )));
    }
    Ok(())
}

/// The clock an authority write samples.
///
/// Injected rather than read from a global because *when* `now` is taken is the
/// property ADR 0075 turns on, and it has to be assertable. Wall-clock tests
/// cannot express it: the store floors every lease at `now + 1`
/// (`store_node_runtime.rs:338`, `ttl_ms.max(1)`), so no TTL value can make a
/// freshly written lease already expired, and the only remaining lever is the
/// clock itself. That is the same arithmetic that made a `Some(0)` TTL a no-op
/// for #990.
pub(crate) struct LeaseClock<'a>(&'a dyn Fn() -> u64);

impl LeaseClock<'_> {
    /// The real wall clock. Production always uses this.
    pub(crate) fn system() -> LeaseClock<'static> {
        LeaseClock(&current_store_unix_ms)
    }

    fn now(&self) -> u64 {
        (self.0)()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl<'a> LeaseClock<'a> {
    /// A clock a test drives, so "sampled after the lock, before the rename"
    /// becomes an assertion instead of a race.
    pub(crate) fn injected(source: &'a dyn Fn() -> u64) -> Self {
        LeaseClock(source)
    }
}

/// Publish the next machine lease by atomic replace, under the lease lock.
///
/// The caller supplies `next` as a closure over the clock rather than a ready
/// value, and this function samples that clock **after** the lock is held and
/// immediately before serializing. That is the structural form of the rule the
/// Space-locked writer had to keep by hand: a writer that waited for the lock
/// cannot carry a stale `now` past its own expiry, because it never had one.
///
/// Returns the published document so the caller records exactly what landed
/// rather than what it intended.
pub(crate) fn publish_lease_document(
    _lock: &NodeLeaseLock,
    node_home: &Path,
    node_id: &str,
    clock: &LeaseClock<'_>,
    next: impl FnOnce(u64) -> StoreResult<NodeDaemonLeaseDocument>,
) -> StoreResult<NodeDaemonLeaseDocument> {
    let current = read_lease_document(node_home, node_id)?;
    // Sampled here: lock held, nothing else between this and the rename.
    let document = next(clock.now())?;
    require_document_belongs_here(&document, node_id, &lease_document_path(node_home))?;
    require_forward_only(current.as_ref(), &document, node_home)?;
    atomic_replace(node_home, &document)?;
    Ok(document)
}

/// Authority only ever moves forward on a node.
///
/// A write that would lower `generation` is a resurrected predecessor; one that
/// would shorten `expires` within a generation is a clock that stepped
/// backwards. Both are refused rather than clamped, because a lease that
/// silently shrinks is a lease whose holder believes it owns more time than the
/// file grants. Publishing the same document twice stays idempotent.
fn require_forward_only(
    current: Option<&NodeDaemonLeaseDocument>,
    next: &NodeDaemonLeaseDocument,
    node_home: &Path,
) -> StoreResult<()> {
    let Some(current) = current else {
        return Ok(());
    };
    if next.lease.generation < current.lease.generation {
        return Err(StoreError::Conflict(format!(
            "{MACHINE_LEASE_DOCUMENT_INVALID}: refusing to move Node {} from generation {} back to {} in {}",
            next.lease.node_id,
            current.lease.generation,
            next.lease.generation,
            node_home.display()
        )));
    }
    // Expiry is monotonic *within a status*, not across one. A renewal must
    // never shrink the lease — that is the backwards-clock case. But Draining
    // sets its own shorter window and Released deliberately expires the lease
    // now; clamping those would leave a released generation looking live, which
    // is the opposite of what the rule protects.
    if next.lease.generation == current.lease.generation
        && next.lease.status == current.lease.status
        && next.lease.expires_unix_ms < current.lease.expires_unix_ms
    {
        return Err(StoreError::Conflict(format!(
            "{MACHINE_LEASE_DOCUMENT_INVALID}: refusing to move Node {} generation {} {:?} expiry back from {} to {} in {}",
            next.lease.node_id,
            current.lease.generation,
            current.lease.status,
            current.lease.expires_unix_ms,
            next.lease.expires_unix_ms,
            node_home.display()
        )));
    }
    Ok(())
}

/// tmp in the same directory → fsync the tmp inode → rename → fsync the
/// directory.
///
/// The directory fsync is not optional and not cargo-culted: POSIX lets a
/// crash recover either directory entry after a rename, so without it a reboot
/// can resurrect the previous document — and with it a generation that has
/// already been superseded. The same reasoning is already written down for the
/// trust journal's compaction; this is the same primitive on a smaller file.
fn atomic_replace(node_home: &Path, document: &NodeDaemonLeaseDocument) -> StoreResult<()> {
    let tmp = lease_document_tmp_path(node_home);
    let target = lease_document_path(node_home);
    let mut bytes = serde_json::to_vec(document)?;
    bytes.push(b'\n');
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &target)?;
    if let Ok(dir) = File::open(node_home) {
        let _ = dir.sync_all();
    }
    Ok(())
}
