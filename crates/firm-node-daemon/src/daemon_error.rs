//! Finite shared daemon/application errors; preserve typed causes and Display.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error(transparent)]
    ProviderProcessAdmissionClosed(#[from] harness_runtime_host::ProcessGroupRegistrationError),
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    SupervisorLeaseLost(String),
    #[error("RUNTIME_COMMAND_RECOVERY_REQUIRED: {0}")]
    RuntimeRecoveryRequired(String),
    #[error("PROVIDER_ADMISSION_REJECTED_NO_EFFECT: {0}")]
    ProviderAdmissionRejected(String),
    #[error("PROVIDER_ADMISSION_CONTENTION_NO_EFFECT: {0}")]
    ProviderAdmissionContention(harness_store::StoreError),
    #[error("PROVIDER_EFFECT_ACCEPTED: {0}")]
    ProviderEffectAccepted(String),
    #[error("store error: {0}")]
    Store(#[from] harness_store::StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type DaemonResult<T> = Result<T, DaemonError>;

impl DaemonError {
    pub fn is_supervisor_lease_lost(&self) -> bool {
        matches!(self, Self::SupervisorLeaseLost(_))
    }

    pub fn is_provider_process_admission_closed(&self) -> bool {
        matches!(self, Self::ProviderProcessAdmissionClosed(_))
    }

    pub fn is_provider_compatibility_blocked(&self) -> bool {
        matches!(self, Self::Usage(message) if message.starts_with("PROVIDER_COMPATIBILITY_BLOCKED:"))
    }
}
