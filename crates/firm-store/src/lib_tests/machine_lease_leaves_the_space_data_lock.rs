//! The machine lease lives in its own file, under its own lock (ADR 0075).
//!
//! The property every test here defends is one sentence: **a data write can no
//! longer cost the machine its authority.** Generation 4 of the live dogfood
//! node lost it by queueing 14,996 ms on an Execution Space `.store.lock`
//! against a 14,994 ms budget — no authority check refused that renewal; it
//! simply never got to run. So the tests assert the absence of coupling, not
//! just the presence of a file.

use super::*;
use std::sync::atomic::Ordering;

const NODE: &str = "2437c3dd-0000-4000-8000-00000000e2a0";

fn machine_lease_store(label: &str) -> (PathBuf, HarnessStore) {
    let firm_home = team_test_firm_home(label);
    let root = firm_home.join("execution-spaces").join("space");
    let store = HarnessStore::new(&root);
    store.init().expect("initialize store");
    (firm_home, store)
}

fn acquire(store: &HarnessStore, daemon: &str, instance: &str, ttl_ms: u64) -> NodeDaemonLease {
    store
        .acquire_machine_lease(NODE, daemon, instance, ttl_ms, &[])
        .expect("acquire the machine lease")
}

/// The reason this ADR exists. A writer holds the Space data lock for longer
/// than the lease TTL — the shape of an 8.75 MB trust-journal rewrite — while
/// the machine lease is renewed throughout. Under the old design every renewal
/// queued behind that lock and the machine was lost; here the two never meet.
#[test]
fn a_held_space_lock_cannot_cost_the_machine_its_authority() {
    let (_home, store) = machine_lease_store("gen-4-regression");
    let ttl_ms = 500;

    // Hold the Space data lock for well past the lease TTL.
    let holder = store.clone();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let _data_lock = holder
            .acquire_write_lock()
            .expect("hold the Space data lock");
        locked_tx.send(()).expect("announce the held lock");
        done_rx.recv_timeout(Duration::from_secs(10)).ok();
    });
    locked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the Space data lock is held");

    // Acquired *after* the data lock is held, which is the stronger claim: even
    // taking the machine does not queue behind a data write. The TTL is large
    // enough that fixture setup cannot consume it — a 50 ms TTL would race its
    // own scaffolding, which is exactly how #990 was written.
    let lease = acquire(&store, "node-daemon:gen4", "instance-1", ttl_ms);

    // Renew across three full TTLs while that lock is held.
    let mut renewals = 0;
    let deadline = Instant::now() + Duration::from_millis(ttl_ms * 3);
    while Instant::now() < deadline {
        store
            .renew_machine_lease(
                NODE,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                ttl_ms,
            )
            .unwrap_or_else(|error| {
                panic!("renewal {renewals} queued behind a data write: {error}")
            });
        renewals += 1;
        std::thread::sleep(Duration::from_millis(ttl_ms / 4));
    }
    assert!(renewals >= 3, "expected repeated renewals, got {renewals}");

    let (current, source) = store
        .current_machine_lease(NODE)
        .expect("resolve")
        .expect("a machine lease");
    assert_eq!(source, MachineLeaseSource::NodeFile);
    assert_eq!(current.status, NodeDaemonLeaseStatus::Active);
    assert_eq!(current.generation, lease.generation);

    done_tx.send(()).ok();
    writer.join().expect("writer thread");
}

/// Atomic replace, stated as the property a lock-free reader depends on: at no
/// point does a reader see a partial document. The tmp file is proof the write
/// staged somewhere else first.
#[test]
fn a_reader_never_sees_a_half_written_document() {
    let (home, store) = machine_lease_store("atomic-replace");
    let lease = acquire(&store, "node-daemon:atomic", "instance-1", 60_000);
    let document = home.join("nodes").join(NODE).join("node-daemon-lease.json");
    assert!(
        document.exists(),
        "the lease document is where node_home says"
    );
    assert!(
        !home
            .join("nodes")
            .join(NODE)
            .join("node-daemon-lease.json.tmp")
            .exists(),
        "the staging file is renamed away, never left behind"
    );

    let reader = store.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let watcher = std::thread::spawn(move || {
        let mut reads = 0;
        while !flag.load(Ordering::SeqCst) {
            // Every read either decodes or the file is absent; a torn read
            // would surface here as a decode error.
            reader.current_machine_lease(NODE).expect("lock-free read");
            reads += 1;
        }
        reads
    });
    for _ in 0..40 {
        store
            .renew_machine_lease(
                NODE,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                60_000,
            )
            .expect("renew");
    }
    stop.store(true, Ordering::SeqCst);
    let reads = watcher.join().expect("reader thread");
    assert!(reads > 0, "the reader observed at least one document");
}

