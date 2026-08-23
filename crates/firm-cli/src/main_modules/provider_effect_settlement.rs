use super::*;

pub(super) fn settle_provider_effect(
    ledger: &TeamRunLedger,
    admission: &ProviderEffectAdmission,
    applied: bool,
    result: Option<serde_json::Value>,
    failure_code: Option<String>,
) -> CliResult<()> {
    use harness_core::agentfirm_api::{
        RuntimeCommandStatus, RuntimeEffectCertainty, RuntimePostconditionStatus,
    };
    ledger
        .store
        .settle_runtime_command_with_postcondition(
            &admission.settle_context,
            &admission.command_id,
            if applied {
                RuntimeCommandStatus::Applied
            } else {
                RuntimeCommandStatus::RecoveryRequired
            },
            if applied {
                RuntimeEffectCertainty::Applied
            } else {
                RuntimeEffectCertainty::Unknown
            },
            if applied {
                RuntimePostconditionStatus::Satisfied
            } else {
                RuntimePostconditionStatus::Unknown
            },
            result,
            failure_code,
            &now_string(),
        )
        .map_err(|error| {
            CliError::RuntimeRecoveryRequired(format!(
                "runtime command {} settlement could not be proven: {error}",
                admission.command_id
            ))
        })
        .map(|_| ())
}

pub(super) fn settle_provider_effect_not_applied(
    ledger: &TeamRunLedger,
    admission: &ProviderEffectAdmission,
    failure_code: String,
) -> CliResult<()> {
    ledger
        .store
        .settle_runtime_command_with_postcondition(
            &admission.settle_context,
            &admission.command_id,
            harness_core::agentfirm_api::RuntimeCommandStatus::Failed,
            harness_core::agentfirm_api::RuntimeEffectCertainty::NotApplied,
            harness_core::agentfirm_api::RuntimePostconditionStatus::Unsatisfied,
            None,
            Some(failure_code),
            &now_string(),
        )
        .map_err(|error| {
            CliError::RuntimeRecoveryRequired(format!(
                "not-applied settlement for runtime command {} could not be proven: {error}",
                admission.command_id
            ))
        })
        .map(|_| ())
}
