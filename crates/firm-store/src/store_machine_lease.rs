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
use crate::store_node_home::machine_lease_unresolved;

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

/// A machine lease that came from the node file, and therefore may authorize a
/// provider effect.
///
/// The ADR claims the typed source makes "only `NodeFile` authorizes" a
/// compile-time obligation rather than a review habit. A `(lease, source)` pair
/// does not deliver that: a caller can destructure it and drop the source with
/// no diagnostic. This does — the field is private and the only constructor is
/// the `NodeFile` arm of [`HarnessStore::authoritative_machine_lease`], so a
/// fence that wants a lease it may act on must name this type, and a
/// `LegacySpaceRow` can never be turned into one.
///
/// E2a-2's 46 deciders take this rather than a bare `NodeDaemonLease`, which is
/// why it exists before them: the obligation has to be in place before the call
/// sites are written, not retrofitted after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedMachineLease(NodeDaemonLease);

impl AuthorizedMachineLease {
    /// The lease, once the caller has been forced to obtain it from the one
    /// source that authorizes.
    pub fn lease(&self) -> &NodeDaemonLease {
        &self.0
    }

    pub fn into_lease(self) -> NodeDaemonLease {
        self.0
    }
}

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
    #[must_use = "the source decides whether this lease may authorize an effect; \
                  dropping it is how a legacy row becomes authority"]
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
    /// The refusal reason as one readable line.
    ///
    /// A fence wrapping this in its own typed error must not embed a whole
    /// serialized `TrustError` inside another one — the result is unreadable in
    /// a panic message and doubly-escaped on the wire.
    pub fn machine_lease_refusal_reason(error: &StoreError) -> String {
        error
            .trust_error()
            .map(|typed| typed.message)
            .unwrap_or_else(|| error.to_string())
    }

    pub fn authoritative_machine_lease(
        &self,
        node_id: &str,
    ) -> StoreResult<AuthorizedMachineLease> {
        match self.current_machine_lease(node_id)? {
            Some((lease, source)) if source.authorizes_provider_effect() => {
                Ok(AuthorizedMachineLease(lease))
            }
            // Both refusals carry the same typed code: from a fence's point of
            // view "the machine lease did not resolve to something that
            // authorizes" is one answer, and splitting it would make 46 call
            // sites decide which half they meant.
            Some((_, source)) => Err(machine_lease_unresolved(
                node_id,
                format!(
                    "{MACHINE_LEASE_NOT_AUTHORITATIVE}: Node {node_id} resolved from {} and cannot authorize a provider effect; this Store predates the machine lease cutover",
                    source.as_str()
                ),
            )),
            None => Err(machine_lease_unresolved(
                node_id,
                format!("{MACHINE_LEASE_NOT_AUTHORITATIVE}: Node {node_id} has no machine lease"),
            )),
        }
    }

    /// The node-file lease for call sites that carry their own refusal.
    ///
    /// `Ok(None)` means only one thing: this machine has no lease at all — the
    /// benign absence several callers already handle. A lease that resolves
    /// from a legacy Space row is an `Err`, never a `None`, because reading it
    /// as "no daemon here" is exactly how a pre-cutover row would quietly stop
    /// fencing anything.
    ///
    /// Callers that must have a lease use `authoritative_machine_lease` and get
    /// the `AuthorizedMachineLease` newtype instead.
    pub fn current_authorized_machine_lease(
        &self,
        node_id: &str,
    ) -> StoreResult<Option<NodeDaemonLease>> {
        match self.current_machine_lease(node_id)? {
            Some((lease, source)) if source.authorizes_provider_effect() => Ok(Some(lease)),
            Some((_, source)) => Err(machine_lease_unresolved(
                node_id,
                format!(
                    "{MACHINE_LEASE_NOT_AUTHORITATIVE}: Node {node_id} resolved from {} and cannot authorize a provider effect; this Store predates the machine lease cutover",
                    source.as_str()
                ),
            )),
            None => Ok(None),
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
        let document =
            publish_lease_document(&lock, &node_home, node_id, &clock, |current, now| {
                recorded_at = now;
                match next(current, now)? {
                    Some(lease) => Ok(NodeDaemonLeaseDocument::new(lease, std::process::id())),
                    None => {
                        let unchanged = current.cloned().ok_or_else(|| {
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

impl HarnessStore {
    /// Seed machine authority the way a post-cutover node actually carries it:
    /// the node-file document that every fence now reads, and — as far as it
    /// still can — the legacy Space row that E2b has not yet retired.
    ///
    /// **The document is acquired first, and it is the only half that may
    /// fail.** After cutover the daemon writes the document and nothing else:
    /// `release_node_authorities` publishes `Released` on the document and
    /// leaves the predecessor's Space row exactly where it was, because a
    /// second live authority record is the failure ADR 0075 removes. A fixture
    /// that drains through the daemon and then seeds a successor therefore
    /// meets a legacy row that is still `Active` and will never be released by
    /// anyone. Refusing there would fail a test for a record no fence reads.
    /// So the row is kept in step best-effort: it is a projection courtesy for
    /// the cutover window, not authority, and the two records are allowed to
    /// diverge exactly where production lets them.
    ///
    /// The legacy rows are read for one reason — the cutover mint. The first
    /// document on a node must start above every generation any Space ever
    /// issued (ADR 0075 Migration 3), or a successor could reuse a number a
    /// predecessor already drove under. Once the document exists its own
    /// generation is the only input and this argument is ignored.
    ///
    /// Un-gated for the same reason `append_mission` is (`store_store_base.rs`):
    /// integration tests under `tests/` link the non-test build, so a
    /// `cfg(test)` seeder is invisible to them. `#[doc(hidden)]` and the
    /// `_for_test` suffix carry the intent instead.
    #[doc(hidden)]
    pub fn seed_machine_authority_for_test(
        &self,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        let space_generations = self
            .latest_node_daemon_lease(node_id)?
            .map(|row| row.generation)
            .into_iter()
            .collect::<Vec<_>>();
        let document = self.acquire_machine_lease(
            node_id,
            daemon_id,
            instance_id,
            ttl_ms,
            &space_generations,
        )?;
        let _ =
            self.acquire_node_daemon_lease(node_id, daemon_id, instance_id, now_unix_ms, ttl_ms);
        Ok(document)
    }
}

impl HarnessStore {
    /// Drive one machine-authority transition across both records, the way a
    /// cutover-era daemon does.
    ///
    /// Lifecycle tests assert what a fence decides, and fences read the
    /// document — but the legacy row is still written until E2b, and a fixture
    /// whose two records disagreed would fail for reasons unrelated to its
    /// subject. These keep them in step so a test says what it means.
    #[doc(hidden)]
    pub fn drain_machine_authority_for_test(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
        drain_ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        let _ = self.drain_node_daemon_lease(
            node_id,
            daemon_id,
            generation,
            instance_id,
            now_unix_ms,
            drain_ttl_ms,
        );
        self.drain_machine_lease(node_id, daemon_id, generation, instance_id, drain_ttl_ms)
    }

    #[doc(hidden)]
    pub fn release_machine_authority_for_test(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        let _ = self.release_node_daemon_lease(
            node_id,
            daemon_id,
            generation,
            instance_id,
            now_unix_ms,
        );
        self.release_machine_lease(node_id, daemon_id, generation, instance_id)
    }
}

impl HarnessStore {
    /// Make the current machine lease look expired, the way wall-clock time
    /// does to a crashed daemon that stopped renewing.
    ///
    /// There is deliberately no production writer for this: `expires` is
    /// monotonic within a status, so nothing may shorten a live lease — that
    /// guard is what bounds a backwards clock step. A fixture that needs an
    /// expired predecessor is simulating *elapsed time*, not a write, so it
    /// writes the document directly rather than being handed an API that would
    /// undo the guard for everyone.
    ///
    /// Un-gated for the same reason as the seeders above: integration tests
    /// under `tests/` link the non-test build.
    #[doc(hidden)]
    pub fn expire_machine_lease_for_test(&self, node_id: &str) -> StoreResult<NodeDaemonLease> {
        use crate::node_lease_document::{lease_document_path, NodeDaemonLeaseDocument};
        let node_home = self.node_home(node_id)?;
        let mut document = read_lease_document(&node_home, node_id)?.ok_or_else(|| {
            StoreError::Conflict(format!("no machine lease to expire for Node {node_id}"))
        })?;
        let now = crate::store_node_runtime::current_store_unix_ms();
        document.lease.expires_unix_ms = now.saturating_sub(1);
        document.lease.renewed_unix_ms = document.lease.expires_unix_ms;
        let bytes = serde_json::to_vec(&NodeDaemonLeaseDocument::new(
            document.lease.clone(),
            document.owner_pid,
        ))?;
        std::fs::write(lease_document_path(&node_home), bytes)?;
        Ok(document.lease)
    }
}
