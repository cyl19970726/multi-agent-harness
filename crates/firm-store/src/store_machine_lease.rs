//! The machine lease as a Store operation (ADR 0075).
//!
//! This is the seam between the node-file mechanism (`node_lease_document`,
//! `node_lease_history`, `node_lease_lock`) and the Store API every fence and
//! writer already speaks. Acquire, renew, drain and release move here from
//! `store_node_runtime`; the per-Space `node_daemon_leases.jsonl` writers stay
//! alive until E2b so pre-cutover stores keep decoding, but nothing in this
//! module touches them.
//!
//! The lock rule is the load-bearing part. Every function here takes the lease
//! lock, does its two file operations, and releases — no Space I/O, no second
//! lock, no provider call in between. That is what stops a heartbeat queueing
//! behind an 8.75 MB Work write, which is the failure that cost generation 4
//! the machine by two milliseconds.

use super::*;

use crate::node_lease_document::{
    publish_lease_document, read_lease_document, LeaseClock, NodeDaemonLeaseDocument,
};
use crate::node_lease_history::{append_lease_history, generation_was_released};
use crate::node_lease_lock::{lease_lock_timeout, NodeLeaseLock};

/// Which record answered "who owns this machine".
///
/// Typed rather than implied, because the two sources do not carry the same
/// authority and a reader must not be able to forget which one it got. Only
/// [`MachineLeaseSource::NodeFile`] authorizes a provider effect; a
/// `LegacySpaceRow` is pre-cutover history that projections may display and
/// fences must refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineLeaseSource {
    NodeFile,
    LegacySpaceRow,
}

impl MachineLeaseSource {
    /// The one question a fence asks. Written as a method so the answer cannot
    /// be re-derived, differently, at each of the 46 call sites.
    pub fn authorizes_provider_effect(self) -> bool {
        matches!(self, Self::NodeFile)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NodeFile => "node_file",
            Self::LegacySpaceRow => "legacy_space_row",
        }
    }
}

/// `MACHINE_LEASE_NOT_AUTHORITATIVE` — a fence resolved a lease, but from the
/// legacy Space rows, which never authorize an effect after cutover.
pub const MACHINE_LEASE_NOT_AUTHORITATIVE: &str = "MACHINE_LEASE_NOT_AUTHORITATIVE";

impl HarnessStore {
    /// Resolve who owns this machine, and say where the answer came from.
    ///
    /// Lock-free: atomic replace means there is no half-written document to
    /// observe, which is strictly stronger than the torn-tail retry an
    /// append-only ledger needs. Expiry is judged against the caller's clock,
    /// as the fences already do.
    ///
    /// A Store that cannot name its node home fails closed with
    /// `MACHINE_LEASE_FILE_UNRESOLVED` rather than falling back to Space rows —
    /// "I do not know who owns this machine" is never "nobody does".
    pub fn current_machine_lease(
        &self,
        node_id: &str,
    ) -> StoreResult<Option<(NodeDaemonLease, MachineLeaseSource)>> {
        let node_home = self.node_home(node_id)?;
        if let Some(document) = read_lease_document(&node_home, node_id)? {
            return Ok(Some((document.lease, MachineLeaseSource::NodeFile)));
        }
        // No document yet: a pre-cutover store. The row is readable and is what
        // `daemon status` should show, but it is not authority.
        Ok(self
            .latest_node_daemon_lease(node_id)?
            .map(|lease| (lease, MachineLeaseSource::LegacySpaceRow)))
    }

    /// The fence form: resolve, and refuse anything a provider effect may not
    /// be built on. Every one of ADR 0075's decider sites goes through this, so
    /// "only NodeFile authorizes" is one predicate rather than 46 copies.
    pub fn authoritative_machine_lease(&self, node_id: &str) -> StoreResult<NodeDaemonLease> {
        match self.current_machine_lease(node_id)? {
            Some((lease, source)) if source.authorizes_provider_effect() => Ok(lease),
            Some((_, source)) => Err(StoreError::Conflict(format!(
                "{MACHINE_LEASE_NOT_AUTHORITATIVE}: Node {node_id} resolved from {} and cannot authorize a provider effect; this Store predates the machine lease cutover",
                source.as_str()
            ))),
            None => Err(StoreError::Conflict(format!(
                "{MACHINE_LEASE_NOT_AUTHORITATIVE}: Node {node_id} has no machine lease"
            ))),
        }
    }

