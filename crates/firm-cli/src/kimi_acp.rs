//! CLI compatibility facade for the Kimi provider package.

pub(crate) use harness_provider_kimi::*;

pub(crate) fn provider_error(error: harness_provider_kimi::KimiError) -> crate::CliError {
    match error {
        harness_provider_kimi::KimiError::ProcessGroupAdmissionClosed(error) => {
            crate::CliError::ProviderProcessAdmissionClosed(error)
        }
        harness_provider_kimi::KimiError::Callback {
            detail,
            supervisor_lease_lost: true,
        } => crate::CliError::SupervisorLeaseLost(detail),
        other => crate::CliError::Usage(other.to_string()),
    }
}
