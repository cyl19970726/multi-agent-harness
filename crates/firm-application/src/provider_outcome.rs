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
