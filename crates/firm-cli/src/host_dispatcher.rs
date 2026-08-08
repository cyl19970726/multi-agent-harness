//! Host attention dispatcher — polls the store for actionable HostAttention
//! records and dispatches them to the appropriate handler.
//!
//! Stub: full implementation tracked in #387.

use harness_core::HostDispatchConfig;
use harness_store::HarnessStore;

/// Outcome of a single dispatch poll.
#[derive(Debug)]
pub struct DispatchOutcome {
    pub inspected: usize,
    pub handled: Vec<String>,
    pub escalated: Vec<String>,
    pub failed: Vec<String>,
}

impl DispatchOutcome {
    pub fn is_noop(&self) -> bool {
        self.handled.is_empty() && self.escalated.is_empty() && self.failed.is_empty()
    }
}

/// Poll the store for actionable host attention records and dispatch them.
/// Returns a summary of what was done.
pub fn poll_and_dispatch(
    _store: &HarnessStore,
    _ledger: &std::sync::Arc<crate::TeamRunLedger>,
    _objective: &str,
    _config: &HostDispatchConfig,
) -> Result<DispatchOutcome, crate::CliError> {
    // Stub: no host attention dispatching yet.
    // Full implementation will read HostAttention from the store and
    // dispatch accordingly (#387).
    Ok(DispatchOutcome {
        inspected: 0,
        handled: vec![],
        escalated: vec![],
        failed: vec![],
    })
}
