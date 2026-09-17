use super::*;

/// Human-readable provider compatibility note keyed by (provider, version,
/// status). Extracted from `team_provider_profiles.rs` to keep that file under
/// the maintained-file size ceiling; behavior is unchanged.
pub(super) fn provider_compatibility_note(
    provider: &str,
    version: Option<&str>,
    status: ProviderCompatibilityStatus,
) -> String {
    match (provider, version, status) {
        ("kimi", Some("0.31.0" | "0.31.1"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.31.x is adapter-reviewed for persistent ACP prompt \
             delivery, model/reasoning-effort selection, native-session resume, \
             next-round batched mail, and cooperative Interrupt through the ACP \
             session/cancel notification."
                .to_string()
        }
        ("kimi", Some("0.36.1"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.36.1 is adapter-reviewed for persistent ACP prompt \
             delivery, K3/max selection, same-session resume with attach replay \
             drained, next-round batched mail, and cooperative Interrupt through \
             the ACP session/cancel notification."
                .to_string()
        }
        ("kimi", Some("0.41.0"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.41.0 is adapter-reviewed on kimi-acp-v1 (no protocol change \
             from 0.39.0). A 2026-09-17 live canary on native session \
             session_765b4654 exercised session/new, exact same-session resume, two \
             K3/max prompt rounds to end_turn, the permission allow handshake, and \
             narrow runtime Close through session/close plus clean owned-process \
             reap. Cooperative Interrupt through the session/cancel notification is \
             live-verified by the team_run_api cancel/close suite re-run against 0.41.0 \
             (serial). Ordinary mail remains next-round batched."
                .to_string()
        }
        ("kimi", Some("0.39.0"), ProviderCompatibilityStatus::Current) => {
            "Kimi Code 0.39.0 is adapter-reviewed for persistent ACP prompt \
             delivery, K3/max selection, exact same-session resume with attach \
             replay drained, next-round batched mail, cooperative Interrupt \
             through session/cancel, and narrow runtime Close through \
             session/close plus clean owned-process reap."
                .to_string()
        }
        ("codex", Some("0.148.0-alpha.9"), ProviderCompatibilityStatus::Current) => {
            "Codex 0.148.0-alpha.9 is adapter-reviewed for persistent app-server \
             thread open/resume, effective sandbox and approval-policy receipts, \
             completed rounds, current-turn interrupt, explicit runtime Close, and \
             exact same-thread Reopen. Native Goal supervision and live steer remain \
             review-required capability slices."
                .to_string()
        }
        (_, _, ProviderCompatibilityStatus::Current) => "Installed provider version matches an adapter-reviewed version.".to_string(),
        (_, _, ProviderCompatibilityStatus::ReviewRequired) => "Installed provider version has not been reviewed against this adapter contract; regenerate protocol schemas and run provider acceptance before promotion.".to_string(),
        (_, _, ProviderCompatibilityStatus::Unavailable) => "Provider version could not be detected.".to_string(),
        (_, _, ProviderCompatibilityStatus::Incompatible) => "Provider version is known to be incompatible with this adapter contract.".to_string(),
        (_, _, ProviderCompatibilityStatus::Unknown) => "No reviewed provider version is registered for this execution mode.".to_string(),
    }
}
