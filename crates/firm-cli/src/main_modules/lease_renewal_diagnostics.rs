//! Volatile heartbeat diagnostics. Never authorizes an effect or renews a lease.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn entries() -> &'static Mutex<BTreeMap<String, serde_json::Value>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<String, serde_json::Value>>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}

pub(crate) fn record(id: &str, kind: &str, expires: u64, elapsed: Duration, error: Option<&str>) {
    let mut entries = entries().lock().unwrap_or_else(|error| error.into_inner());
    let previous = entries.get(id);
    let failure_count = previous
        .and_then(|v| v["failure_count"].as_u64())
        .unwrap_or(0)
        + u64::from(error.is_some());
    let last_error = error.map(str::to_owned).or_else(|| {
        previous
            .and_then(|v| v["last_error"].as_str())
            .map(str::to_owned)
    });
    entries.insert(
        id.to_owned(),
        serde_json::json!({
            "id": id, "kind": kind, "confirmed_expires_unix_ms": expires,
            "attempt_elapsed_ms": elapsed.as_millis(), "failure_count": failure_count,
            "last_error": last_error, "retrying": error.is_some(),
            "observed_unix_ms": crate::current_unix_ms_u64(),
        }),
    );
}

pub(crate) fn snapshot() -> Vec<serde_json::Value> {
    entries()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .values()
        .cloned()
        .collect()
}
