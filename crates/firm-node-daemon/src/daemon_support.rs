//! Neutral coordination helpers shared by daemon and CLI.
use crate::daemon_error::{DaemonError as CliError, DaemonResult as CliResult};
use harness_core::{AgentTeamRun, ProviderRuntimeProjection};
use harness_store::HarnessStore;
use std::path::{Path, PathBuf};
pub fn canonicalize_best_effort(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}
pub fn latest_team_run(store: &HarnessStore, id: &str) -> CliResult<AgentTeamRun> {
    store
        .latest_team_runs()?
        .into_iter()
        .find(|run| run.id == id)
        .ok_or_else(|| CliError::Usage(format!("team run not found: {id}")))
}
pub fn is_unclosed_managed_member(member: &ProviderRuntimeProjection, team_run_id: &str) -> bool {
    member.team_run_id == team_run_id
        && !member.is_external_interactive()
        && member.coordination_is_active()
}
/// Evidence-ref prefix that binds a durable adoption outcome to the exact
/// canonical state it was observed under.
pub const CANONICAL_STATE_EVIDENCE_PREFIX: &str = "team-run-canonical-state:";
pub fn canonical_state_evidence_ref(fingerprint: &str) -> String {
    format!("{CANONICAL_STATE_EVIDENCE_PREFIX}{fingerprint}")
}
pub fn canonical_state_from_evidence(evidence_refs: &[String]) -> Option<&str> {
    evidence_refs
        .iter()
        .find_map(|reference| reference.strip_prefix(CANONICAL_STATE_EVIDENCE_PREFIX))
}

use std::time::{SystemTime, UNIX_EPOCH};
pub fn current_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn now_string() -> String {
    let millis = current_unix_ms();
    format!("unix-ms:{millis}")
}
pub fn current_unix_ms_u64() -> u64 {
    current_unix_ms().min(u64::MAX as u128) as u64
}
