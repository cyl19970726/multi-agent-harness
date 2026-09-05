use firm_core::agentfirm_api::{
    is_lost_runtime_generation_delivery_failure_code, CanonicalWorkDelivery, WorkDeliveryStatus,
};
use firm_core::{HostAttention, HostAttentionStatus, Validate};
use std::fmt;

/// Result of applying one durable projection record to the current fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionFoldDecision {
    Insert,
    Replay,
    Advance,
}

/// Stable failure categories for the two current source/lifecycle projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionFoldViolation {
    InvalidSnapshot,
    ImmutableIdentityConflict,
    VersionRegression,
    VersionGap,
    SameVersionConflict,
    InvalidLifecycleTransition,
}

impl fmt::Display for ProjectionFoldViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSnapshot => "invalid snapshot",
            Self::ImmutableIdentityConflict => "immutable identity conflict",
            Self::VersionRegression => "version regression",
            Self::VersionGap => "version gap",
            Self::SameVersionConflict => "same version has different content",
            Self::InvalidLifecycleTransition => "invalid lifecycle transition",
        })
    }
}

impl std::error::Error for ProjectionFoldViolation {}

/// Fold one canonical WorkDelivery revision. Work delivery has no legacy
/// ledger: every accepted row is a full canonical trust side record.
pub fn fold_canonical_work_delivery(
    current: Option<&CanonicalWorkDelivery>,
    next: &CanonicalWorkDelivery,
) -> Result<ProjectionFoldDecision, ProjectionFoldViolation> {
    if let Some(current) = current {
        if !same_work_delivery_identity(current, next) {
            return Err(ProjectionFoldViolation::ImmutableIdentityConflict);
        }
        if next.version < current.version {
            return Err(ProjectionFoldViolation::VersionRegression);
        }
        if next.version == current.version {
            return if next == current {
                Ok(ProjectionFoldDecision::Replay)
            } else {
                Err(ProjectionFoldViolation::SameVersionConflict)
            };
        }
        if next.version != current.version.saturating_add(1) {
            return Err(ProjectionFoldViolation::VersionGap);
        }
        if !valid_work_delivery_transition(current, next) {
            return Err(ProjectionFoldViolation::InvalidLifecycleTransition);
        }
        return Ok(ProjectionFoldDecision::Advance);
    }
    if valid_initial_work_delivery(next) {
        Ok(ProjectionFoldDecision::Insert)
    } else {
        Err(ProjectionFoldViolation::InvalidSnapshot)
    }
}

/// Canonical HostAttention source records are immutable causal facts. They are
/// always initial Actionable snapshots; delivery lifecycle belongs to the
/// mutable HostAttention ledger.
pub fn fold_host_attention_source(
    current: Option<&HostAttention>,
    next: &HostAttention,
) -> Result<ProjectionFoldDecision, ProjectionFoldViolation> {
    if !valid_host_attention_source(next) {
        return Err(ProjectionFoldViolation::InvalidSnapshot);
    }
    match current {
        None => Ok(ProjectionFoldDecision::Insert),
        Some(current) if current == next => Ok(ProjectionFoldDecision::Replay),
        Some(_) => Err(ProjectionFoldViolation::ImmutableIdentityConflict),
    }
}

/// Fold one HostAttention lifecycle row. `None` intentionally admits a
/// structurally valid legacy-only row: it remains a read-compatible lifecycle
/// projection, but can never manufacture a canonical WorkDelivery fallback.
pub fn fold_host_attention_lifecycle(
    current: Option<&HostAttention>,
    next: &HostAttention,
) -> Result<ProjectionFoldDecision, ProjectionFoldViolation> {
    if next.validate().is_err() {
        return Err(ProjectionFoldViolation::InvalidSnapshot);
    }
    let Some(current) = current else {
        return Ok(ProjectionFoldDecision::Insert);
    };
    if !same_host_attention_identity(current, next) {
        return Err(ProjectionFoldViolation::ImmutableIdentityConflict);
    }
    if current == next {
        return Ok(ProjectionFoldDecision::Replay);
    }
    if !valid_host_attention_transition(current, next) {
        return Err(ProjectionFoldViolation::InvalidLifecycleTransition);
    }
    Ok(ProjectionFoldDecision::Advance)
}

pub fn same_host_attention_identity(left: &HostAttention, right: &HostAttention) -> bool {
    left.id == right.id
        && left.team_run_id == right.team_run_id
        && left.kind == right.kind
        && left.work_id == right.work_id
        && left.work_version == right.work_version
        && left.source_event_ref == right.source_event_ref
        && left.member_run_id == right.member_run_id
        && left.created_at == right.created_at
}

