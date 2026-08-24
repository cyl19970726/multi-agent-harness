//! CLI compatibility facade for the Kimi provider package.

pub(crate) use harness_provider_kimi::*;

impl From<harness_provider_kimi::KimiError> for crate::CliError {
    fn from(error: harness_provider_kimi::KimiError) -> Self {
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
}
