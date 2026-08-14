//! Non-authoritative wake hint for same-machine supervisor loops.
//!
//! The durable store commit is the only fact. This module is the
//! "accelerator" half of event-driven wake: writers bump a tiny hint file
//! after wake-relevant commits (new canonical message, new queued work
//! delivery, new member close latch); idle supervisor loops re-read the
//! file between bounded sleep chunks and re-scan the store early when the
//! content changed.
//!
//! A lost, stale, or never-written hint changes nothing: loops still
//! reconcile on their normal backoff schedule, which remains the
//! authoritative fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hint file name inside the store root.
pub const WAKE_HINT_FILE: &str = "wake_hint.json";

/// Path of the wake hint file for a store root.
pub fn wake_hint_path(root: &Path) -> PathBuf {
    root.join(WAKE_HINT_FILE)
}

static HINT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Best-effort bump of the wake hint. Never fails the caller: the hint is
/// an accelerator only, and backoff reconciliation remains authoritative.
///
/// `source` should be a short static identifier of the commit kind
/// (for example `"message"` or `"member_close"`); it is diagnostic only.
pub fn bump_wake_hint(root: &Path, source: &str) {
    let _ = bump_wake_hint_result(root, source);
}

fn bump_wake_hint_result(root: &Path, source: &str) -> std::io::Result<()> {
    let at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = HINT_SEQ.fetch_add(1, Ordering::Relaxed);
    // Content uniqueness is what readers compare; timestamp + per-process
    // counter + pid is enough for a hint. `source` is restricted to simple
    // static identifiers (no quotes/backslashes) by convention.
    let body = format!(
        "{{\"at_unix_ms\":{at_unix_ms},\"seq\":{seq},\"pid\":{},\"source\":\"{source}\"}}",
        std::process::id()
    );
    let tmp = root.join(format!(".wake_hint.{}.tmp", std::process::id()));
    fs::write(&tmp, body)?;
    fs::rename(&tmp, wake_hint_path(root))?;
    Ok(())
}

/// Whether `aggregate_kind`/`transition` of a trust-kernel commit is
/// wake-relevant: an idle supervisor loop may have new claimable work or
/// mail afterwards. Claim/receipt/cursor transitions are deliberately
/// excluded — they are read-side bookkeeping by the loops themselves.
pub(crate) fn wake_relevant_commit(aggregate_kind: &str, transition: &str) -> bool {
    match aggregate_kind {
        // New canonical Message plus its queued per-recipient deliveries
        // (covers both local authoring and cross-node remote persist).
        "message" => true,
        // Canonical WorkDelivery created alongside a fresh binding.
        "work_execution_binding" => transition == "bound",
        // Legacy per-member-run WorkDeliveries created in a batch.
        "work_event_delivery_batch" => transition == "deliveries_created",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_creates_and_changes_hint_file() {
        let dir = std::env::temp_dir().join(format!(
            "wake-hint-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = wake_hint_path(&dir);
        assert!(!path.exists());

        bump_wake_hint(&dir, "message");
        let first = fs::read_to_string(&path).unwrap();
        assert!(first.contains("\"source\":\"message\""));

        bump_wake_hint(&dir, "work_execution_binding");
        let second = fs::read_to_string(&path).unwrap();
        assert_ne!(first, second, "each bump must change the hint content");
        assert!(second.contains("\"source\":\"work_execution_binding\""));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bump_on_missing_root_is_swallowed() {
        let missing = Path::new("/definitely/not/a/real/store/root");
        // Must not panic; the hint is best-effort.
        bump_wake_hint(missing, "message");
    }

    #[test]
    fn wake_relevance_allowlist() {
        assert!(wake_relevant_commit("message", "authored"));
        assert!(wake_relevant_commit("message", "persisted"));
        assert!(wake_relevant_commit("work_execution_binding", "bound"));
        assert!(wake_relevant_commit(
            "work_event_delivery_batch",
            "deliveries_created"
        ));
        assert!(!wake_relevant_commit("work_execution_binding", "released"));
        assert!(!wake_relevant_commit("message_delivery", "claimed"));
        assert!(!wake_relevant_commit("work_delivery", "retried"));
        assert!(!wake_relevant_commit("subscription_cursor", "advanced"));
    }
}
