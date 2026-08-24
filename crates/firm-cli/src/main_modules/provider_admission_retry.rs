use super::*;

pub(super) const MAX_PRE_EFFECT_PROVIDER_ADMISSION_RETRIES: u32 = 3;

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
    mut revalidate: impl FnMut() -> CliResult<()>,
    mut operation: impl FnMut() -> CliResult<T>,
    mut wait: impl FnMut(Duration),
) -> CliResult<T> {
    let mut retries = 0u32;
    loop {
        revalidate()?;
        match operation() {
            Err(CliError::ProviderAdmissionContention(_))
                if retries < MAX_PRE_EFFECT_PROVIDER_ADMISSION_RETRIES =>
            {
                let delay_ms = 50u64.saturating_mul(1u64 << retries.min(3));
                retries += 1;
                wait(Duration::from_millis(delay_ms));
            }
            result => return result,
        }
    }
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