fn same_work_delivery_identity(
    left: &CanonicalWorkDelivery,
    right: &CanonicalWorkDelivery,
) -> bool {
    left.id == right.id
        && left.work_id == right.work_id
        && left.work_revision == right.work_revision
        && left.work_execution_binding_id == right.work_execution_binding_id
        && left.recipient_agent_member_id == right.recipient_agent_member_id
        && left.recipient_session_id == right.recipient_session_id
        && left.recipient_session_generation == right.recipient_session_generation
        && left.target_node_id == right.target_node_id
        && left.created_at == right.created_at
}

fn valid_initial_work_delivery(row: &CanonicalWorkDelivery) -> bool {
    !row.id.is_empty()
        && !row.work_id.is_empty()
        && row.work_revision > 0
        && !row.work_execution_binding_id.is_empty()
        && !row.recipient_agent_member_id.is_empty()
        && !row.recipient_session_id.is_empty()
        && row.recipient_session_generation > 0
        && !row.target_node_id.is_empty()
        && row.status == WorkDeliveryStatus::Queued
        && row.attempt == 1
        && row.claim_id.is_none()
        && row.claimed_node_daemon_generation.is_none()
        && row.provider_receipt_id.is_none()
        && row.failure_code.is_none()
        && row.version == 1
        && !row.created_at.is_empty()
        && row.updated_at == row.created_at
}

fn valid_work_delivery_transition(
    current: &CanonicalWorkDelivery,
    next: &CanonicalWorkDelivery,
) -> bool {
    if next.attempt != current.attempt || next.updated_at.is_empty() {
        return false;
    }
    match (current.status, next.status) {
        (WorkDeliveryStatus::Queued, WorkDeliveryStatus::Claimed) => {
            current.claim_id.is_none()
                && next
                    .claim_id
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                && next
                    .claimed_node_daemon_generation
                    .is_some_and(|value| value > 0)
                && next.provider_receipt_id.is_none()
                && next.failure_code.is_none()
        }
        (WorkDeliveryStatus::Queued, WorkDeliveryStatus::Failed) => {
            current.claim_id.is_none()
                && next.claim_id.is_none()
                && next.claimed_node_daemon_generation.is_none()
                && next.provider_receipt_id.is_none()
                && next.failure_code.as_deref()
                    == Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM")
        }
        (WorkDeliveryStatus::Claimed, WorkDeliveryStatus::ProviderReceived) => {
            same_work_delivery_claim(current, next)
                && next
                    .provider_receipt_id
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                && next.failure_code.is_none()
        }
        (WorkDeliveryStatus::Claimed, WorkDeliveryStatus::Failed) => {
            same_work_delivery_claim(current, next)
                && next.provider_receipt_id.is_none()
                && next
                    .failure_code
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
        }
        // A delivery the provider already received can only be superseded,
        // never completed, by a settlement writer: the exact runtime
        // generation that received it is provably gone. A NodeDaemon drain or
        // an Operator predecessor recovery proves the owned provider process
        // groups terminated; a Host lost-execution recovery (DEV-230) proves
        // only that the delivery's exact MemberRun/AgentSession generation can
        // never pass the runtime fence again, so an orphaned process may still
        // run but no outcome for it can ever be recorded. The immutable provider
        // receipt stays on the row as evidence of what did cross the provider
        // boundary, and only the named lost-generation codes may claim this
        // transition, so nothing can record it as a semantic turn result.
        (WorkDeliveryStatus::ProviderReceived, WorkDeliveryStatus::Failed) => {
            same_work_delivery_claim(current, next)
                && next.provider_receipt_id == current.provider_receipt_id
                && next
                    .provider_receipt_id
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                && next
                    .failure_code
                    .as_deref()
                    .is_some_and(is_lost_runtime_generation_delivery_failure_code)
        }
        _ => false,
    }
}

fn same_work_delivery_claim(current: &CanonicalWorkDelivery, next: &CanonicalWorkDelivery) -> bool {
    current.claim_id.is_some()
        && next.claim_id == current.claim_id
        && next.claimed_node_daemon_generation == current.claimed_node_daemon_generation
}

fn valid_host_attention_source(row: &HostAttention) -> bool {
    row.validate().is_ok()
        && row.status == HostAttentionStatus::Actionable
        && row.attempt == 0
        && row.last_failure_reason.is_none()
        && row.updated_at == row.created_at
}

