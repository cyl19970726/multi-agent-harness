//! CLI compatibility facade for the Codex provider package.

pub(crate) use harness_provider_codex::*;

pub(crate) fn provider_error(error: harness_provider_codex::CodexError) -> crate::CliError {
    match error {
        harness_provider_codex::CodexError::ProcessGroupAdmissionClosed(error) => {
            crate::CliError::ProviderProcessAdmissionClosed(error)
        }
        harness_provider_codex::CodexError::Callback {
            detail,
            supervisor_lease_lost: true,
        } => crate::CliError::SupervisorLeaseLost(detail),
        other => crate::CliError::Usage(other.to_string()),
    }
}
