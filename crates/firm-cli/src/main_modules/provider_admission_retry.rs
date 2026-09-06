use super::*;

/// Keep the only retryable admission failure typed. This classifier is used
/// exclusively before a RuntimeCommand exists; once a command is prepared,
/// effect certainty and reconciliation remain authoritative.
pub(super) fn classify_pre_effect_provider_admission_error(error: CliError) -> CliError {
    match error {
        CliError::Store(error @ harness_store::StoreError::LockTimeout(_)) => {
            CliError::ProviderAdmissionContention(error)
        }
        other => CliError::ProviderAdmissionRejected(other.to_string()),
    }
}

pub(super) fn retry_pre_effect_provider_admission<T>(
    revalidate: impl FnMut() -> CliResult<()>,
    operation: impl FnMut() -> CliResult<T>,
    wait: impl FnMut(Duration),
) -> CliResult<T> {
    harness_runtime_supervisor::policy::retry_pre_effect_admission(
        revalidate,
        operation,
        |error| matches!(error, CliError::ProviderAdmissionContention(_)),
        wait,
    )
}

/// Re-run only the zero-effect provider-process admission. Every attempt first
/// revalidates the exact Supervisor lease; the one-shot function then reloads
/// the canonical MemberRun generation, AgentSession and NodeDaemon lease.
pub(super) fn prepare_provider_process_effect_with_retry(
    ledger: &TeamRunLedger,
    member: &ProviderRuntimeProjection,
    transport_attempt: u64,
) -> CliResult<ProviderEffectAdmission> {
    retry_pre_effect_provider_admission(
        || ledger.require_supervisor_lease(),
        || prepare_provider_process_effect(ledger, member, transport_attempt),
        std::thread::sleep,
    )
}
