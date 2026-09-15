//! One process-wide lock for tests that touch the shared live-turn registry.
//!
//! `harness_runtime_host`'s authority-loss registry is process-global, so a
//! `Process`-scoped fan-out reaches every live turn in the test binary, not
//! just the one its own test registered. CI runs `cargo test -- --test-threads=1`
//! and is therefore safe, but a plain local `cargo test` is not: a test that
//! fans out could mark a turn another test is driving, and a test that counts
//! live turns could see someone else's.
//!
//! Every test that registers a live provider turn or fans an authority-loss
//! interrupt out holds this guard for its whole body.

use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) fn serialized_live_provider_turns() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}
