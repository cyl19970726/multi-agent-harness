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
    store: &HarnessStore,
    _ledger: &std::sync::Arc<crate::TeamRunLedger>,
    _objective: &str,
    _config: &HostDispatchConfig,
) -> Result<DispatchOutcome, crate::CliError> {
    // Stub: minimal dispatch scaffold (issue #387).
    let now = crate::now_string();

    // P1-2: detect review timeouts and generate HostAttention rows.
    let timeouts = match store.detect_review_timeouts(&now) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("host-dispatcher: review timeout detection failed: {error}");
            vec![]
        }
    };
    let timeout_count = timeouts.len();

    Ok(DispatchOutcome {
        inspected: timeout_count,
        handled: vec![],
        escalated: vec![],
        failed: vec![],
    })
}
