//! CLI compatibility facade for the Kimi provider package.

pub(crate) use harness_provider_kimi::*;

pub(crate) fn callback_error(error: crate::CliError) -> harness_provider_kimi::KimiError {
    harness_provider_kimi::KimiError::Callback {
        supervisor_lease_lost: error.is_supervisor_lease_lost(),
        detail: error.to_string(),
    }
}

impl From<harness_provider_kimi::KimiError> for crate::CliError {
    fn from(error: harness_provider_kimi::KimiError) -> Self {
        match error {
            harness_provider_kimi::KimiError::Callback {
                detail,
                supervisor_lease_lost: true,
            } => crate::CliError::SupervisorLeaseLost(detail),
            other => crate::CliError::Usage(other.to_string()),
        }
    }
}