fn valid_host_attention_transition(current: &HostAttention, next: &HostAttention) -> bool {
    if next.updated_at.is_empty() {
        return false;
    }
    match (current.status, next.status) {
        (HostAttentionStatus::Actionable, HostAttentionStatus::Claimed) => {
            next.attempt == current.attempt.saturating_add(1) && next.last_failure_reason.is_none()
        }
        (HostAttentionStatus::Claimed, HostAttentionStatus::Actionable) => {
            next.attempt == current.attempt && next.last_failure_reason.is_some()
        }
        (HostAttentionStatus::Claimed, HostAttentionStatus::Delivered)
        | (HostAttentionStatus::Claimed, HostAttentionStatus::Acknowledged) => {
            next.attempt == current.attempt
                && same_host_attention_claim(current, next)
                && next.provider_receipt_id.is_some()
                && next.last_failure_reason == current.last_failure_reason
        }
        (HostAttentionStatus::Delivered, HostAttentionStatus::Acknowledged) => {
            next.attempt == current.attempt
                && same_host_attention_claim(current, next)
                && next.provider_receipt_id == current.provider_receipt_id
                && next.last_failure_reason == current.last_failure_reason
        }
        (HostAttentionStatus::Actionable, HostAttentionStatus::EscalationRequired)
        | (HostAttentionStatus::Claimed, HostAttentionStatus::EscalationRequired) => {
            next.attempt == current.attempt && next.last_failure_reason.is_some()
        }
        _ => false,
    }
}

