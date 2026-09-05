use super::*;

/// The three orthogonal settlement dimensions of one provider effect (D1).
/// The caller derives each dimension from typed evidence — the receipt's
/// certainty/postcondition and the correlated cycle outcome — and states
/// them explicitly; nothing here is derived from a bare `applied` boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderEffectSettlement {
    pub(crate) status: harness_core::agentfirm_api::RuntimeCommandStatus,
    pub(crate) certainty: harness_core::agentfirm_api::RuntimeEffectCertainty,
    pub(crate) postcondition: harness_core::agentfirm_api::RuntimePostconditionStatus,
}

impl ProviderEffectSettlement {
    /// The effect is proven applied and its postcondition is satisfied.
    pub(crate) const APPLIED_SATISFIED: Self = Self {
        status: harness_core::agentfirm_api::RuntimeCommandStatus::Applied,
        certainty: harness_core::agentfirm_api::RuntimeEffectCertainty::Applied,
        postcondition: harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
    };
    /// The effect may have crossed the provider boundary but its outcome is
    /// unproven: recovery is required and nothing is satisfied.
    pub(crate) const UNPROVEN: Self = Self {
        status: harness_core::agentfirm_api::RuntimeCommandStatus::RecoveryRequired,
        certainty: harness_core::agentfirm_api::RuntimeEffectCertainty::Unknown,
        postcondition: harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
    };
}

/// Free-constant aliases for the two canned settlements. Associated
/// constants cannot be `use`-imported, so files near the maintained-file
/// size gate import these short names instead of the full
/// `ProviderEffectSettlement::…` paths.
pub(crate) mod settlements {
    use super::ProviderEffectSettlement;

    pub(crate) const APPLIED_SATISFIED: ProviderEffectSettlement =
        ProviderEffectSettlement::APPLIED_SATISFIED;
    pub(crate) const UNPROVEN: ProviderEffectSettlement = ProviderEffectSettlement::UNPROVEN;
}

pub(crate) fn settle_prepared_runtime_command_recovery(
    store: &harness_store::HarnessStore,
    context: &harness_core::agentfirm_api::MutationContext,
    command_id: &str,
    failure_code: impl Into<String>,
) -> CliResult<()> {
    store
        .mark_prepared_runtime_command_recovery(
            context,
            command_id,
            failure_code.into(),
            &now_string(),
        )
        .map_err(|error| {
            CliError::RuntimeRecoveryRequired(format!(
                "prepared runtime command {command_id} could not enter recovery: {error}"
            ))
        })
        .map(|_| ())
}

pub(crate) fn settle_provider_effect(
    ledger: &TeamRunLedger,
    admission: &ProviderEffectAdmission,
    settlement: ProviderEffectSettlement,
    result: Option<serde_json::Value>,
    failure_code: Option<String>,
) -> CliResult<()> {
    ledger
        .store
        .settle_runtime_command_with_postcondition(
            &admission.settle_context,
            &admission.command_id,
            settlement.status,
            settlement.certainty,
            settlement.postcondition,
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

pub(crate) fn settle_provider_effect_not_applied(
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

pub(crate) fn record_provider_cycle_correlation(
    ledger: &TeamRunLedger,
    admission: &ProviderEffectAdmission,
    correlation: &harness_core::agentfirm_api::ProviderCycleCorrelation,
) -> CliResult<()> {
    let current = ledger
        .store
        .runtime_commands(&admission.settle_context.execution_space_id)?
        .into_iter()
        .find(|command| command.id == admission.command_id)
        .ok_or_else(|| {
            CliError::RuntimeRecoveryRequired(format!(
                "provider cycle command {} disappeared before terminal correlation",
                admission.command_id
            ))
        })?;
    let mut context = admission.settle_context.clone();
    context.command_name = "node_daemon.provider_cycle.correlate".into();
    context.idempotency_key = format!("{}:terminal", admission.command_id);
    context.expected_version = current.version;
    context.request_fingerprint = None;
    ledger
        .store
        .record_runtime_cycle_correlation(
            &context,
            &admission.command_id,
            correlation,
            &now_string(),
        )
        .map_err(|error| {
            CliError::RuntimeRecoveryRequired(format!(
                "provider cycle {} terminal correlation failed closed: {error}",
                admission.command_id
            ))
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D1: the settlement dimensions are stated explicitly and stay
    /// orthogonal. A settlement carrying Unknown certainty settles as
    /// not-Applied on every axis; a satisfied settlement settles Applied on
    /// every axis — and the two never share a dimension value, so no single
    /// input can silently steer the other two.
    #[test]
    fn provider_effect_settlement_dimensions_are_explicit() {
        use harness_core::agentfirm_api::{
            RuntimeCommandStatus, RuntimeEffectCertainty, RuntimePostconditionStatus,
        };
        let applied = ProviderEffectSettlement::APPLIED_SATISFIED;
        assert_eq!(applied.status, RuntimeCommandStatus::Applied);
        assert_eq!(applied.certainty, RuntimeEffectCertainty::Applied);
        assert_eq!(applied.postcondition, RuntimePostconditionStatus::Satisfied);

        let unproven = ProviderEffectSettlement::UNPROVEN;
        assert_ne!(unproven.status, RuntimeCommandStatus::Applied);
        assert_ne!(unproven.certainty, RuntimeEffectCertainty::Applied);
        assert_ne!(
            unproven.postcondition,
            RuntimePostconditionStatus::Satisfied
        );
        assert_eq!(unproven.status, RuntimeCommandStatus::RecoveryRequired);
        assert_eq!(unproven.certainty, RuntimeEffectCertainty::Unknown);
        assert_eq!(unproven.postcondition, RuntimePostconditionStatus::Unknown);

        // The dimensions are independent: mixing one UNPROVEN axis with the
        // APPLIED_SATISFIED axes yields a different, also-expressible
        // settlement — the type does not collapse them to one flag.
        let mixed = ProviderEffectSettlement {
            postcondition: unproven.postcondition,
            ..applied
        };
        assert_ne!(mixed, applied);
        assert_ne!(mixed, unproven);
        assert_eq!(mixed.status, RuntimeCommandStatus::Applied);
        assert_eq!(mixed.postcondition, RuntimePostconditionStatus::Unknown);
    }
}