/// Authority only moves forward. A resurrected predecessor or a backwards
/// system-clock step is refused by name rather than clamped, because a lease
/// that silently shrinks is one whose holder believes it owns more time than
/// the file grants.
#[test]
fn a_write_that_would_move_authority_backwards_is_refused() {
    let (home, store) = machine_lease_store("forward-only");
    let first = acquire(&store, "node-daemon:fwd", "instance-1", 60_000);
    store
        .release_machine_lease(NODE, &first.daemon_id, first.generation, &first.instance_id)
        .expect("release");
    let second = acquire(&store, "node-daemon:fwd", "instance-2", 60_000);
    assert_eq!(second.generation, first.generation + 1);

    // Hand-write a lower generation straight into the document, the way a
    // restored backup or a resurrected predecessor would.
    let path = home.join("nodes").join(NODE).join("node-daemon-lease.json");
    let raw = std::fs::read_to_string(&path).expect("read document");
    let rolled_back = raw.replace(
        &format!("\"generation\":{}", second.generation),
        &format!("\"generation\":{}", first.generation),
    );
    std::fs::write(&path, rolled_back).expect("plant a stale document");
    let error = store
        .renew_machine_lease(
            NODE,
            &second.daemon_id,
            second.generation,
            &second.instance_id,
            60_000,
        )
        .expect_err("a stale document cannot be renewed as the current generation");
    assert!(
        error.to_string().contains("NODE_DAEMON_GENERATION_FENCED"),
        "{error}"
    );
}

/// The history file exists for exactly one reader: reattach, which asks whether
/// a *named past generation* reached Released, because expiry is not a
/// provider-drain receipt. One row per generation transition, never per
/// renewal — the property that keeps it ~28 KB instead of 28 MB.
#[test]
fn the_history_records_one_row_per_generation_transition() {
    let (home, store) = machine_lease_store("generation-history");
    let first = acquire(&store, "node-daemon:hist", "instance-1", 60_000);
    for _ in 0..25 {
        store
            .renew_machine_lease(
                NODE,
                &first.daemon_id,
                first.generation,
                &first.instance_id,
                60_000,
            )
            .expect("renew");
    }
    store
        .drain_machine_lease(
            NODE,
            &first.daemon_id,
            first.generation,
            &first.instance_id,
            60_000,
        )
        .expect("drain");
    store
        .release_machine_lease(NODE, &first.daemon_id, first.generation, &first.instance_id)
        .expect("release");
    let second = acquire(&store, "node-daemon:hist", "instance-2", 60_000);

    let history = home
        .join("nodes")
        .join(NODE)
        .join("node-daemon-lease-history.jsonl");
    let rows = std::fs::read_to_string(&history).expect("history file");
    let count = rows.lines().filter(|line| !line.trim().is_empty()).count();
    assert_eq!(
        count, 4,
        "acquired/drained/released/acquired — 25 renewals add nothing:\n{rows}"
    );

    assert!(
        store
            .machine_generation_was_released(NODE, &first.daemon_id, first.generation)
            .expect("history read"),
        "the predecessor's Released record survives its successor's acquire"
    );
    assert!(
        !store
            .machine_generation_was_released(NODE, &second.daemon_id, second.generation)
            .expect("history read"),
        "a live generation has not been released"
    );
    assert!(
        !store
            .machine_generation_was_released(NODE, "node-daemon:hist", 99)
            .expect("history read"),
        "a generation with no record fails closed, never open"
    );
}

/// Only the node file authorizes. A pre-cutover store still resolves its legacy
/// row — `daemon status` should show it — but a fence built on it is refused by
/// name, so the cutover cannot half-happen.
#[test]
fn only_the_node_file_authorizes_a_provider_effect() {
    let (_home, store) = machine_lease_store("legacy-source");
    store
        .insert_execution_node(&ExecutionNode {
            id: NODE.into(),
            display_name: "legacy".into(),
            status: ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        })
        .expect("insert Node");
    store
        .acquire_node_daemon_lease(NODE, "node-daemon:legacy", "instance-1", 1, 60_000)
        .expect("a pre-cutover Space row");

    let (lease, source) = store
        .current_machine_lease(NODE)
        .expect("resolve")
        .expect("the legacy row is still readable");
    assert_eq!(source, MachineLeaseSource::LegacySpaceRow);
    assert_eq!(lease.daemon_id, "node-daemon:legacy");
    assert!(!source.authorizes_provider_effect());

    let error = store
        .authoritative_machine_lease(NODE)
        .expect_err("a legacy row cannot authorize an effect");
    assert!(
        error.to_string().contains(MACHINE_LEASE_NOT_AUTHORITATIVE),
        "{error}"
    );

    // Once the document exists it wins, and the legacy row is left untouched.
    acquire(&store, "node-daemon:cutover", "instance-1", 60_000);
    let (lease, source) = store
        .current_machine_lease(NODE)
        .expect("resolve")
        .expect("a machine lease");
    assert_eq!(source, MachineLeaseSource::NodeFile);
    assert_eq!(lease.daemon_id, "node-daemon:cutover");
    assert_eq!(
        store
            .latest_node_daemon_lease(NODE)
            .expect("legacy read")
            .expect("row")
            .daemon_id,
        "node-daemon:legacy",
        "E2b retires the Space writers; E2a leaves their rows alone"
    );
}

