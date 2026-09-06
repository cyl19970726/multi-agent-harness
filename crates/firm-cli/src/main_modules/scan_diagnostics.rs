//! Process-local scan cost. These observations never authorize runtime work.
use crate::HarnessStore;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn entries() -> &'static Mutex<BTreeMap<String, serde_json::Value>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<String, serde_json::Value>>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}

pub(crate) struct ScanObservation {
    key: String,
    store: HarnessStore,
    started: Instant,
    before: BTreeMap<String, (u64, u64, u64)>,
}
impl ScanObservation {
    pub(crate) fn begin(key: String, store: &HarnessStore) -> Self {
        Self {
            key,
            store: store.clone(),
            started: Instant::now(),
            before: store
                .read_scan_metrics()
                .into_iter()
                .map(|metric| {
                    (
                        metric.ledger,
                        (
                            metric.total_decoded_rows,
                            metric.total_bytes_read,
                            metric.total_cloned_rows,
                        ),
                    )
                })
                .collect(),
        }
    }
}
impl Drop for ScanObservation {
    fn drop(&mut self) {
        let ledgers = self
            .store
            .read_scan_metrics()
            .into_iter()
            .map(|metric| {
                let before = self.before.get(&metric.ledger).copied().unwrap_or_default();
                serde_json::json!({
                    "ledger": metric.ledger,
                    "ledger_bytes": metric.ledger_bytes,
                    "decoded_rows_during_pass": metric.total_decoded_rows.saturating_sub(before.0),
                    "bytes_read_during_pass": metric.total_bytes_read.saturating_sub(before.1),
                    "cloned_rows_during_pass": metric.total_cloned_rows.saturating_sub(before.2),
                    "last_read_reason": metric.reason,
                    "last_read_decoded_rows": metric.decoded_rows,
                })
            })
            .collect::<Vec<_>>();
        entries().lock().unwrap_or_else(|e| e.into_inner()).insert(self.key.clone(), serde_json::json!({
            "scope": self.key,
            "elapsed_us": self.started.elapsed().as_micros(),
            "observed_unix_ms": crate::current_unix_ms_u64(),
            "counter_scope": "store handle and clones; overlapping readers included",
            "coverage": "successful cache, full-history and tail reads; failed reads and metadata probes excluded",
            "ledgers": ledgers,
        }));
    }
}
pub(crate) fn snapshot() -> Vec<serde_json::Value> {
    entries()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}