fn same_host_attention_claim(current: &HostAttention, next: &HostAttention) -> bool {
    current.claim_id.is_some()
        && next.claim_id == current.claim_id
        && next.claimed_host_surface == current.claimed_host_surface
        && next.claimed_host_thread_id == current.claimed_host_thread_id
        && next.claimed_host_lease_id == current.claimed_host_lease_id
        && next.claimed_host_lease_generation == current.claimed_host_lease_generation
        && next.claimed_host_lease_owner_id == current.claimed_host_lease_owner_id
        && next.claimed_recipient_member_run_id == current.claimed_recipient_member_run_id
        && next.claimed_recipient_session_id == current.claimed_recipient_session_id
        && next.claimed_recipient_session_generation == current.claimed_recipient_session_generation
        && next.claimed_node_daemon_id == current.claimed_node_daemon_id
        && next.claimed_node_daemon_generation == current.claimed_node_daemon_generation
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_core::HostAttentionKind;

    fn queued_delivery() -> CanonicalWorkDelivery {
        CanonicalWorkDelivery {
            id: "delivery-1".into(),
            work_id: "work-1".into(),
            work_revision: 2,
            work_execution_binding_id: "binding-1".into(),
            recipient_agent_member_id: "member-1".into(),
            recipient_session_id: "session-1".into(),
            recipient_session_generation: 1,
            target_node_id: "node-1".into(),
            status: WorkDeliveryStatus::Queued,
            attempt: 1,
            claim_id: None,
            claimed_node_daemon_generation: None,
            provider_receipt_id: None,
            failure_code: None,
            version: 1,
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    fn source_attention() -> HostAttention {
        HostAttention {
            id: "attention-1".into(),
            team_run_id: "run-1".into(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: "work-1".into(),
            work_version: 2,
            source_event_ref: "event-1".into(),
            member_run_id: Some("member-run-1".into()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            claimed_recipient_member_run_id: None,
            claimed_recipient_session_id: None,
            claimed_recipient_session_generation: None,
            claimed_node_daemon_id: None,
            claimed_node_daemon_generation: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    #[test]
    fn canonical_work_delivery_fold_is_versioned_and_identity_fenced() {
        let queued = queued_delivery();
        assert_eq!(
            fold_canonical_work_delivery(None, &queued),
            Ok(ProjectionFoldDecision::Insert)
        );
        assert_eq!(
            fold_canonical_work_delivery(Some(&queued), &queued),
            Ok(ProjectionFoldDecision::Replay)
        );

        let mut claimed = queued.clone();
        claimed.status = WorkDeliveryStatus::Claimed;
        claimed.claim_id = Some("claim-1".into());
        claimed.claimed_node_daemon_generation = Some(3);
        claimed.version = 2;
        claimed.updated_at = "t2".into();
        assert_eq!(
            fold_canonical_work_delivery(Some(&queued), &claimed),
            Ok(ProjectionFoldDecision::Advance)
        );

        let mut released_before_claim = queued.clone();
        released_before_claim.status = WorkDeliveryStatus::Failed;
        released_before_claim.failure_code =
            Some("WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM".into());
        released_before_claim.version = 2;
        released_before_claim.updated_at = "t-release".into();
        assert_eq!(
            fold_canonical_work_delivery(Some(&queued), &released_before_claim),
            Ok(ProjectionFoldDecision::Advance)
        );

        let mut forged = claimed.clone();
        forged.recipient_session_id = "successor-session".into();
        forged.version = 3;
        assert_eq!(
            fold_canonical_work_delivery(Some(&claimed), &forged),
            Err(ProjectionFoldViolation::ImmutableIdentityConflict)
        );
        assert_eq!(
            fold_canonical_work_delivery(Some(&claimed), &queued),
            Err(ProjectionFoldViolation::VersionRegression)
        );

        let mut same_version_drift = claimed.clone();
        same_version_drift.updated_at = "forged".into();
        assert_eq!(
            fold_canonical_work_delivery(Some(&claimed), &same_version_drift),
            Err(ProjectionFoldViolation::SameVersionConflict)
        );
    }

    #[test]
    fn host_attention_fold_separates_source_identity_from_lifecycle() {
        let source = source_attention();
        assert_eq!(
            fold_host_attention_source(None, &source),
            Ok(ProjectionFoldDecision::Insert)
        );

        let mut claimed = source.clone();
        claimed.status = HostAttentionStatus::Claimed;
        claimed.attempt = 1;
        claimed.claim_id = Some("claim-1".into());
        claimed.claimed_host_surface = Some("codex-app".into());
        claimed.claimed_host_thread_id = Some("thread-1".into());
        claimed.updated_at = "t2".into();
        assert_eq!(
            fold_host_attention_lifecycle(None, &claimed),
            Ok(ProjectionFoldDecision::Insert),
            "a structurally valid legacy-only HostAttention stays readable"
        );
        assert_eq!(
            fold_host_attention_lifecycle(Some(&source), &claimed),
            Ok(ProjectionFoldDecision::Advance)
        );

        let mut delivered = claimed.clone();
        delivered.status = HostAttentionStatus::Delivered;
        delivered.provider_receipt_id = Some("receipt-1".into());
        delivered.updated_at = "t3".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&claimed), &delivered),
            Ok(ProjectionFoldDecision::Advance)
        );

        let mut poisoned_delivery = delivered.clone();
        poisoned_delivery.last_failure_reason = Some("forged failure".into());
        assert_eq!(
            fold_host_attention_lifecycle(Some(&claimed), &poisoned_delivery),
            Err(ProjectionFoldViolation::InvalidLifecycleTransition)
        );

        let mut acknowledged = delivered.clone();
        acknowledged.status = HostAttentionStatus::Acknowledged;
        acknowledged.updated_at = "t4".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&delivered), &acknowledged),
            Ok(ProjectionFoldDecision::Advance)
        );
        let mut poisoned_ack = acknowledged.clone();
        poisoned_ack.last_failure_reason = Some("forged failure".into());
        assert_eq!(
            fold_host_attention_lifecycle(Some(&delivered), &poisoned_ack),
            Err(ProjectionFoldViolation::InvalidLifecycleTransition)
        );

        let mut retryable = claimed.clone();
        retryable.status = HostAttentionStatus::Actionable;
        retryable.claim_id = None;
        retryable.claimed_host_surface = None;
        retryable.claimed_host_thread_id = None;
        retryable.last_failure_reason = Some("transport failed".into());
        retryable.updated_at = "t4".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&claimed), &retryable),
            Ok(ProjectionFoldDecision::Advance)
        );
        let mut retry_claim = claimed.clone();
        retry_claim.attempt = 2;
        retry_claim.claim_id = Some("claim-2".into());
        retry_claim.last_failure_reason = None;
        retry_claim.updated_at = "t5".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&retryable), &retry_claim),
            Ok(ProjectionFoldDecision::Advance)
        );
        let mut poisoned_retry_claim = retry_claim.clone();
        poisoned_retry_claim.last_failure_reason = retryable.last_failure_reason.clone();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&retryable), &poisoned_retry_claim),
            Err(ProjectionFoldViolation::InvalidLifecycleTransition)
        );

        let mut out_of_order = source.clone();
        out_of_order.updated_at = "t4".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&delivered), &out_of_order),
            Err(ProjectionFoldViolation::InvalidLifecycleTransition)
        );

        let mut forged = delivered.clone();
        forged.work_id = "forged-work".into();
        assert_eq!(
            fold_host_attention_lifecycle(Some(&delivered), &forged),
            Err(ProjectionFoldViolation::ImmutableIdentityConflict)
        );
    }
}