/// The cutover's generation rule: the first document starts above every
/// generation any Space ever issued, so a successor cannot reuse a number a
/// predecessor already drove under.
#[test]
fn the_first_document_starts_above_every_space_generation() {
    let (_home, store) = machine_lease_store("cutover-generation");
    let lease = store
        .acquire_machine_lease(NODE, "node-daemon:cut", "instance-1", 60_000, &[7, 148, 12])
        .expect("acquire at cutover");
    assert_eq!(lease.generation, 149, "max across Spaces + 1");
}

/// A Store that cannot name its node home fails every machine-authority read
/// closed. "I do not know who owns this machine" is never "nobody does".
#[test]
fn an_unbound_store_fails_machine_authority_closed() {
    let store = HarnessStore::new(team_test_firm_home("machine-lease-unbound"));
    for error in [
        store.current_machine_lease(NODE).expect_err("resolve"),
        store.authoritative_machine_lease(NODE).expect_err("fence"),
        store
            .acquire_machine_lease(NODE, "d", "i", 60_000, &[])
            .expect_err("acquire"),
    ] {
        assert!(error.is_machine_lease_unresolved(), "{error}");
    }
}

/// Rule 1 of the lock model, enforced rather than reviewed: a thread holding
/// the lease lock may not take a Space lock. The registry panics on the
/// attempt, so the two-lock graph stays acyclic by construction.
#[test]
#[should_panic(expected = "ADR 0075 lock rule")]
fn taking_a_space_lock_under_the_lease_lock_panics() {
    let (home, store) = machine_lease_store("lock-registry");
    let node_home = home.join("nodes").join(NODE);
    std::fs::create_dir_all(&node_home).expect("node home");
    let _lease_lock =
        crate::node_lease_lock::NodeLeaseLock::acquire(&node_home, Duration::from_millis(500))
            .expect("take the lease lock");
    let _ = store.acquire_write_lock();
}

/// The time-sampling rule, made assertable.
///
/// ADR 0075 requires `now` to be sampled **after** the lease lock is held and
/// immediately before the rename, so a writer that waited for the lock cannot
/// carry a stale timestamp past its own expiry. Wall-clock tests cannot express
/// that: the store floors every lease at `now + 1` (`ttl_ms.max(1)`), so no TTL
/// value can produce an already-expired lease and the clock is the only lever —
/// the same arithmetic that made a `Some(0)` TTL a no-op for #990.
///
/// Here the injected clock returns a time far later than the caller's, standing
/// in for a long wait, and the document must carry the late sample.
#[test]
fn the_written_lease_carries_the_clock_sampled_under_the_lock() {
    use crate::node_lease_document::{
        publish_lease_document, read_lease_document, LeaseClock, NodeDaemonLeaseDocument,
    };
    use crate::node_lease_lock::NodeLeaseLock;

    let (home, _store) = machine_lease_store("clock-sample");
    let node_home = home.join("nodes").join(NODE);
    std::fs::create_dir_all(&node_home).expect("node home");

    let stale_now = 1_000_000_u64;
    let after_a_long_wait = stale_now + 60_000;
    let samples = std::cell::Cell::new(0);
    let source = || {
        samples.set(samples.get() + 1);
        after_a_long_wait
    };
    let clock = LeaseClock::new(&source);

    let lock = NodeLeaseLock::acquire(&node_home, Duration::from_millis(500)).expect("lease lock");
    let published = publish_lease_document(&lock, &node_home, NODE, &clock, |now| {
        assert_eq!(
            now, after_a_long_wait,
            "the writer samples the clock, not the caller"
        );
        Ok(NodeDaemonLeaseDocument::new(
            NodeDaemonLease {
                node_id: NODE.into(),
                daemon_id: "node-daemon:clock".into(),
                generation: 1,
                instance_id: "instance-1".into(),
                status: NodeDaemonLeaseStatus::Active,
                acquired_unix_ms: now,
                renewed_unix_ms: now,
                expires_unix_ms: now.saturating_add(15_000),
                released_unix_ms: None,
            },
            std::process::id(),
        ))
    })
    .expect("publish");
    drop(lock);

    assert_eq!(samples.get(), 1, "sampled exactly once, inside the lock");
    assert_eq!(published.lease.acquired_unix_ms, after_a_long_wait);
    assert_eq!(
        published.lease.expires_unix_ms,
        after_a_long_wait + 15_000,
        "the lease is dated from the late sample, so a queued writer cannot \
         publish a window that has already passed"
    );
    let on_disk = read_lease_document(&node_home, NODE)
        .expect("read back")
        .expect("a document");
    assert_eq!(
        on_disk.lease.expires_unix_ms,
        published.lease.expires_unix_ms
    );
}
