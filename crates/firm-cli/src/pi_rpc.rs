//! Pi persistent runtime composition facade.
//!
//! The RPC transport/session owner and the provider-neutral Team runtime
//! binding are separate modules; this facade preserves existing crate paths.

pub(crate) use harness_provider_pi::{PiRpcClient, PiSpawnOptions};

pub(crate) fn provider_error(error: harness_provider_pi::PiError) -> crate::CliError {
    match error {
        harness_provider_pi::PiError::ProcessGroupAdmissionClosed(error) => {
            crate::CliError::ProviderProcessAdmissionClosed(error)
        }
        harness_provider_pi::PiError::Callback {
            detail,
            supervisor_lease_lost: true,
        } => crate::CliError::SupervisorLeaseLost(detail),
        other => crate::CliError::Usage(other.to_string()),
    }
}
mod team_runtime;
pub(crate) use team_runtime::PiTeamRuntime;
