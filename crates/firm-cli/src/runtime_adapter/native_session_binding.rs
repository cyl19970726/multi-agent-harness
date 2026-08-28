//! Provider-native binding transitions that must commit before a cycle reaches
//! its terminal boundary.

use harness_core::ProviderRuntimeProjection;
use serde_json::Value;

use crate::{
    native_session_ref, refresh_member_after_provider_callbacks, CliError, CliResult, TeamRunLedger,
};

pub(super) fn persist_verified_claude_session_binding(
    ledger: &TeamRunLedger,
    round_start: &ProviderRuntimeProjection,
    event: &Value,
    native_locator_kind: &str,
) -> CliResult<ProviderRuntimeProjection> {
    let native_session_id = event
        .pointer("/data/sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::RuntimeRecoveryRequired(
                "verified session_bound omitted native session id".into(),
            )
        })?;
    let current = refresh_member_after_provider_callbacks(ledger, round_start)?;
    match current.native_session.as_ref() {
        Some(existing) if existing.native_session_id != native_session_id => {
            return Err(CliError::RuntimeRecoveryRequired(format!(
                "claude session_bound attempted to replace exact native session {} with {native_session_id}",
                existing.native_session_id
            )));
        }
        Some(_) => return Ok(current),
        None => {}
    }
    let mut bound = current.clone();
    bound.native_session = Some(native_session_ref(
        &current,
        native_session_id,
        native_locator_kind,
    ));
    ledger.save_member_run(&current, &bound)?;
    Ok(bound)
}
