//! CLI compatibility facade for the Codex provider package.

pub(crate) use harness_provider_codex::*;

impl From<harness_provider_codex::CodexError> for crate::CliError {
    fn from(error: harness_provider_codex::CodexError) -> Self {
        crate::CliError::Usage(error.to_string())
    }
}
