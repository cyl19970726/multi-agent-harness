//! The machine lease's own lock — a leaf, non-nesting lock (ADR 0075).
//!
//! The whole point of moving machine authority out of Execution Space data is
//! that a heartbeat must never queue behind a data write. That only holds if
//! the two locks are never held at once, in either order:
//!
//! 1. A thread holding the lease lock acquires **no other lock** and performs
//!    **no Space I/O**; it touches only the three files in the node directory.
//! 2. A thread holding a Space `.store.lock` **may not acquire** the lease
//!    lock; it reads the lease document lock-free.
//!
//! The lock graph then has no edges at all, so deadlock is impossible by
//! construction rather than by discipline. Discipline is what decays; a
//! structure that cannot express the bad state does not. The registry below
//! makes rule 1 and rule 2 enforced rather than merely written down: in debug
//! builds it panics the moment either order is attempted, naming both locks.

use super::*;

/// How long to wait for the lease lock.
///
/// Honest because of who the contenders are: this daemon's own renewal and a
/// rare operator verb, each holding the lock for one ~350-byte atomic replace.
/// A wait longer than this is not congestion, it is a stuck or dead holder,
/// and the caller should hear about it rather than keep queueing — the exact
/// failure mode that cost generation 4 the machine by two milliseconds.
pub(crate) fn lease_lock_timeout(ttl: Duration) -> Duration {
    (ttl / 4)
        .min(Duration::from_secs(1))
        .max(Duration::from_millis(50))
}

const LEASE_LOCK_POLL: Duration = Duration::from_millis(2);

/// `MACHINE_LEASE_LOCK_TIMEOUT` — the lease lock was not free in time.
pub const MACHINE_LEASE_LOCK_TIMEOUT: &str = "MACHINE_LEASE_LOCK_TIMEOUT";

thread_local! {
    /// How many lease locks and Space locks this thread holds. Debug-only:
    /// the registry exists to fail a test run loudly, not to police release
    /// builds, and it is per-thread because that is the scope a lock ordering
    /// violation actually lives in.
    static HELD_LEASE_LOCKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static HELD_SPACE_LOCKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Rule 2: refuse the lease lock to a thread already inside a Space write.
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
pub(crate) fn registry_enter_lease_lock(lock_path: &Path) {
    #[cfg(debug_assertions)]
    {
        assert!(
            HELD_SPACE_LOCKS.with(|held| held.get()) == 0,
            "ADR 0075 lock rule: {} was requested while this thread holds an Execution Space .store.lock; the machine lease lock is a leaf lock and the two are never held together",
            lock_path.display()
        );
        HELD_LEASE_LOCKS.with(|held| held.set(held.get() + 1));
    }
}

#[cfg_attr(not(debug_assertions), allow(unused))]
pub(crate) fn registry_exit_lease_lock() {
    #[cfg(debug_assertions)]
    HELD_LEASE_LOCKS.with(|held| held.set(held.get().saturating_sub(1)));
}

/// Rule 1: refuse a Space write lock to a thread holding the lease lock.
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
pub(crate) fn registry_enter_space_lock(lock_path: &Path) {
    #[cfg(debug_assertions)]
    {
        assert!(
            HELD_LEASE_LOCKS.with(|held| held.get()) == 0,
            "ADR 0075 lock rule: Execution Space lock {} was requested while this thread holds the machine lease lock; a lease-lock holder does no Space I/O",
            lock_path.display()
        );
        HELD_SPACE_LOCKS.with(|held| held.set(held.get() + 1));
    }
}

#[cfg_attr(not(debug_assertions), allow(unused))]
pub(crate) fn registry_exit_space_lock() {
    #[cfg(debug_assertions)]
    HELD_SPACE_LOCKS.with(|held| held.set(held.get().saturating_sub(1)));
}

/// `<node_home>/.node-daemon-lease.lock`.
pub(crate) fn lease_lock_path(node_home: &Path) -> PathBuf {
    node_home.join(".node-daemon-lease.lock")
}

/// Exclusive hold on one node's machine lease.
///
/// Held only across a single atomic replace of the lease document plus its one
/// history append — never across Space I/O, another lock, or a provider call.
pub(crate) struct NodeLeaseLock {
    file: File,
}

impl NodeLeaseLock {
    /// Take the lease lock, creating the node directory if this is the first
    /// daemon to run here.
    ///
    /// A `flock` held by a process that died is released by the kernel, so a
    /// crashed predecessor cannot wedge the machine — that is the reason to
    /// use `flock` here rather than a lock file whose liveness we would have
    /// to prove ourselves.
    pub(crate) fn acquire(node_home: &Path, timeout: Duration) -> StoreResult<Self> {
        fs::create_dir_all(node_home)?;
        let lock_path = lease_lock_path(node_home);
        registry_enter_lease_lock(&lock_path);
        match Self::acquire_locked(&lock_path, timeout) {
            Ok(lock) => Ok(lock),
            Err(error) => {
                registry_exit_lease_lock();
                Err(error)
            }
        }
    }

    fn acquire_locked(lock_path: &Path, timeout: Duration) -> StoreResult<Self> {
        let started = Instant::now();
        let deadline = started + timeout;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        loop {
            match lock_file_exclusive(&file) {
                Ok(()) => return Ok(Self { file }),
                Err(error) if would_block_lock(&error) => {
                    if Instant::now() >= deadline {
                        return Err(StoreError::LockTimeout(format!(
                            "{MACHINE_LEASE_LOCK_TIMEOUT}: {}; lock_wait_ms={}; lock_budget_ms={}",
                            lock_path.display(),
                            started.elapsed().as_millis(),
                            timeout.as_millis()
                        )));
                    }
                    thread::sleep(
                        LEASE_LOCK_POLL.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }
}

impl Drop for NodeLeaseLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
        registry_exit_lease_lock();
    }
}
