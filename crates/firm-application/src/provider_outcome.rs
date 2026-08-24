use serde::{Deserialize, Serialize};

/// Whether a durable delivery may be selected for this scheduling pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum DeliveryEligibility {
    Ready,
    NotReady(DeliveryIneligibility),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryIneligibility {
    PrerequisitePending { work_id: String },
    PrerequisiteCancelled { work_id: String },
    AuthorityUnavailable { authority_ref: String },
}

/// Result of the authoritative admission immediately before a provider effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum AdmissionOutcome {
    Admitted { command_id: String },
    RejectedNoEffect(AdmissionRejection),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRejection {
    NotReady,
    AuthorityFenced,
    BindingFenced,
    PermissionDenied,
}

/// What is durably known about one provider effect.
///
/// `Unknown` is deliberately distinct from failure. Repeating an unknown
/// effect could duplicate provider input, so it always requires reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum ProviderEffectOutcome {
    NotApplied { reason: String },
    Accepted { receipt_id: String },
    Unknown { recovery_ref: String },
}

/// Provider cycle observation is not semantic Work completion or Host acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum CycleOutcome {
    Terminal { correlation_id: String },
    Interrupted { correlation_id: String },
    StillRunning,
    Unknown { recovery_ref: String },
}

/// Application-owned identity supplied around one provider-native cycle. The
/// provider adapter owns only its native input/terminal ids; RuntimeCommand,
/// delivery, session generation and transport attempt remain Harness facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCycleAuthority {
    pub invocation_id: String,
    pub source_delivery_id: Option<String>,
    pub native_session_id: String,
    pub agent_session_generation: u64,
    pub provider_attempt: u64,
}

/// Close the provider-native correlation against the exact application-owned
/// RuntimeCommand/session authority. A provider terminal is operational
/// evidence only and never becomes semantic Work completion here.
pub fn correlate_provider_cycle(
    authority: ProviderCycleAuthority,
    native: firm_runtime_contract::NativeCycleCorrelation,
    terminal_observed: bool,
    interrupted: bool,
) -> Result<
    (
        firm_core::agentfirm_api::ProviderCycleCorrelation,
        CycleOutcome,
    ),
    String,
> {
    let required = |value: &str, field: &str| {
        (!value.trim().is_empty())
            .then_some(())
            .ok_or_else(|| format!("PROVIDER_CYCLE_CORRELATION_MISSING: {field}"))
    };
    required(&authority.invocation_id, "invocation_id")?;
    required(&authority.native_session_id, "native_session_id")?;
    required(&native.provider_input_id, "provider_input_id")?;
    if authority.agent_session_generation == 0 || authority.provider_attempt == 0 {
        return Err(
            "PROVIDER_CYCLE_CORRELATION_INVALID: generations and attempts are one-based".into(),
        );
    }
    let receipt_id = native
        .input_acceptance_receipt
        .response_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "PROVIDER_CYCLE_CORRELATION_MISSING: input_acceptance_receipt".to_string()
        })?;
    if !native.input_acceptance_receipt.success {
        return Err("PROVIDER_CYCLE_CORRELATION_INVALID: input receipt was not successful".into());
    }
    if native
        .terminal_provider_input_id
        .as_deref()
        .is_some_and(|terminal| terminal != native.provider_input_id)
    {
        return Err(
            "PROVIDER_CYCLE_TERMINAL_MISMATCH: terminal belongs to another provider input".into(),
        );
    }
    if native
        .exact_terminal_ref
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err("PROVIDER_CYCLE_CORRELATION_INVALID: empty exact_terminal_ref".into());
    }

    let correlation = firm_core::agentfirm_api::ProviderCycleCorrelation {
        invocation_id: authority.invocation_id.clone(),
        source_delivery_id: authority.source_delivery_id,
        provider_input_id: native.provider_input_id,
        input_acceptance_receipt: receipt_id.to_string(),
        terminal_provider_input_id: native.terminal_provider_input_id,
        exact_terminal_ref: native.exact_terminal_ref,
        native_session_id: authority.native_session_id,
        agent_session_generation: authority.agent_session_generation,
        provider_attempt: authority.provider_attempt,
    };
    let outcome = if !terminal_observed {
        CycleOutcome::StillRunning
    } else if interrupted {
        CycleOutcome::Interrupted {
            correlation_id: authority.invocation_id,
        }
    } else {
        CycleOutcome::Terminal {
            correlation_id: authority.invocation_id,
        }
    };
    Ok((correlation, outcome))
}