    /// Where this machine's lease document lives, for `daemon status`.
    pub fn machine_lease_path(&self, node_id: &str) -> StoreResult<PathBuf> {
        Ok(crate::node_lease_document::lease_document_path(
            &self.node_home(node_id)?,
        ))
    }

    /// Did this exact past generation publish `Released`?
    ///
    /// The one historical question anyone asks of the lease, and the reason the
    /// document has a companion history file: expiry is not a provider-drain
    /// receipt, so reattach must distinguish "released" from "timed out". False
    /// is the fail-closed answer and covers "no record either way".
    pub fn machine_generation_was_released(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
    ) -> StoreResult<bool> {
        generation_was_released(&self.node_home(node_id)?, node_id, daemon_id, generation)
    }

    /// Take the machine for a new daemon generation.
    ///
    /// `previous_space_generations` is the cutover input: on a node whose
    /// authority still lives in Space rows, the first document must start above
    /// every generation any Space ever issued, or a successor could reuse a
    /// number a predecessor already drove under.
    pub fn acquire_machine_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        ttl_ms: u64,
        previous_space_generations: &[u64],
    ) -> StoreResult<NodeDaemonLease> {
        self.write_machine_lease(node_id, ttl_ms, |current, now| {
            let generation = match current {
                Some(document) => {
                    let lease = &document.lease;
                    if lease.status != NodeDaemonLeaseStatus::Released {
                        if lease.daemon_id == daemon_id
                            && lease.instance_id == instance_id
                            && lease.status == NodeDaemonLeaseStatus::Active
                            && lease.expires_unix_ms > now
                        {
                            return Ok(None);
                        }
                        let code = if lease.daemon_id == daemon_id
                            && lease.instance_id == instance_id
                        {
                            "NODE_DAEMON_PREDECESSOR_SETTLEMENT_REQUIRED"
                        } else {
                            "NODE_DAEMON_PREDECESSOR_RECOVERY_REQUIRED"
                        };
                        return Err(StoreError::Conflict(format!(
                            "{code}: Node {node_id} generation {} owned by daemon {} instance {} is {:?}, not explicitly Released",
                            lease.generation, lease.daemon_id, lease.instance_id, lease.status
                        )));
                    }
                    lease.generation
                }
                // Cutover: start above every generation the Spaces issued.
                None => previous_space_generations.iter().copied().max().unwrap_or(0),
            };
            let generation = generation.checked_add(1).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "NODE_DAEMON_GENERATION_CEILING: Node {node_id} cannot advance past generation {generation}"
                ))
            })?;
            Ok(Some(NodeDaemonLease {
                node_id: node_id.to_string(),
                daemon_id: daemon_id.to_string(),
                generation,
                instance_id: instance_id.to_string(),
                status: NodeDaemonLeaseStatus::Active,
                acquired_unix_ms: now,
                renewed_unix_ms: now,
                expires_unix_ms: now.saturating_add(ttl_ms.max(1)),
                released_unix_ms: None,
            }))
        })
    }

    /// Extend this exact generation. One write per machine, not one per Space.
    pub fn renew_machine_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.write_machine_lease(node_id, ttl_ms, |current, now| {
            let mut lease = require_exact_generation(
                current, node_id, daemon_id, generation, instance_id,
            )?;
            if lease.status != NodeDaemonLeaseStatus::Active || lease.expires_unix_ms <= now {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_GENERATION_FENCED: {daemon_id} generation {generation} no longer owns Node {node_id}"
                )));
            }
            lease.renewed_unix_ms = now;
            lease.expires_unix_ms = now.saturating_add(ttl_ms.max(1));
            Ok(Some(lease))
        })
    }

    /// Enter predecessor settlement: no new provider effect, obligations kept.
    pub fn drain_machine_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        drain_ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.write_machine_lease(node_id, drain_ttl_ms, |current, now| {
            let mut lease = require_exact_generation(
                current, node_id, daemon_id, generation, instance_id,
            )?;
            if lease.status == NodeDaemonLeaseStatus::Draining {
                return Ok(None);
            }
            if lease.status != NodeDaemonLeaseStatus::Active {
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_GENERATION_FENCED: Node {node_id} cannot enter predecessor settlement from {:?}",
                    lease.status
                )));
            }
            lease.status = NodeDaemonLeaseStatus::Draining;
            lease.renewed_unix_ms = now;
            lease.expires_unix_ms = now.saturating_add(drain_ttl_ms.max(1));
            Ok(Some(lease))
        })
    }

    /// Publish `Released`, the one thing a successor may acquire after.
    ///
    /// The settlement proof is the caller's to gather — from every registered
    /// Space, before this is called — so that the publish is all-or-nothing and
    /// `authority_released: false` stops meaning "partly".
    pub fn release_machine_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
    ) -> StoreResult<NodeDaemonLease> {
        self.write_machine_lease(node_id, 0, |current, now| {
            let mut lease =
                require_exact_generation(current, node_id, daemon_id, generation, instance_id)?;
            if lease.status == NodeDaemonLeaseStatus::Released {
                return Ok(None);
            }
            lease.status = NodeDaemonLeaseStatus::Released;
            lease.renewed_unix_ms = now;
            lease.expires_unix_ms = now;
            lease.released_unix_ms = Some(now);
            Ok(Some(lease))
        })
    }

    /// One lease-lock critical section: read, decide, publish, record.
    ///
    /// `next` returning `Ok(None)` means "already in the requested state" — an
    /// idempotent no-op that writes nothing and appends no history row, so a
    /// retried verb cannot inflate the generation history.
    fn write_machine_lease(
        &self,
        node_id: &str,
        ttl_ms: u64,
        next: impl FnOnce(Option<&NodeDaemonLeaseDocument>, u64) -> StoreResult<Option<NodeDaemonLease>>,
    ) -> StoreResult<NodeDaemonLease> {
        let node_home = self.node_home(node_id)?;
        let lock = NodeLeaseLock::acquire(
            &node_home,
            lease_lock_timeout(Duration::from_millis(ttl_ms.max(1))),
        )?;
        let clock = LeaseClock::system();
        let mut settled: Option<NodeDaemonLease> = None;
        let mut recorded_at = 0;
        let document = publish_lease_document(&lock, &node_home, node_id, &clock, |now| {
            recorded_at = now;
            let current = read_lease_document(&node_home, node_id)?;
            match next(current.as_ref(), now)? {
                Some(lease) => Ok(NodeDaemonLeaseDocument::new(lease, std::process::id())),
                None => {
                    let unchanged = current.ok_or_else(|| {
                        StoreError::Conflict(format!(
                            "NODE_DAEMON_GENERATION_FENCED: {node_id} has no machine lease to keep"
                        ))
                    })?;
                    settled = Some(unchanged.lease.clone());
                    Ok(unchanged)
                }
            }
        })?;
        if settled.is_none() {
            // One row per generation transition, never per renewal: a renewal
            // keeps the generation, so only a status change is history.
            let is_transition = read_lease_history_tail(&node_home)?.is_none_or(|last| {
                last.generation != document.lease.generation || last.status != document.lease.status
            });
            if is_transition {
                append_lease_history(&lock, &node_home, &document, recorded_at)?;
            }
        }
        Ok(document.lease)
    }
}

fn read_lease_history_tail(node_home: &Path) -> StoreResult<Option<NodeDaemonLease>> {
    Ok(crate::node_lease_history::read_lease_history(node_home)?
        .pop()
        .map(|row| row.lease))
}

/// Every write past the first names the exact generation it believes it owns.
/// A mismatch is a stale daemon, never a repair.
fn require_exact_generation(
    current: Option<&NodeDaemonLeaseDocument>,
    node_id: &str,
    daemon_id: &str,
    generation: u64,
    instance_id: &str,
) -> StoreResult<NodeDaemonLease> {
    let lease = current
        .map(|document| document.lease.clone())
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
    if lease.daemon_id != daemon_id
        || lease.generation != generation
        || lease.instance_id != instance_id
    {
        return Err(StoreError::Conflict(format!(
            "NODE_DAEMON_GENERATION_FENCED: stale daemon cannot write Node {node_id}"
        )));
    }
    Ok(lease)
}
