//! CLI compatibility facade for the Codex provider package.

pub(crate) use harness_provider_codex::*;

impl From<harness_provider_codex::CodexError> for crate::CliError {
    fn from(error: harness_provider_codex::CodexError) -> Self {
        match error {
            harness_provider_codex::CodexError::Callback {
                detail,
                supervisor_lease_lost: true,
            } => crate::CliError::SupervisorLeaseLost(detail),
            other => crate::CliError::Usage(other.to_string()),
        }
    }
}