/// Project durable cycle certainty without guessing from provider process
/// success. An accepted input with no correlated terminal is Unknown and must
/// never be automatically replayed.
pub fn durable_provider_cycle_outcome(
    command: &firm_core::agentfirm_api::RuntimeCommandRecord,
) -> CycleOutcome {
    use firm_core::agentfirm_api::{
        RuntimeCommandKind, RuntimeCommandStatus, RuntimeEffectCertainty,
    };
    if command.command != RuntimeCommandKind::StartCycle {
        return CycleOutcome::Unknown {
            recovery_ref: command.id.clone(),
        };
    }
    if command.cycle_correlation.is_some()
        && command.status == RuntimeCommandStatus::Applied
        && command.effect_certainty == RuntimeEffectCertainty::Applied
    {
        return CycleOutcome::Terminal {
            correlation_id: command.id.clone(),
        };
    }
    if command.status == RuntimeCommandStatus::Accepted {
        return CycleOutcome::StillRunning;
    }
    if command.status == RuntimeCommandStatus::Applied
        && command.effect_certainty == RuntimeEffectCertainty::Applied
    {
        return CycleOutcome::Unknown {
            recovery_ref: command.id.clone(),
        };
    }
    CycleOutcome::Unknown {
        recovery_ref: command.id.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum SemanticOutcome {
    Submitted { report_id: String },
    Accepted { report_id: String },
    Rejected { report_id: String },
    Unproven,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRetryAuthority {
    RetryWithNewAttempt { next_attempt: u64 },
    StopNoRetry,
    RequireReconciliation { recovery_ref: String },
}

/// The sole automatic provider retry policy.
///
/// Only a proven `NotApplied` effect may receive a fresh transport attempt.
/// Accepted and unknown effects are never automatically resent.
pub fn provider_retry_authority(
    outcome: &ProviderEffectOutcome,
    transport_attempt: u64,
    max_attempts: u64,
) -> ProviderRetryAuthority {
    match outcome {
        ProviderEffectOutcome::NotApplied { .. } if transport_attempt < max_attempts => {
            ProviderRetryAuthority::RetryWithNewAttempt {
                next_attempt: transport_attempt + 1,
            }
        }
        ProviderEffectOutcome::NotApplied { .. } | ProviderEffectOutcome::Accepted { .. } => {
            ProviderRetryAuthority::StopNoRetry
        }
        ProviderEffectOutcome::Unknown { recovery_ref } => {
            ProviderRetryAuthority::RequireReconciliation {
                recovery_ref: recovery_ref.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_cycle(
        terminal_provider_input_id: Option<&str>,
    ) -> firm_runtime_contract::NativeCycleCorrelation {
        firm_runtime_contract::NativeCycleCorrelation {
            provider_input_id: "provider-input:1".into(),
            input_acceptance_receipt: firm_runtime_contract::ControlTransportReceipt {
                command: "deliver".into(),
                response_id: Some("provider-receipt:1".into()),
                success: true,
            },
            terminal_provider_input_id: terminal_provider_input_id.map(str::to_string),
            exact_terminal_ref: Some("provider-terminal:1".into()),
        }
    }

    fn cycle_authority() -> ProviderCycleAuthority {
        ProviderCycleAuthority {
            invocation_id: "runtime-command:1".into(),
            source_delivery_id: Some("work-delivery:1".into()),
            native_session_id: "native-session:1".into(),
            agent_session_generation: 2,
            provider_attempt: 3,
        }
    }

    #[test]
    fn cycle_correlation_closes_exact_input_terminal_and_application_authority() {
        let (correlation, outcome) = correlate_provider_cycle(
            cycle_authority(),
            native_cycle(Some("provider-input:1")),
            true,
            false,
        )
        .unwrap();
        assert_eq!(correlation.invocation_id, "runtime-command:1");
        assert_eq!(correlation.provider_attempt, 3);
        assert_eq!(correlation.agent_session_generation, 2);
        assert_eq!(
            outcome,
            CycleOutcome::Terminal {
                correlation_id: "runtime-command:1".into()
            }
        );
    }

    #[test]
    fn mismatched_terminal_is_rejected_instead_of_crossing_cycles() {
        let error = correlate_provider_cycle(
            cycle_authority(),
            native_cycle(Some("provider-input:old")),
            true,
            false,
        )
        .unwrap_err();
        assert!(error.contains("PROVIDER_CYCLE_TERMINAL_MISMATCH"));
    }

    #[test]
    fn accepted_input_without_terminal_is_unknown_and_not_a_retry_signal() {
        let command: firm_core::agentfirm_api::RuntimeCommandRecord =
            serde_json::from_value(serde_json::json!({
                "id": "runtime-command:accepted-input",
                "execution_space_id": "space",
                "target_node_id": "node",
                "target_node_daemon_id": "daemon",
                "target_node_daemon_generation": 1,
                "authenticated_actor": {"kind": "service", "id": "daemon"},
                "command": "start_cycle",
                "required_capability": "cycle.start",
                "idempotency_key": "cycle",
                "request_fingerprint": "sha256:test",
                "status": "applied",
                "phase": "settled",
                "effect_certainty": "applied",
                "postcondition_status": "satisfied",
                "binding": {},
                "precondition": {},
                "postcondition": {},
                "provider_attempt": 1,
                "version": 2,
                "created_at": "t0",
                "updated_at": "t1"
            }))
            .unwrap();
        assert_eq!(
            durable_provider_cycle_outcome(&command),
            CycleOutcome::Unknown {
                recovery_ref: "runtime-command:accepted-input".into()
            }
        );
    }

    #[test]
    fn only_proven_not_applied_effects_receive_a_bounded_new_attempt() {
        let not_applied = ProviderEffectOutcome::NotApplied {
            reason: "spawn failed before provider input".into(),
        };
        assert_eq!(
            provider_retry_authority(&not_applied, 1, 3),
            ProviderRetryAuthority::RetryWithNewAttempt { next_attempt: 2 }
        );
        assert_eq!(
            provider_retry_authority(&not_applied, 3, 3),
            ProviderRetryAuthority::StopNoRetry
        );
        assert_eq!(
            provider_retry_authority(
                &ProviderEffectOutcome::Accepted {
                    receipt_id: "provider-receipt:1".into(),
                },
                1,
                3,
            ),
            ProviderRetryAuthority::StopNoRetry
        );
        assert_eq!(
            provider_retry_authority(
                &ProviderEffectOutcome::Unknown {
                    recovery_ref: "runtime-command:1".into(),
                },
                1,
                3,
            ),
            ProviderRetryAuthority::RequireReconciliation {
                recovery_ref: "runtime-command:1".into()
            }
        );
    }
}
