use harness_core::agentfirm_api::{
    NativeContinuationProjection, RuntimeEffectCertainty, RuntimePostconditionStatus,
};
use harness_core::ProviderBindingAdmission;

use crate::RuntimeContractError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTerminalFailure {
    pub reason: String,
    pub http_status: Option<i64>,
}

impl ProviderTerminalFailure {
    const STATUS_PREFIX: &'static str = "provider_terminal";

    pub fn to_provider_status(&self) -> String {
        let status = self
            .http_status
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string());
        format!("{}:{}:{status}", Self::STATUS_PREFIX, self.reason.trim())
    }

    pub fn parse(provider_status: &str) -> Option<Self> {
        let rest = provider_status.strip_prefix(Self::STATUS_PREFIX)?;
        let (reason, status) = rest.strip_prefix(':')?.rsplit_once(':')?;
        Some(Self {
            reason: reason.to_string(),
            http_status: status.parse().ok(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub admission: ProviderBindingAdmission,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeObservation {
    pub native_session_ref: Option<String>,
    pub active_effect_id: Option<String>,
    pub continuation: NativeContinuationProjection,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInspection {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReceipt {
    pub effect_id: String,
    pub certainty: RuntimeEffectCertainty,
    pub postcondition: RuntimePostconditionStatus,
    pub native_evidence: Vec<String>,
}

/// Complete postcondition required before composition swap, driver handoff,
/// or runtime release. Every field must be `Satisfied`; `Unknown` and
/// `Unsatisfied` are both failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QuiesceReceipt {
    pub admission_fenced: RuntimePostconditionStatus,
    pub continuation_inhibited: RuntimePostconditionStatus,
    pub active_cycle_terminal: RuntimePostconditionStatus,
    pub native_queue_empty: RuntimePostconditionStatus,
    pub writable_children_drained: RuntimePostconditionStatus,
    pub idle_observed: RuntimePostconditionStatus,
    pub flush_confirmed: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

impl QuiesceReceipt {
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
        let mut fields = Vec::new();
        macro_rules! satisfied {
            ($field:ident) => {
                if self.$field != RuntimePostconditionStatus::Satisfied {
                    fields.push(format!("{}={:?}", stringify!($field), self.$field));
                }
            };
        }
        satisfied!(admission_fenced);
        satisfied!(continuation_inhibited);
        satisfied!(active_cycle_terminal);
        satisfied!(native_queue_empty);
        satisfied!(writable_children_drained);
        satisfied!(idle_observed);
        satisfied!(flush_confirmed);
        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::QuiesceIncomplete { fields })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuiesceStep {
    FenceAdmission,
    InhibitContinuation,
    SettleActiveCycle,
    DrainNativeQueue,
    DrainWritableChildren,
    ObserveIdle,
    ConfirmFlush,
}

impl QuiesceStep {
    pub(crate) const ORDER: [Self; 7] = [
        Self::FenceAdmission,
        Self::InhibitContinuation,
        Self::SettleActiveCycle,
        Self::DrainNativeQueue,
        Self::DrainWritableChildren,
        Self::ObserveIdle,
        Self::ConfirmFlush,
    ];
}

pub struct QuiesceReceiptBuilder {
    next: usize,
    conditions: [RuntimePostconditionStatus; 7],
    evidence: Vec<String>,
}

impl Default for QuiesceReceiptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl QuiesceReceiptBuilder {
    pub fn new() -> Self {
        Self {
            next: 0,
            conditions: [RuntimePostconditionStatus::Unknown; 7],
            evidence: Vec::new(),
        }
    }

    pub fn record(
        &mut self,
        step: QuiesceStep,
        condition: RuntimePostconditionStatus,
        evidence: impl Into<String>,
    ) -> Result<(), RuntimeContractError> {
        let expected = QuiesceStep::ORDER.get(self.next).copied();
        if expected != Some(step) {
            return Err(RuntimeContractError::QuiesceOrder {
                expected,
                actual: step,
            });
        }
        self.conditions[self.next] = condition;
        self.evidence.push(evidence.into());
        self.next += 1;
        Ok(())
    }

    pub fn finish(self) -> QuiesceReceipt {
        QuiesceReceipt {
            admission_fenced: self.conditions[0],
            continuation_inhibited: self.conditions[1],
            active_cycle_terminal: self.conditions[2],
            native_queue_empty: self.conditions[3],
            writable_children_drained: self.conditions[4],
            idle_observed: self.conditions[5],
            flush_confirmed: self.conditions[6],
            evidence: self.evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReleaseReceipt {
    pub native_runtime_released: RuntimePostconditionStatus,
    pub live_handle_disposed: RuntimePostconditionStatus,
    pub authority_detached: RuntimePostconditionStatus,
    pub flush_confirmed: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

/// Receipt for reversible Team-member Close. It does not claim a complete
/// execution-lane quiesce, writable-workspace drain, or native-store flush;
/// those stronger facts belong exclusively to [`QuiesceReceipt`] and
/// [`ReleaseReceipt`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemberRuntimeCloseReceipt {
    pub control_acknowledged: RuntimePostconditionStatus,
    pub current_cycle_terminal: RuntimePostconditionStatus,
    pub managed_runtime_released: RuntimePostconditionStatus,
    pub live_handle_disposed: RuntimePostconditionStatus,
    pub native_session_retained: RuntimePostconditionStatus,
    pub evidence: Vec<String>,
}

impl MemberRuntimeCloseReceipt {
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
        let mut fields = Vec::new();
        macro_rules! satisfied {
            ($field:ident) => {
                if self.$field != RuntimePostconditionStatus::Satisfied {
                    fields.push(format!("{}={:?}", stringify!($field), self.$field));
                }
            };
        }
        satisfied!(control_acknowledged);
        satisfied!(current_cycle_terminal);
        satisfied!(managed_runtime_released);
        satisfied!(live_handle_disposed);
        satisfied!(native_session_retained);
        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::MemberCloseIncomplete { fields })
        }
    }
}

impl ReleaseReceipt {
    pub fn verify(&self) -> Result<(), RuntimeContractError> {
        if self.native_runtime_released == RuntimePostconditionStatus::Satisfied
            && self.live_handle_disposed == RuntimePostconditionStatus::Satisfied
            && self.authority_detached == RuntimePostconditionStatus::Satisfied
            && self.flush_confirmed == RuntimePostconditionStatus::Satisfied
        {
            Ok(())
        } else {
            Err(RuntimeContractError::ReleaseIncomplete)
        }
    }
}
