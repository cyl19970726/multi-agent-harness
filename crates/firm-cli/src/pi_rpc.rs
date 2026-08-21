//! Pi persistent runtime composition facade.
//!
//! The RPC transport/session owner and the provider-neutral Team runtime
//! binding are separate modules; this facade preserves existing crate paths.

pub(crate) use harness_provider_pi::{
    confirm_pi_session_flush, PiRpcClient, PiSpawnOptions, HANDSHAKE_TIMEOUT,
};

pub(crate) fn callback_error(error: crate::CliError) -> harness_provider_pi::PiError {
    harness_provider_pi::PiError::Callback {
        supervisor_lease_lost: error.is_supervisor_lease_lost(),
        detail: error.to_string(),
    }
}

impl From<harness_provider_pi::PiError> for crate::CliError {
    fn from(error: harness_provider_pi::PiError) -> Self {
        match error {
            harness_provider_pi::PiError::Callback {
                detail,
                supervisor_lease_lost: true,
            } => crate::CliError::SupervisorLeaseLost(detail),
            other => crate::CliError::Usage(other.to_string()),
        }
    }
}
mod team_runtime;
pub(crate) use team_runtime::PiTeamRuntime;
